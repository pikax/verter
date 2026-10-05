//! A slow editor in front of the real transport writer.
//!
//! The server speaks LSP through [`verter_lsp::outbound::serve`] over an
//! in-memory pipe whose client end is not read while an edit storm publishes
//! diagnostics, control notifications pile up and requests run, so every byte
//! the server emits backs up exactly as it would behind an editor that stopped
//! draining stdout. The case binds the transport contract:
//!
//! * while the client is stalled every class stays within its own budget —
//!   control, response and replaceable bytes are accounted separately, and the
//!   control producers left waiting are accounted too — and once the client
//!   reads again every document ends on its newest complete diagnostics, never
//!   a superseded set after a newer one;
//! * `$/cancelRequest`, `shutdown` and the control backlog are written ahead of
//!   the diagnostics backlog rather than behind it, and a server→client request
//!   is answered while that backlog stands;
//! * a client that never asked for partial results receives a large response
//!   whole, regardless of the response byte budget.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt as _;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::sync::Notify;
use tower_lsp_server::jsonrpc::{ErrorCode, Result};
use tower_lsp_server::ls_types::notification::{LogMessage, ShowMessage};
use tower_lsp_server::ls_types::request::WorkspaceConfiguration;
use tower_lsp_server::ls_types::{
    ConfigurationParams, Diagnostic, InitializeParams, InitializeResult, LogMessageParams,
    MessageType, PublishDiagnosticsParams, Range, ShowMessageParams, Uri,
};
use tower_lsp_server::{LanguageServer, LspService};
use verter_lsp::outbound::{ClassBudget, Delivery, Load, Outbound, OutboundBudget};

const DOCUMENTS: usize = 48;
const EDITS: u64 = 40;
const WARNINGS: usize = 32;
const LARGE_RESULT_ITEMS: usize = 20_000;
/// Every byte the server emits sits in the pipe until the test reads it.
const PIPE_BYTES: usize = 4 * 1024;

/// The smallest server that exercises the transport: one request that runs until
/// cancelled, one that returns a large complete result, one that asks the client
/// a question, and `shutdown`.
struct Probe {
    outbound: Outbound,
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

