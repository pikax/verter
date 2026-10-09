//! A tsgo answer names lines and characters in the bytes the engine held when
//! it dequeued the request. These tests play the engine over an in-memory
//! duplex and replace the provider's content between dispatch and answer:
//! every range must decode from the dispatched bytes, or the query must end in
//! the typed conflict — never a mix of the two documents.

use super::*;
use crate::provider_query::{ConflictKind, DeliveredSurfaceId};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use verter_tsgo_api::jsonrpc::framing::{encode_message, MessageFramer};

/// The engine side of an in-memory tsgo transport.
struct Engine {
    stream: tokio::io::DuplexStream,
    framer: MessageFramer,
}

impl Engine {
    /// The next message the provider wrote.
    async fn next(&mut self) -> serde_json::Value {
        let mut chunk = [0u8; 8192];
        loop {
            if let Some(message) = self.framer.next_message().expect("decode") {
                return message;
            }
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.stream.read(&mut chunk),
            )
            .await
            .expect("the provider must write its next message promptly")
            .expect("read");
            assert_ne!(read, 0, "the provider closed its stdin");
            self.framer.push(&chunk[..read]);
        }
    }

    /// The next message, which must be `method`.
    async fn expect(&mut self, method: &str) -> serde_json::Value {
        let message = self.next().await;
        assert_eq!(message["method"], method, "unexpected message {message}");
        message
    }

    async fn answer(&mut self, request: &serde_json::Value, result: serde_json::Value) {
        let response = encode_message(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": request["id"],
            "result": result,
        }));
        self.stream.write_all(&response).await.expect("write");
        self.stream.flush().await.expect("flush");
    }
}

fn provider() -> (TsgoTypeProvider, Engine) {
    let (provider_side, engine_side) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(provider_side);
    (
        TsgoTypeProvider::from_initialized_transport(read, write),
        Engine {
            stream: engine_side,
            framer: MessageFramer::new(),
        },
    )
}

fn path(name: &str) -> String {
    if cfg!(windows) {
        format!("D:/w/{name}")
    } else {
        format!("/w/{name}")
    }
}

const DISPATCHED: &str = "const alpha = 1;\nconst beta = 2;\n";
/// Shifts every line of [`DISPATCHED`] down by one, so a line/character decoded
/// against these bytes lands on a different offset.
const REPLACED: &str = "// a comment that moves every line\nconst alpha = 1;\nconst beta = 2;\n";

fn beta_offset(content: &str) -> u32 {
    content.find("beta").expect("fixture names beta") as u32
}

fn beta_range() -> serde_json::Value {
    serde_json::json!({
        "start": { "line": 1, "character": 6 },
        "end": { "line": 1, "character": 10 },
    })
}

#[tokio::test]
async fn hover_range_decodes_from_the_dispatched_bytes_across_a_content_replacement() {
    let (provider, mut engine) = provider();
    let file = path("a.ts");
    provider.update_file(&file, DISPATCHED).await.expect("open");
    engine.expect("textDocument/didOpen").await;

    let offset = beta_offset(DISPATCHED);
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        assert_eq!(hover["params"]["position"]["line"], 1);
        assert_eq!(hover["params"]["position"]["character"], 6);
        // The barrier: the content is replaced after dispatch and before the
        // engine's answer is decoded.
        provider.update_file(&file, REPLACED).await.expect("update");
        engine.expect("textDocument/didChange").await;
        engine
            .answer(
                &hover,
                serde_json::json!({
                    "contents": { "kind": "plaintext", "value": "const beta: 2" },
                    "range": beta_range(),
                }),
            )
            .await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(&file), offset),
        engine_side
    );
    let hover = hover.expect("hover").expect("an answered hover");
    assert_eq!(
        (hover.range_start, hover.range_end),
        (Some(offset), Some(offset + 4)),
        "the range names `beta` in the bytes the query was dispatched against, \
         not the line the replacement moved under it"
    );
}

