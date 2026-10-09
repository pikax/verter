//! A tsgo answer names lines and characters in the bytes the engine held when
//! it dequeued the request. These tests play the engine over an in-memory
//! duplex and replace the provider's content between dispatch and answer:
//! every range must decode from the dispatched bytes, or the query must end in
//! the typed conflict — never a mix of the two documents.

use super::*;
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
    let (hover, ()) = tokio::join!(provider.get_hover(&file, offset), engine_side);
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
    let (locations, ()) = tokio::join!(provider.get_references(&origin, 29), engine_side);
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
    let (hover, ()) = tokio::join!(provider.get_hover(&file, offset), engine_side);
    let error = hover.expect_err("moved out-of-band bytes must not decode");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

#[tokio::test]
async fn the_writer_refuses_a_query_whose_requested_file_moved_before_its_frame() {
    let (stdin, mut engine_stdin) = tokio::io::duplex(64 * 1024);
    let (stdin_tx, stdin_rx) = mpsc::channel::<StdinMessage>(16);
    let ledger = Arc::new(DeliveryLedger::default());
    let (_control_tx, control_rx) = mpsc::unbounded_channel();
    let (_normal_tx, normal_rx) = mpsc::channel(1);
    let (_background_tx, background_rx) = mpsc::channel(1);
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

    let key = contents_key(&path("a.ts"));
    // The query converts against what the engine holds now: nothing.
    let converted = ledger.requested(&key, Some(Arc::from(DISPATCHED)));

    // A delivery of the same document is placed first.
    let versions = Arc::new(Mutex::new(HashMap::new()));
    let (done, delivered) = oneshot::channel();
    stdin_tx
        .send(StdinMessage::Document(
            b"Content-Length: 2\r\n\r\n{}".to_vec(),
            Box::new(DocumentDelivery {
                versions: versions.lock_owned().await,
                contents: Arc::new(Mutex::new(HashMap::new())),
                accepted: Arc::new(StdMutex::new(HashMap::new())),
                key: key.clone(),
                value: Some((1, Arc::from(REPLACED))),
                done,
                diagnostics: None,
            }),
        ))
        .await
        .unwrap();
    let (placed, bound) = oneshot::channel();
    stdin_tx
        .send(StdinMessage::Query(
            b"QUERY-FRAME".to_vec(),
            Box::new(QueryAnchor {
                path: key.clone(),
                requested: converted,
                placed,
            }),
        ))
        .await
        .unwrap();
    delivered.await.expect("the delivery is flushed");
    let refused = bound.await.expect("the writer answers the anchor");
    assert_eq!(refused.unwrap_err().path(), key);

    // Only the delivery reached the engine; the stale query frame did not.
    drop(stdin_tx);
    let mut written = Vec::new();
    engine_stdin.read_to_end(&mut written).await.unwrap();
    assert_eq!(written, b"Content-Length: 2\r\n\r\n{}");
}