    async fn ask_client(&self, _: Value) -> Result<Vec<Value>> {
        self.outbound
            .send_request::<WorkspaceConfiguration>(ConfigurationParams { items: Vec::new() })
            .await
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

fn current() -> bool {
    true
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_client_gets_bounded_backlog_prompt_control_and_newest_diagnostics() {
    let budget = OutboundBudget {
        control: ClassBudget {
            messages: 8,
            bytes: 64 * 1024,
        },
        response: ClassBudget {
            messages: 4,
            bytes: 1024 * 1024,
        },
        replaceable: ClassBudget {
            messages: 4,
            bytes: 1024 * 1024,
        },
    };
    let outbound = Outbound::new(budget);
    let lane = outbound.diagnostics_lane();
    let large_ran = Arc::new(Notify::new());
    let cancelled = Arc::new(Notify::new());

    let (service, _socket) = {
        let outbound = outbound.clone();
        let large_ran = Arc::clone(&large_ran);
        let cancelled = Arc::clone(&cancelled);
        LspService::build(move |_| Probe {
            outbound,
            large_ran,
            cancelled,
        })
        .custom_method("test/runUntilCancelled", Probe::run_until_cancelled)
        .custom_method("test/large", Probe::large)
        .custom_method("test/askClient", Probe::ask_client)
        .finish()
    };
    let (editor_end, server_end) = tokio::io::duplex(PIPE_BYTES);
    let (server_in, server_out) = tokio::io::split(server_end);
    let server = tokio::spawn(verter_lsp::outbound::serve(
        server_in,
        server_out,
        service,
        outbound.clone(),
    ));
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

    // Stall the writer on the client before anything else is offered: a control
    // frame larger than the pipe is the first frame it takes, and it cannot
    // finish writing it while nobody reads. Without it, a writer that had not yet
    // filled the pipe when the warnings below arrive would rightly write some of
    // them, and the stalled-state accounting would race the scheduler.
    let filler = "f".repeat(4 * PIPE_BYTES);
    outbound.notify_detached::<LogMessage>(LogMessageParams {
        typ: MessageType::LOG,
        message: filler.clone(),
    });

    // The edit storm, issued while nobody reads, from one task that owns every
    // publication. Each publication is polled once as it is issued, so every
    // document's offers enter the lane in edit order; one that already resolved
    // on that first poll is recorded instead of being polled again.
    let (offered_tx, offered) = tokio::sync::oneshot::channel();
    let storm = tokio::spawn({
        let lane = lane.clone();
        async move {
            let mut delivered = 0usize;
            let mut publications = FuturesUnordered::new();
            for edit in 1..=EDITS {
                for document in 0..DOCUMENTS {
                    let mut publish =
                        Box::pin(lane.publish_diagnostics(edit, payload(document, edit), current));
                    match futures_util::poll!(&mut publish) {
                        std::task::Poll::Ready(delivery) => {
                            delivered += usize::from(delivery == Delivery::Delivered);
                        }
                        std::task::Poll::Pending => publications.push(publish),
                    }
                }
            }
            let _ = offered_tx.send(());
            while let Some(delivery) = publications.next().await {
                delivered += usize::from(delivery == Delivery::Delivered);
            }
            delivered
        }
    });

    // Every edit is offered behind the stalled writer, so the storm backs the
    // lane up to its budget and documents wait for it.
    offered.await.expect("the storm offers every edit");
    assert!(
        outbound.load().replaceable.waiting.messages > 0,
        "the storm backs up behind the stalled client: {:?}",
        outbound.load()
    );

    // Control traffic offered behind the stalled transport by producers that
    // cannot await: more than the control budget admits.
    let warnings: Vec<String> = (0..WARNINGS).map(|i| format!("warning-{i}")).collect();
    for message in &warnings {
        outbound.notify_detached::<ShowMessage>(ShowMessageParams {
            typ: MessageType::WARNING,
            message: message.clone(),
        });
    }

    // Requests issued behind the backlog: a large result for a client that never
    // asked for partial results, one that asks the client a question, one that is
    // then cancelled, and — once the large and cancelled ones are handled —
    // `shutdown`.
    let cancelled_seen = cancelled.notified();
    let large_seen = large_ran.notified();
    tokio::pin!(cancelled_seen, large_seen);
    editor
        .send(json!({"jsonrpc": "2.0", "id": 4, "method": "test/large", "params": {}}))
        .await;
    editor
        .send(json!({"jsonrpc": "2.0", "id": 5, "method": "test/askClient", "params": {}}))
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
    // Both handled requests have produced their responses, which the stalled
    // writer cannot write: they are held in the response class.
    tokio::time::timeout(Duration::from_secs(30), async {
        while outbound.load().response.admitted.messages < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the large and cancelled responses are produced while the output is stalled");

    // Mixed-class accounting while the client is stalled: each class within its
    // own budget, the control producers left waiting accounted with their bytes.
    let stalled = outbound.load();
    assert!(
        stalled.control.admitted.messages <= budget.control.messages
            && stalled.control.admitted.bytes <= budget.control.bytes,
        "control exceeded its budget while stalled: {stalled:?}"
    );
    let warning_bytes = |message: &str| {
        serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "method": "window/showMessage",
            "params": {"type": 2, "message": message},
        }))
        .unwrap()
        .len()
    };
    assert!(
        stalled.control.retained().messages > WARNINGS,
        "every offered control message is still the transport's, and counted: {stalled:?}"
    );
    assert!(
        stalled.control.waiting.bytes
            >= warnings[budget.control.messages + 1..]
                .iter()
                .map(|m| warning_bytes(m))
                .sum::<usize>(),
        "waiting control messages are accounted with their bytes: {stalled:?}"
    );
    assert!(
        stalled.response.admitted.messages <= budget.response.messages,
        "responses exceeded their budget while stalled: {stalled:?}"
    );
    assert!(
        stalled.response.admitted.messages >= 2
            && stalled.response.admitted.bytes > LARGE_RESULT_ITEMS * "\"item-0\",".len(),
        "the large and cancelled responses are held, accounted with their bytes, while \n         stalled: {stalled:?}"
    );
    assert!(
        stalled.replaceable.admitted.messages <= budget.replaceable.messages
            && stalled.replaceable.admitted.bytes <= budget.replaceable.bytes,
        "diagnostics exceeded their budget while stalled: {stalled:?}"
    );

    editor
        .send(json!({"jsonrpc": "2.0", "id": 3, "method": "shutdown"}))
        .await;

    let mut responses: HashMap<u64, Value> = HashMap::new();
    let mut shown = Vec::new();
    let mut stalling_frames = 0usize;
    let mut diagnostics_before_control = 0usize;
    let mut newest: HashMap<String, (u64, usize)> = HashMap::new();
    let mut final_sets: HashMap<String, Value> = HashMap::new();
    let complete = |newest: &HashMap<String, (u64, usize)>| {
        newest.len() == DOCUMENTS && newest.values().all(|(edit, _)| *edit == EDITS)
    };
    tokio::time::timeout(Duration::from_secs(60), async {
        while responses.len() < 4 || shown.len() < WARNINGS || !complete(&newest) {
            let message = editor.recv().await;
            if message.get("method") == Some(&json!("workspace/configuration")) {
                // A server→client request: answer it.
                editor
                    .send(json!({"jsonrpc": "2.0", "id": message["id"], "result": [{"answered": true}]}))
                    .await;
                continue;
            }
            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                responses.insert(id, message);
                continue;
            }
            if message["method"] == "window/logMessage" {
                assert!(
                    responses.is_empty() && shown.is_empty() && newest.is_empty(),
                    "the frame that stalled the writer is written first"
                );
                assert_eq!(
                    message["params"]["message"], filler,
                    "the stalling frame is written whole"
                );
                stalling_frames += 1;
                continue;
            }
            if message["method"] == "window/showMessage" {
                shown.push(message["params"]["message"].as_str().unwrap().to_owned());
                continue;
            }
            assert_eq!(message["method"], "textDocument/publishDiagnostics");
            let control_done = responses.contains_key(&2)
                && responses.contains_key(&3)
                && shown.len() == WARNINGS;
            if !control_done {
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
    .expect("the resumed client receives every response, every control message and the newest diagnostics");

    assert_eq!(
        responses[&2]["error"]["code"],
        ErrorCode::RequestCancelled.code(),
        "the cancelled request is answered as cancelled"
    );
    assert_eq!(responses[&3]["result"], Value::Null);
    assert_eq!(
        responses[&5]["result"],
        json!([{"answered": true}]),
        "the client's reply reached the server request that asked"
    );
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
    assert_eq!(
        stalling_frames, 1,
        "the stalling frame reaches the client once"
    );
    assert_eq!(shown, warnings, "control messages keep their order");

    // The storm's backlog cannot precede the control traffic: only the few
    // frames the writer takes before the shutdown response exists can.
    assert!(
        diagnostics_before_control <= 4,
        "control waited behind {diagnostics_before_control} diagnostics"
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
    // The envelope, over everything each class ever held.
    let high_water = outbound.high_water();
    assert_eq!(
        high_water.replaceable.admitted.messages, budget.replaceable.messages,
        "the storm backs the lane up to its budget: {high_water:?}"
    );
    assert!(
        high_water.replaceable.admitted.bytes <= budget.replaceable.bytes,
        "the admitted diagnostics exceeded the byte budget: {high_water:?}"
    );
    assert!(
        high_water.replaceable.retained.messages <= budget.replaceable.messages + DOCUMENTS,
        "the lane retained more than its budget plus one payload per document: {high_water:?}"
    );
    let largest = (0..DOCUMENTS)
        .map(|document| {
            serde_json::to_vec(&json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": payload(document, EDITS),
            }))
            .unwrap()
            .len()
        })
        .max()
        .unwrap();
    assert!(
        high_water.replaceable.retained.bytes <= budget.replaceable.bytes + DOCUMENTS * largest,
        "the lane retained more bytes than its envelope: {high_water:?}"
    );
    assert!(
        high_water.control.admitted.messages <= budget.control.messages,
        "the admitted control set exceeded its budget: {high_water:?}"
    );
    assert!(
        high_water.control.retained.messages >= WARNINGS,
        "every control message was accounted while it waited: {high_water:?}"
    );
    assert!(
        high_water.response.admitted.messages <= budget.response.messages,
        "the admitted responses exceeded their budget: {high_water:?}"
    );
    let drained = outbound.load();
    assert_eq!(
        (
            drained.control.retained(),
            drained.response.retained(),
            drained.replaceable.retained()
        ),
        (Load::default(), Load::default(), Load::default()),
        "nothing is retained once the client has read everything"
    );

    // Fresh basis: publishing only the newest edit of every document again, to
    // the now-reading client, yields exactly the sets the storm ended on —
    // contents and ranges included.
    let fresh_publications = (0..DOCUMENTS)
        .map(|document| lane.publish_diagnostics(EDITS + 1, payload(document, EDITS), current))
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
        "the storm's final diagnostics equal a fresh publication of the newest edit"
    );

    editor
        .send(json!({"jsonrpc": "2.0", "method": "exit"}))
        .await;
    drop(editor);
    tokio::time::timeout(Duration::from_secs(30), server)
        .await
        .expect("the server exits")
        .unwrap();
    assert_eq!(
        lane.publish_diagnostics(EDITS + 2, payload(0, EDITS), current)
            .await,
        Delivery::Closed,
        "an exited transport takes no further publication"
    );
}