#[tokio::test]
async fn references_into_another_file_decode_from_that_files_dispatched_bytes() {
    let (provider, mut engine) = provider();
    let origin = path("a.ts");
    let target = path("b.ts");
    provider
        .update_file(&origin, "import { beta } from './b';\nbeta;\n")
        .await
        .expect("open origin");
    engine.expect("textDocument/didOpen").await;
    provider
        .update_file(&target, DISPATCHED)
        .await
        .expect("open target");
    engine.expect("textDocument/didOpen").await;

    let engine_side = async {
        let references = engine.expect("textDocument/references").await;
        provider
            .update_file(&target, REPLACED)
            .await
            .expect("update");
        engine.expect("textDocument/didChange").await;
        engine
            .answer(
                &references,
                serde_json::json!([{
                    "uri": TsgoTypeProvider::path_to_uri(&target),
                    "range": beta_range(),
                }]),
            )
            .await;
    };
    let (locations, ()) = tokio::join!(
        provider.get_references(&ProviderQuery::at_engine_surface(&origin), 29),
        engine_side
    );
    let locations = locations.expect("references");
    let start = beta_offset(DISPATCHED);
    assert_eq!(locations.len(), 1);
    assert_eq!(
        (locations[0].start, locations[0].end),
        (start, start + 4),
        "a foreign target decodes through its own bytes as the request met them"
    );
}

#[tokio::test]
async fn out_of_band_bytes_republished_under_an_answer_are_a_typed_conflict() {
    // A non-owning attach learns carrier bytes through `load_file` while its
    // relay injects them out of band: no frame on this transport orders them.
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    provider.load_file(&file, DISPATCHED).await.expect("load");

    let offset = beta_offset(DISPATCHED);
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        provider
            .load_file(&file, REPLACED)
            .await
            .expect("republish");
        engine
            .answer(
                &hover,
                serde_json::json!({
                    "contents": { "kind": "plaintext", "value": "const beta: 2" },
                    "range": beta_range(),
                }),
            )
            .await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(&file), offset),
        engine_side
    );
    let error = hover.expect_err("moved out-of-band bytes must not decode");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

/// A stdin writer driven directly, so a test controls exactly which lane
/// messages precede a query frame.
struct Writer {
    lane: mpsc::Sender<StdinMessage>,
    ledger: Arc<DeliveryLedger>,
    engine_stdin: tokio::io::DuplexStream,
    /// The lanes this test never uses, held open so the writer keeps running.
    _idle: (
        mpsc::UnboundedSender<StdinMessage>,
        mpsc::Sender<StdinMessage>,
        mpsc::Sender<StdinMessage>,
    ),
}

impl Writer {
    fn spawn() -> Self {
        let (stdin, engine_stdin) = tokio::io::duplex(64 * 1024);
        let (lane, stdin_rx) = mpsc::channel::<StdinMessage>(16);
        let ledger = Arc::new(DeliveryLedger::default());
        let (control_tx, control_rx) = mpsc::unbounded_channel();
        let (normal_tx, normal_rx) = mpsc::channel(1);
        let (background_tx, background_rx) = mpsc::channel(1);
        tokio::spawn(stdin_writer_loop(
            stdin,
            control_rx,
            stdin_rx,
            normal_rx,
            background_rx,
            None,
            Arc::new(AtomicBool::new(false)),
            std::time::Duration::from_secs(WRITER_STALL_TIMEOUT_SECS),
            Arc::clone(&ledger),
        ));
        Self {
            lane,
            ledger,
            engine_stdin,
            _idle: (control_tx, normal_tx, background_tx),
        }
    }

    /// Place a delivery of `content` for `key` and wait for it to flush.
    async fn deliver(&self, key: &str, content: &str) {
        let versions = Arc::new(Mutex::new(HashMap::new()));
        let (done, delivered) = oneshot::channel();
        self.lane
            .send(StdinMessage::Document(
                b"DELIVERY;".to_vec(),
                Box::new(DocumentDelivery {
                    versions: versions.lock_owned().await,
                    contents: Arc::new(Mutex::new(HashMap::new())),
                    accepted: Arc::new(StdMutex::new(HashMap::new())),
                    key: key.to_string(),
                    value: Some((1, Arc::from(content))),
                    done,
                    diagnostics: None,
                }),
            ))
            .await
            .unwrap();
        delivered.await.expect("the delivery is flushed");
    }

    /// Enqueue `query` on `key`; the frame is the bytes it converted against.
    async fn query(&self, query: &ProviderQuery, key: &str) -> QueryPlaced {
        let prepared = self.ledger.prepare(query, key).expect("prepare");
        let (placed, bound) = oneshot::channel();
        self.lane
            .send(StdinMessage::Query(Box::new(QueryAnchor {
                prepared,
                frame: Box::new(|requested| {
                    Ok(requested.map(|bytes| format!("QUERY[{bytes}];").into_bytes()))
                }),
                placed,
            })))
            .await
            .unwrap();
        bound.await.expect("the writer answers the anchor")
    }

    async fn written(mut self) -> String {
        drop(self.lane);
        drop(self._idle);
        let mut written = Vec::new();
        self.engine_stdin.read_to_end(&mut written).await.unwrap();
        String::from_utf8(written).unwrap()
    }
}

