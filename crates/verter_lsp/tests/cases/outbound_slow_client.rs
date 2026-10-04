//! A slow editor in front of the real tower-lsp writer.
//!
//! The server speaks LSP over an in-memory pipe whose client end is not read
//! while an edit storm publishes diagnostics through the production
//! [`ReplaceableLane`], so every byte the server emits backs up exactly as it
//! would behind an editor that stopped draining stdout. The cases bind the
//! transport contract:
//!
//! * the lane never retains more than its budget while the storm backs up, and
//!   once the client reads again every document ends on its newest complete
//!   diagnostics — never a superseded set after a newer one;
//! * `$/cancelRequest` and `shutdown` are answered ahead of the diagnostics
//!   backlog rather than behind it;
//! * a client that never asked for partial results receives a large response
//!   whole, regardless of the replaceable budget.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::FuturesUnordered;
use futures_util::{FutureExt as _, StreamExt as _};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::jsonrpc::{ErrorCode, Result};
use tower_lsp_server::ls_types::{
    Diagnostic, InitializeParams, InitializeResult, PublishDiagnosticsParams, Range, Uri,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};
use verter_lsp::outbound::{Delivery, OutboundBudget, ReplaceableLane};

const DOCUMENTS: usize = 48;
const EDITS: u64 = 40;
const LARGE_RESULT_ITEMS: usize = 20_000;
/// Every byte the server emits sits in the pipe until the test reads it.
const PIPE_BYTES: usize = 4 * 1024;

/// The smallest server that exercises the transport: one request that runs until
/// cancelled, one that returns a large complete result, and `shutdown`.
struct Probe {
    large_ran: Arc<Notify>,
    cancelled: Arc<Notify>,
}

impl LanguageServer for Probe {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult::default())
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Signals when the request future it lives in is dropped — the observable end
/// of a cancelled request.
struct DropSignal(Arc<Notify>);

impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.notify_one();
    }
}

impl Probe {
    async fn run_until_cancelled(&self, _: Value) -> Result<Value> {
        let _signal = DropSignal(Arc::clone(&self.cancelled));
        std::future::pending().await
    }

    async fn large(&self, _: Value) -> Result<Vec<String>> {
        self.large_ran.notify_one();
        Ok((0..LARGE_RESULT_ITEMS)
            .map(|i| format!("item-{i}"))
            .collect())
    }
}

struct Editor {
    reader: BufReader<tokio::io::ReadHalf<DuplexStream>>,
    writer: tokio::io::WriteHalf<DuplexStream>,
}

impl Editor {
    async fn send(&mut self, message: Value) {
        let body = serde_json::to_vec(&message).unwrap();
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.writer.write_all(header.as_bytes()).await.unwrap();
        self.writer.write_all(&body).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn recv(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).await.unwrap();
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; length.expect("every frame carries a length")];
        self.reader.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}

