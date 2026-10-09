//! A tsserver answer names lines and columns in the bytes the engine held
//! when it dequeued the request. These tests play the engine on the
//! transport's channels and replace the provider's content between dispatch
//! and answer: every range must decode from the dispatched bytes, or the query
//! must end in the typed conflict — never a mix of the two documents.

use super::*;

/// The engine side of an in-memory tsserver transport.
struct Engine {
    stdin_rx: mpsc::Receiver<TsserverStdinMessage>,
    pending: Arc<TsserverPendingRequests>,
}

impl Engine {
    /// The next frame the provider wrote.
    async fn next(&mut self) -> serde_json::Value {
        let message = tokio::time::timeout(std::time::Duration::from_secs(5), self.stdin_rx.recv())
            .await
            .expect("the provider must write its next frame promptly")
            .expect("stdin must stay open");
        let TsserverStdinMessage::Frame(frame) = message else {
            panic!("unexpected stdin shutdown");
        };
        serde_json::from_slice(&frame).expect("every frame is one JSON request")
    }

    /// Answer `request` successfully with `body`.
    fn answer(&self, request: &serde_json::Value, body: serde_json::Value) {
        let seq = request["seq"].as_i64().expect("request seq");
        let tx = self
            .pending
            .table
            .take(seq)
            .expect("the answered request is still pending");
        let _ = tx.send(serde_json::json!({
            "type": "response",
            "request_seq": seq,
            "success": true,
            "command": request["command"],
            "body": body,
        }));
    }

    /// Acknowledge the next frame, which must be a `command` write.
    async fn acknowledge(&mut self, command: &str) {
        let frame = self.next().await;
        assert_eq!(frame["command"], command, "unexpected frame {frame}");
        self.answer(&frame, serde_json::json!({}));
    }
}

fn provider() -> (TsserverTypeProvider, Engine) {
    let (stdin_tx, stdin_rx) = mpsc::channel(64);
    let pending = Arc::new(TsserverPendingRequests::default());
    let transport = Arc::new(TsserverTransport {
        stdin_tx,
        pending: Arc::clone(&pending),
        next_seq: AtomicI64::new(1),
        liveness: Arc::new(EngineLiveness::default()),
        crash_notify: None,
        membership_recovery: Mutex::new(None),
        cancellation: TsserverCancellation::create().map(Arc::new),
        ledger: Default::default(),
    });
    let provider =
        TsserverTypeProvider::over_transport(transport, None, None, "/ws".to_string(), false);
    (provider, Engine { stdin_rx, pending })
}

const DISPATCHED: &str = "const alpha = 1;\nconst beta = 2;\n";
/// Shifts every line of [`DISPATCHED`] down by one, so a line/column decoded
/// against these bytes lands on a different offset.
const REPLACED: &str = "// a comment that moves every line\nconst alpha = 1;\nconst beta = 2;\n";

fn beta_offset(content: &str) -> u32 {
    content.find("beta").expect("fixture names beta") as u32
}

#[tokio::test]
async fn hover_range_decodes_from_the_dispatched_bytes_across_a_content_replacement() {
    let (provider, mut engine) = provider();
    let file = "/ws/src/a.ts";
    let (opened, ()) = tokio::join!(
        provider.open_file(file, DISPATCHED),
        engine.acknowledge("updateOpen")
    );
    opened.expect("open");

    let offset = beta_offset(DISPATCHED);
    let engine_side = async {
        let quickinfo = engine.next().await;
        assert_eq!(quickinfo["command"], "quickinfo");
        assert_eq!(quickinfo["arguments"]["line"], 2);
        assert_eq!(quickinfo["arguments"]["offset"], 7);
        // The barrier: the provider's content is replaced after dispatch and
        // before the engine's answer is decoded.
        let (updated, ()) = tokio::join!(
            provider.update_file(file, REPLACED),
            engine.acknowledge("updateOpen")
        );
        updated.expect("update");
        engine.answer(
            &quickinfo,
            serde_json::json!({
                "displayString": "const beta: 2",
                "kind": "const",
                "documentation": "",
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 2, "offset": 11 },
            }),
        );
    };
    let (hover, ()) = tokio::join!(provider.get_hover(file, offset), engine_side);
    let hover = hover.expect("hover").expect("an answered hover");
    assert_eq!(
        (hover.range_start, hover.range_end),
        (Some(offset), Some(offset + 4)),
        "the range names `beta` in the bytes the query was dispatched against, \
         not the line the replacement moved under it"
    );
}