#[tokio::test]
async fn a_query_converts_against_whatever_delivery_the_writer_placed_before_its_frame() {
    let writer = Writer::spawn();
    let key = contents_key(&path("a.ts"));
    writer.deliver(&key, DISPATCHED).await;
    // A replay of the same bytes reaches the writer just ahead of the frame,
    // then a genuine edit: the frame converts against what precedes it.
    writer.deliver(&key, DISPATCHED).await;
    let bound = writer
        .query(&ProviderQuery::at_engine_surface(&key), &key)
        .await
        .expect("placed")
        .expect("never a conflict on a wire-ordered route")
        .expect("not declined");
    assert_eq!(bound.requested().map(|b| &**b), Some(DISPATCHED));
    writer.deliver(&key, REPLACED).await;
    let bound = writer
        .query(&ProviderQuery::at_engine_surface(&key), &key)
        .await
        .expect("placed")
        .expect("bound")
        .expect("not declined");
    assert_eq!(bound.requested().map(|b| &**b), Some(REPLACED));
    assert_eq!(
        writer.written().await,
        format!("DELIVERY;DELIVERY;QUERY[{DISPATCHED}];DELIVERY;QUERY[{REPLACED}];")
    );
}

#[tokio::test]
async fn a_query_intending_bytes_the_engine_no_longer_holds_never_reaches_it() {
    let writer = Writer::spawn();
    let key = contents_key(&path("a.ts"));
    writer.deliver(&key, DISPATCHED).await;
    // The requester captured A and computed its position against it…
    let intending = ProviderQuery::intending(
        &key,
        DeliveredSurfaceId {
            generation: 1,
            content_epoch: 1,
            incarnation: 1,
        },
        Arc::from(DISPATCHED),
    );
    // …but B reaches the engine before the frame.
    writer.deliver(&key, REPLACED).await;
    let refused = writer.query(&intending, &key).await.expect("answered");
    assert_eq!(
        refused.unwrap_err().kind(),
        ConflictKind::IntendedSurface,
        "A's position is never evaluated against B"
    );
    // A delivered back before the frame is exactly what it intends.
    writer.deliver(&key, DISPATCHED).await;
    writer
        .query(&intending, &key)
        .await
        .expect("answered")
        .expect("A binds")
        .expect("not declined");
    assert_eq!(
        writer.written().await,
        format!("DELIVERY;DELIVERY;DELIVERY;QUERY[{DISPATCHED}];")
    );
}

#[tokio::test]
async fn signature_help_over_republished_out_of_band_bytes_is_a_typed_conflict() {
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    provider.load_file(&file, DISPATCHED).await.expect("load");

    let engine_side = async {
        let request = engine.expect("textDocument/signatureHelp").await;
        provider
            .load_file(&file, REPLACED)
            .await
            .expect("republish");
        engine
            .answer(
                &request,
                serde_json::json!({
                    "signatures": [{ "label": "beta(): void", "parameters": [] }],
                    "activeSignature": 0,
                }),
            )
            .await;
    };
    let (help, ()) = tokio::join!(
        provider.get_signature_help(&ProviderQuery::at_engine_surface(&file), 3),
        engine_side
    );
    let error = help.expect_err("an answer evaluated over moved bytes must not succeed");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

#[tokio::test]
async fn a_disk_target_rewritten_after_dispatch_is_a_typed_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("b.ts");
    std::fs::write(&target, DISPATCHED).expect("write target");
    let target = target.to_string_lossy().to_string();
    let (provider, mut engine) = provider();
    let origin = path("a.ts");
    provider
        .update_file(&origin, "import { beta } from './b';\nbeta;\n")
        .await
        .expect("open origin");
    engine.expect("textDocument/didOpen").await;

    let engine_side = async {
        let references = engine.expect("textDocument/references").await;
        // The engine evaluated the disk bytes; a writer replaces them before
        // the answer is decoded.
        std::fs::write(&target, REPLACED).expect("rewrite target");
        std::fs::File::options()
            .write(true)
            .open(&target)
            .and_then(|file| {
                file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            })
            .expect("pin modification time after dispatch");
        engine
            .answer(
                &references,
                serde_json::json!([{
                    "uri": TsgoTypeProvider::path_to_uri(&target),
                    "range": beta_range(),
                }]),
            )
            .await;
    };
    let (locations, ()) = tokio::join!(
        provider.get_references(&ProviderQuery::at_engine_surface(&origin), 29),
        engine_side
    );
    let error = locations.expect_err("the target's bytes are not the evaluated ones");
    assert!(error.query_conflict, "typed conflict, got {error}");
}
