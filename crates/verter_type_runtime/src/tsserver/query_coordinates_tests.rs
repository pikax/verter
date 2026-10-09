//! A tsserver answer names lines and columns in the bytes the engine held
//! when it dequeued the request. These tests play the engine on the
//! transport's channels and replace the provider's content between dispatch
//! and answer: every range must decode from the dispatched bytes, or the query
//! must end in the typed conflict — never a mix of the two documents.

use super::*;
use crate::provider_query::{
    Attestation, DeliveredSurfaceId, PublicationPosition, SurfacePublications,
};

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
    provider_publishing(None)
}

/// A provider whose plugin reads carrier bytes from `publications`.
fn provider_publishing(
    publications: Option<Arc<dyn SurfacePublications>>,
) -> (TsserverTypeProvider, Engine) {
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
        ledger: DeliveryLedger::new(publications),
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
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(file), offset),
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
    let (locations, ()) = tokio::join!(
        provider.get_definition(&ProviderQuery::at_engine_surface(origin), 29),
        engine_side
    );
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
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(companion), offset),
        engine_side
    );
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
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(companion),
            replaced_offset
        ),
        engine_side
    );
    let hover = hover.expect("hover").expect("an answered hover");
    assert_eq!(hover.range_start, Some(replaced_offset));
}

#[tokio::test]
async fn a_query_on_a_file_neither_delivered_nor_on_disk_sends_nothing() {
    let (provider, mut engine) = provider();
    let missing = verter_test_support::unique_temp_dir("tsserver-query-missing").join("gone.ts");
    let missing = missing.to_string_lossy().to_string();
    let hover = provider
        .get_hover(&ProviderQuery::at_engine_surface(&missing), 3)
        .await
        .expect("hover");
    assert!(hover.is_none());
    // A frame is enqueued synchronously before `get_hover` returns, so an
    // empty channel now is the observed state.
    assert!(
        matches!(
            engine.stdin_rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ),
        "no fabricated position reaches the engine"
    );
}

fn quickinfo_body(line: u32) -> serde_json::Value {
    serde_json::json!({
        "displayString": "const beta: 2",
        "kind": "const",
        "documentation": "",
        "start": { "line": line, "offset": 7 },
        "end": { "line": line, "offset": 11 },
    })
}

const ID: DeliveredSurfaceId = DeliveredSurfaceId {
    generation: 1,
    content_epoch: 1,
    incarnation: 1,
};