#[tokio::test]
async fn definition_into_another_file_decodes_from_that_files_dispatched_bytes() {
    let (provider, mut engine) = provider();
    let origin = "/ws/src/a.ts";
    let target = "/ws/src/b.ts";
    for (file, content) in [
        (origin, "import { beta } from './b';\nbeta;\n"),
        (target, DISPATCHED),
    ] {
        let (opened, ()) = tokio::join!(
            provider.open_file(file, content),
            engine.acknowledge("updateOpen")
        );
        opened.expect("open");
    }

    let engine_side = async {
        let definition = engine.next().await;
        assert_eq!(definition["command"], "definition");
        let (updated, ()) = tokio::join!(
            provider.update_file(target, REPLACED),
            engine.acknowledge("updateOpen")
        );
        updated.expect("update");
        engine.answer(
            &definition,
            serde_json::json!([{
                "file": TsserverTypeProvider::normalize_path(target),
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 2, "offset": 11 },
            }]),
        );
    };
    let (locations, ()) = tokio::join!(provider.get_definition(origin, 29), engine_side);
    let locations = locations.expect("definition");
    let start = beta_offset(DISPATCHED);
    assert_eq!(locations.len(), 1);
    assert_eq!(
        (locations[0].start, locations[0].end),
        (start, start + 4),
        "a foreign target decodes through its own bytes as the request met them"
    );
}

#[tokio::test]
async fn a_carrier_republished_under_an_answer_is_a_typed_conflict() {
    let (provider, mut engine) = provider();
    let source = "/ws/src/App.vue";
    let companion = "/ws/src/App.vue.tsx";
    let project = "/ws/tsconfig.json";
    provider
        .register_carrier_metadata(source, companion, DISPATCHED, project)
        .await
        .expect("register");

    let offset = beta_offset(DISPATCHED);
    let engine_side = async {
        let quickinfo = engine.next().await;
        assert_eq!(quickinfo["command"], "quickinfo");
        assert_eq!(
            quickinfo["arguments"]["file"],
            TsserverTypeProvider::normalize_path(source),
            "a managed carrier is queried under its authored source identity"
        );
        // The plugin reads the store while it evaluates, so a republication
        // between dispatch and decode leaves no way to tell which bytes the
        // engine evaluated.
        provider
            .register_carrier_metadata(source, companion, REPLACED, project)
            .await
            .expect("republish");
        engine.answer(
            &quickinfo,
            serde_json::json!({
                "displayString": "const beta: 2",
                "kind": "const",
                "documentation": "",
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 2, "offset": 11 },
            }),
        );
    };
    let (hover, ()) = tokio::join!(provider.get_hover(companion, offset), engine_side);
    let error = hover.expect_err("a moved out-of-band carrier must not decode");
    assert!(error.query_conflict, "typed conflict, got {error}");

    // An unmoved carrier answers normally.
    let engine_side = async {
        let quickinfo = engine.next().await;
        engine.answer(
            &quickinfo,
            serde_json::json!({
                "displayString": "const beta: 2",
                "kind": "const",
                "documentation": "",
                "start": { "line": 3, "offset": 7 },
                "end": { "line": 3, "offset": 11 },
            }),
        );
    };
    let replaced_offset = beta_offset(REPLACED);
    let (hover, ()) = tokio::join!(provider.get_hover(companion, replaced_offset), engine_side);
    let hover = hover.expect("hover").expect("an answered hover");
    assert_eq!(hover.range_start, Some(replaced_offset));
}

#[tokio::test]
async fn a_query_on_a_file_neither_delivered_nor_on_disk_sends_nothing() {
    let (provider, mut engine) = provider();
    let missing = verter_test_support::unique_temp_dir("tsserver-query-missing").join("gone.ts");
    let missing = missing.to_string_lossy().to_string();
    let hover = provider.get_hover(&missing, 3).await.expect("hover");
    assert!(hover.is_none());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), engine.stdin_rx.recv())
            .await
            .is_err(),
        "no fabricated position reaches the engine"
    );
}