fn payload(document: usize, edit: u64) -> PublishDiagnosticsParams {
    // ~2 KiB per publication, so the transport's own buffers hold only a few.
    let diagnostics = (0..16)
        .map(|i| {
            Diagnostic::new_simple(
                Range::default(),
                format!("doc-{document} edit-{edit} #{i} {}", "x".repeat(96)),
            )
        })
        .collect();
    PublishDiagnosticsParams::new(
        format!("file:///workspace/Doc{document}.vue")
            .parse::<Uri>()
            .unwrap(),
        diagnostics,
        Some(edit as i32),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_client_gets_bounded_backlog_prompt_control_and_newest_diagnostics() {
    let budget = OutboundBudget {
        replaceable_messages: 4,
        replaceable_bytes: 1024 * 1024,
    };
    let lane = ReplaceableLane::new(budget);
    let large_ran = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());
    let client_cell: Arc<OnceCell<Client>> = Arc::new(OnceCell::new());

    let (service, socket) = {
        let large_ran = Arc::clone(&large_ran);
        let cancelled = Arc::clone(&cancelled);
        let client_cell = Arc::clone(&client_cell);
        LspService::build(move |client| {
            let _ = client_cell.set(client);
            Probe {
                large_ran,
                cancelled,
            }
        })
        .custom_method("test/runUntilCancelled", Probe::run_until_cancelled)
        .custom_method("test/large", Probe::large)
        .finish()
    };
    let (editor_end, server_end) = tokio::io::duplex(PIPE_BYTES);
    let (server_in, server_out) = tokio::io::split(server_end);
    let server = tokio::spawn(
        Server::new(server_in, server_out, socket)
            .concurrency_level(verter_lsp::LSP_MAX_CONCURRENCY)
            .serve(service),
    );
    let (editor_in, editor_out) = tokio::io::split(editor_end);
    let mut editor = Editor {
        reader: BufReader::new(editor_in),
        writer: editor_out,
    };

    editor
        .send(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}))
        .await;
    assert_eq!(editor.recv().await["id"], 1);
    editor
        .send(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}))
        .await;
    let client = client_cell
        .get()
        .expect("the service built its client")
        .clone();

    // The edit storm, issued while nobody reads. Each publication is polled once
    // as it is issued, so every document's offers enter the lane in edit order.
    let mut storm = FuturesUnordered::new();
    for edit in 1..=EDITS {
        for document in 0..DOCUMENTS {
            let lane = lane.clone();
            let client = client.clone();
            let mut publish = async move {
                lane.publish_diagnostics(&client, edit, payload(document, edit))
                    .await
            }
            .boxed();
            let _ = futures_util::poll!(&mut publish);
            storm.push(publish);
        }
    }
    let storm = tokio::spawn(async move {
        let mut delivered = 0usize;
        while let Some(delivery) = storm.next().await {
            delivered += usize::from(delivery == Delivery::Delivered);
        }
        delivered
    });

    // Requests issued behind the backlog: a large result for a client that never
    // asked for partial results, a request that is then cancelled, and — once
    // both are handled — `shutdown`. The large and cancelled responses are queued
    // server-side while the client is still not reading; `shutdown` is answered
    // as the client resumes.
    let cancelled_seen = cancelled.notified();
    let large_seen = large_ran.notified();
    tokio::pin!(cancelled_seen, large_seen);
    editor
        .send(json!({"jsonrpc": "2.0", "id": 4, "method": "test/large", "params": {}}))
        .await;
    editor
        .send(json!({"jsonrpc": "2.0", "id": 2, "method": "test/runUntilCancelled", "params": {}}))
        .await;
    editor
        .send(json!({"jsonrpc": "2.0", "method": "$/cancelRequest", "params": {"id": 2}}))
        .await;
    tokio::time::timeout(Duration::from_secs(30), async {
        cancelled_seen.await;
        large_seen.await;
    })
    .await
    .expect("the server handles requests and cancellation while its output is stalled");
    editor
        .send(json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"}))
        .await;

    let mut responses: HashMap<u64, Value> = HashMap::new();
    let mut diagnostics_before_control = 0usize;
    let mut newest: HashMap<String, (u64, usize)> = HashMap::new();
    let mut final_sets: HashMap<String, Value> = HashMap::new();
    let complete = |newest: &HashMap<String, (u64, usize)>| {
        newest.len() == DOCUMENTS && newest.values().all(|(edit, _)| *edit == EDITS)
    };
    tokio::time::timeout(Duration::from_secs(60), async {
        while responses.len() < 3 || !complete(&newest) {
            let message = editor.recv().await;
            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                responses.insert(id, message);
                continue;
            }
            assert_eq!(message["method"], "textDocument/publishDiagnostics");
            if !(responses.contains_key(&2) && responses.contains_key(&3)) {
                diagnostics_before_control += 1;
            }
            let params: PublishDiagnosticsParams =
                serde_json::from_value(message["params"].clone()).unwrap();
            let edit = params.version.unwrap() as u64;
            assert_eq!(
                params.diagnostics.len(),
                16,
                "a publication reaches the client as one complete replacement"
            );
            let entry = newest
                .entry(params.uri.as_str().to_owned())
                .or_insert((0, 0));
            assert!(
                edit > entry.0,
                "{} received edit {edit} after edit {}",
                params.uri.as_str(),
                entry.0
            );
            *entry = (edit, entry.1 + 1);
            final_sets.insert(params.uri.as_str().to_owned(), message["params"].clone());
        }
    })
    .await
    .expect("the resumed client receives every response and the newest diagnostics");

    assert_eq!(
        responses[&2]["error"]["code"],
        ErrorCode::RequestCancelled.code(),
        "the cancelled request is answered as cancelled"
    );
    assert_eq!(responses[&3]["result"], Value::Null);
    let large = responses[&4]["result"]
        .as_array()
        .unwrap_or_else(|| panic!("a full result array, got {}", responses[&4]));
    assert_eq!(
        large.len(),
        LARGE_RESULT_ITEMS,
        "the large result is never truncated"
    );
    assert_eq!(
        large[LARGE_RESULT_ITEMS - 1],
        format!("item-{}", LARGE_RESULT_ITEMS - 1)
    );

    // Only what the transport already held — its write buffer and the pipe — can
    // precede the control responses; the storm's backlog cannot.
    assert!(
        diagnostics_before_control <= 16,
        "cancel/shutdown waited behind {diagnostics_before_control} diagnostics"
    );
    // Coalescing: each document received a handful of complete sets, ending on
    // its newest, out of the EDITS publications issued for it.
    for (uri, (_, received)) in &newest {
        assert!(*received <= 3, "{uri} received {received} superseded sets");
    }
    let delivered = storm.await.unwrap();
    assert_eq!(
        delivered,
        newest.values().map(|(_, received)| received).sum::<usize>(),
        "every delivered publication reached the client"
    );
    // The envelope, over everything the lane ever owned: the storm filled the
    // admitted set to its budget and never past it, and every payload it held
    // back waited as its document's single newest offer.
    let high_water = lane.high_water();
    assert_eq!(
        high_water.admitted.messages, budget.replaceable_messages,
        "the storm backs the lane up to its budget: {high_water:?}"
    );
    assert!(
        high_water.admitted.bytes <= budget.replaceable_bytes,
        "the admitted set exceeded the byte budget: {high_water:?}"
    );
    assert!(
        high_water.retained.messages <= budget.replaceable_messages + DOCUMENTS,
        "the lane retained more than its budget plus one payload per document: {high_water:?}"
    );
    let largest = (0..DOCUMENTS)
        .map(|document| serde_json::to_vec(&payload(document, EDITS)).unwrap().len())
        .max()
        .unwrap();
    assert!(
        high_water.retained.bytes <= budget.replaceable_bytes + DOCUMENTS * largest,
        "the lane retained more bytes than its envelope: {high_water:?}"
    );
    assert_eq!(lane.load().retained(), Default::default());

    // Fresh basis: publishing only the newest epoch of every document, through a
    // fresh lane to the now-reading client, yields exactly the sets the storm
    // ended on — contents and ranges included.
    let fresh_lane = ReplaceableLane::new(budget);
    let fresh_publications = (0..DOCUMENTS)
        .map(|document| fresh_lane.publish_diagnostics(&client, EDITS, payload(document, EDITS)))
        .collect::<FuturesUnordered<_>>()
        .collect::<Vec<_>>();
    let fresh_reads = async {
        let mut fresh = HashMap::new();
        while fresh.len() < DOCUMENTS {
            let message = editor.recv().await;
            assert_eq!(message["method"], "textDocument/publishDiagnostics");
            let uri = message["params"]["uri"].as_str().unwrap().to_owned();
            fresh.insert(uri, message["params"].clone());
        }
        fresh
    };
    let (fresh_deliveries, fresh_sets) = tokio::time::timeout(Duration::from_secs(60), async {
        tokio::join!(fresh_publications, fresh_reads)
    })
    .await
    .expect("the fresh publication reaches the reading client");
    assert!(fresh_deliveries.iter().all(|d| *d == Delivery::Delivered));
    assert_eq!(
        final_sets, fresh_sets,
        "the storm's final diagnostics equal a fresh publication of the newest epoch"
    );

    editor
        .send(json!({"jsonrpc": "2.0", "method": "exit"}))
        .await;
    drop(editor);
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .expect("the server exits")
        .unwrap();
}