#[tokio::test]
async fn a_query_intending_a_surface_the_engine_no_longer_holds_sends_nothing() {
    let (provider, mut engine) = provider();
    let file = "/ws/src/a.ts";
    let (opened, ()) = tokio::join!(
        provider.open_file(file, DISPATCHED),
        engine.acknowledge("updateOpen")
    );
    opened.expect("open");
    // The requester captured A…
    let intending = ProviderQuery::intending(
        TsserverTypeProvider::normalize_path(file),
        ID,
        Arc::from(DISPATCHED),
    );
    // …and B reached the engine before the query was dispatched.
    let (updated, ()) = tokio::join!(
        provider.update_file(file, REPLACED),
        engine.acknowledge("updateOpen")
    );
    updated.expect("update");
    let error = provider
        .get_hover(&intending, beta_offset(DISPATCHED))
        .await
        .expect_err("A's offset is never sent against B");
    assert!(error.query_conflict, "typed conflict, got {error}");
    assert!(matches!(
        engine.stdin_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    // Restored to A before dispatch: exactly the intended surface.
    let (restored, ()) = tokio::join!(
        provider.update_file(file, DISPATCHED),
        engine.acknowledge("updateOpen")
    );
    restored.expect("restore");
    let engine_side = async {
        let quickinfo = engine.next().await;
        engine.answer(&quickinfo, quickinfo_body(2));
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(&intending, beta_offset(DISPATCHED)),
        engine_side
    );
    let hover = hover.expect("hover").expect("answered");
    assert_eq!(hover.range_start, Some(beta_offset(DISPATCHED)));
}

#[tokio::test]
async fn signature_help_and_highlights_settle_out_of_band_bytes_before_answering() {
    let (provider, mut engine) = provider();
    let source = "/ws/src/App.vue";
    let companion = "/ws/src/App.vue.tsx";
    let project = "/ws/tsconfig.json";
    provider
        .register_carrier_metadata(source, companion, DISPATCHED, project)
        .await
        .expect("register");
    let query = ProviderQuery::at_engine_surface(companion);

    let engine_side = async {
        let request = engine.next().await;
        assert_eq!(request["command"], "signatureHelp");
        provider
            .register_carrier_metadata(source, companion, REPLACED, project)
            .await
            .expect("republish");
        engine.answer(
            &request,
            serde_json::json!({
                "items": [{
                    "prefixDisplayParts": [{ "text": "beta(" }],
                    "suffixDisplayParts": [{ "text": ")" }],
                    "separatorDisplayParts": [{ "text": ", " }],
                    "parameters": [],
                }],
                "selectedItemIndex": 0,
                "argumentIndex": 0,
            }),
        );
    };
    let (help, ()) = tokio::join!(provider.get_signature_help(&query, 3), engine_side);
    let error = help.expect_err("help evaluated over moved bytes must not succeed");
    assert!(error.query_conflict, "typed conflict, got {error}");

    let engine_side = async {
        let request = engine.next().await;
        assert_eq!(request["command"], "documentHighlights");
        provider
            .register_carrier_metadata(source, companion, DISPATCHED, project)
            .await
            .expect("republish");
        engine.answer(
            &request,
            serde_json::json!([{
                "file": TsserverTypeProvider::normalize_path(source),
                "highlightSpans": [{
                    "start": { "line": 3, "offset": 7 },
                    "end": { "line": 3, "offset": 11 },
                    "kind": "reference",
                }],
            }]),
        );
    };
    let (highlights, ()) = tokio::join!(
        provider.get_document_highlights(&query, beta_offset(REPLACED)),
        engine_side
    );
    let error = highlights.expect_err("highlights over moved bytes must not succeed");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

#[tokio::test]
async fn highlights_decode_byte_ranges_from_the_dispatched_bytes() {
    let (provider, mut engine) = provider();
    let file = "/ws/src/a.ts";
    let (opened, ()) = tokio::join!(
        provider.open_file(file, DISPATCHED),
        engine.acknowledge("updateOpen")
    );
    opened.expect("open");
    let engine_side = async {
        let request = engine.next().await;
        assert_eq!(request["command"], "documentHighlights");
        let (updated, ()) = tokio::join!(
            provider.update_file(file, REPLACED),
            engine.acknowledge("updateOpen")
        );
        updated.expect("update");
        engine.answer(
            &request,
            serde_json::json!([{
                "file": TsserverTypeProvider::normalize_path(file),
                "highlightSpans": [{
                    "start": { "line": 2, "offset": 7 },
                    "end": { "line": 2, "offset": 11 },
                    "kind": "writtenReference",
                }],
            }]),
        );
    };
    let offset = beta_offset(DISPATCHED);
    let (highlights, ()) = tokio::join!(
        provider.get_document_highlights(&ProviderQuery::at_engine_surface(file), offset),
        engine_side
    );
    let highlights = highlights.expect("highlights");
    assert_eq!(highlights.len(), 1);
    assert_eq!(
        (highlights[0].start, highlights[0].end),
        (offset, offset + 4),
        "a second-line span is a byte range in the dispatched bytes, never packed"
    );
    assert!(matches!(
        highlights[0].kind,
        TypeDocumentHighlightKind::Write
    ));
}

#[tokio::test]
async fn a_disk_target_rewritten_after_dispatch_is_a_typed_conflict() {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("b.ts");
    std::fs::write(&target, DISPATCHED).expect("write target");
    let target = TsserverTypeProvider::normalize_path(&target.to_string_lossy());
    let (provider, mut engine) = provider();
    let origin = "/ws/src/a.ts";
    let (opened, ()) = tokio::join!(
        provider.open_file(origin, "import { beta } from './b';\nbeta;\n"),
        engine.acknowledge("updateOpen")
    );
    opened.expect("open");

    let engine_side = async {
        let definition = engine.next().await;
        assert_eq!(definition["command"], "definition");
        std::fs::write(&target, REPLACED).expect("rewrite target");
        std::fs::File::options()
            .write(true)
            .open(&target)
            .and_then(|file| {
                file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            })
            .expect("pin modification time after dispatch");
        engine.answer(
            &definition,
            serde_json::json!([{
                "file": target,
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 2, "offset": 11 },
            }]),
        );
    };
    let (locations, ()) = tokio::join!(
        provider.get_definition(&ProviderQuery::at_engine_surface(origin), 29),
        engine_side
    );
    let error = locations.expect_err("the target's bytes are not the evaluated ones");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

/// Published rows: path → (publication epoch, bytes).
type PublishedRows = HashMap<String, (u64, Arc<str>)>;

/// The carrier store as the plugin reads it, moved by any writer.
#[derive(Default)]
struct Store {
    rows: parking_lot::Mutex<(u64, PublishedRows)>,
}

impl Store {
    /// Publish `content` under both identities the plugin serves it by.
    fn publish(&self, source: &str, companion: &str, content: &str) {
        let mut rows = self.rows.lock();
        rows.0 += 1;
        let epoch = rows.0;
        for path in [source, companion] {
            rows.1.insert(
                TsserverTypeProvider::normalize_path(path),
                (epoch, Arc::from(content)),
            );
        }
    }
}

impl SurfacePublications for Store {
    fn position(&self) -> Option<PublicationPosition> {
        Some(PublicationPosition {
            instance: Arc::from("store"),
            epoch: self.rows.lock().0,
        })
    }

    fn attest(&self, path: &str, bytes: &str) -> Attestation {
        match self.rows.lock().1.get(path) {
            None => Attestation::Unpublished,
            Some((_, published)) if **published != *bytes => Attestation::Contradicted,
            Some((epoch, _)) => Attestation::Attested(PublicationPosition {
                instance: Arc::from("store"),
                epoch: *epoch,
            }),
        }
    }
}

const SOURCE: &str = "/ws/src/App.vue";
const COMPANION: &str = "/ws/src/App.vue.tsx";
const PROJECT: &str = "/ws/tsconfig.json";

async fn published_carrier() -> (TsserverTypeProvider, Engine, Arc<Store>) {
    let store = Arc::new(Store::default());
    let (provider, engine) =
        provider_publishing(Some(Arc::clone(&store) as Arc<dyn SurfacePublications>));
    store.publish(SOURCE, COMPANION, DISPATCHED);
    provider
        .register_carrier_metadata(SOURCE, COMPANION, DISPATCHED, PROJECT)
        .await
        .expect("register");
    (provider, engine, store)
}

#[tokio::test]
async fn another_writers_publication_under_an_answer_is_a_typed_conflict() {
    let (provider, mut engine, store) = published_carrier().await;
    let engine_side = async {
        let quickinfo = engine.next().await;
        // Another LSP publishes B into the shared store; this process never
        // re-registers the carrier.
        store.publish(SOURCE, COMPANION, REPLACED);
        engine.answer(&quickinfo, quickinfo_body(2));
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(COMPANION),
            beta_offset(DISPATCHED)
        ),
        engine_side
    );
    let error = hover.expect_err("the plugin may have served B");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

#[tokio::test]
async fn a_publication_that_changes_and_changes_back_under_an_answer_is_a_typed_conflict() {
    let (provider, mut engine, store) = published_carrier().await;
    let engine_side = async {
        let quickinfo = engine.next().await;
        store.publish(SOURCE, COMPANION, REPLACED);
        store.publish(SOURCE, COMPANION, DISPATCHED);
        engine.answer(&quickinfo, quickinfo_body(2));
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(COMPANION),
            beta_offset(DISPATCHED)
        ),
        engine_side
    );
    let error = hover.expect_err("the engine may have evaluated the intermediate publication");
    assert!(error.query_conflict, "typed conflict, got {error}");
}

#[tokio::test]
async fn a_publication_ahead_of_registration_refuses_the_query_before_dispatch() {
    let (provider, mut engine, store) = published_carrier().await;
    // The store already serves B; this process has not registered it.
    store.publish(SOURCE, COMPANION, REPLACED);
    let error = provider
        .get_hover(
            &ProviderQuery::at_engine_surface(COMPANION),
            beta_offset(DISPATCHED),
        )
        .await
        .expect_err("the request would convert against bytes the plugin no longer serves");
    assert!(error.query_conflict, "typed conflict, got {error}");
    assert!(matches!(
        engine.stdin_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    // Registration catches up: the query binds again.
    provider
        .register_carrier_metadata(SOURCE, COMPANION, REPLACED, PROJECT)
        .await
        .expect("register");
    let engine_side = async {
        let quickinfo = engine.next().await;
        engine.answer(&quickinfo, quickinfo_body(3));
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(COMPANION),
            beta_offset(REPLACED)
        ),
        engine_side
    );
    assert_eq!(
        hover.expect("hover").expect("answered").range_start,
        Some(beta_offset(REPLACED))
    );
}

#[tokio::test]
async fn a_carrier_target_republished_under_a_definition_is_a_typed_conflict() {
    let (provider, mut engine, store) = published_carrier().await;
    let origin = "/ws/src/main.ts";
    let (opened, ()) = tokio::join!(
        provider.open_file(origin, "import App from './App.vue';\nApp;\n"),
        engine.acknowledge("updateOpen")
    );
    opened.expect("open");
    let engine_side = async {
        let definition = engine.next().await;
        assert_eq!(definition["command"], "definition");
        store.publish(SOURCE, COMPANION, REPLACED);
        engine.answer(
            &definition,
            serde_json::json!([{
                "file": TsserverTypeProvider::normalize_path(SOURCE),
                "start": { "line": 2, "offset": 7 },
                "end": { "line": 2, "offset": 11 },
            }]),
        );
    };
    let (locations, ()) = tokio::join!(
        provider.get_definition(&ProviderQuery::at_engine_surface(origin), 4),
        engine_side
    );
    let error = locations.expect_err("the carrier target's publication moved");
    assert!(error.query_conflict, "typed conflict, got {error}");
}
