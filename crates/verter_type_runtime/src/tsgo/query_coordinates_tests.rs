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

/// A relay injection of `content` that the relay confirms applied.
async fn inject(provider: &TsgoTypeProvider, file: &str, content: &str) {
    provider.begin_injection(file);
    provider
        .finish_injection(file, InjectionOutcome::Applied(Arc::from(content)))
        .await;
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
    // A non-owning attach's relay injects carrier bytes out of band: no frame
    // on this transport orders them.
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    inject(&provider, &file, DISPATCHED).await;

    let offset = beta_offset(DISPATCHED);
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        inject(&provider, &file, REPLACED).await;
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
    control: mpsc::UnboundedSender<StdinMessage>,
    interactive: mpsc::Sender<StdinMessage>,
    normal: mpsc::Sender<StdinMessage>,
    background: mpsc::Sender<StdinMessage>,
    ledger: Arc<DeliveryLedger>,
    engine_stdin: tokio::io::DuplexStream,
}

impl Writer {
    fn spawn() -> Self {
        Self::spawn_with_pipe(64 * 1024)
    }

    /// A writer whose pipe to the engine holds at most `pipe` unread bytes, so
    /// a larger flush blocks until the engine side reads.
    fn spawn_with_pipe(pipe: usize) -> Self {
        let (stdin, engine_stdin) = tokio::io::duplex(pipe);
        let ledger = Arc::new(DeliveryLedger::default());
        let (control, control_rx) = mpsc::unbounded_channel();
        let (interactive, interactive_rx) = mpsc::channel(16);
        let (normal, normal_rx) = mpsc::channel(16);
        let (background, background_rx) = mpsc::channel(16);
        tokio::spawn(stdin_writer_loop(
            stdin,
            control_rx,
            interactive_rx,
            normal_rx,
            background_rx,
            None,
            Arc::new(AtomicBool::new(false)),
            std::time::Duration::from_secs(WRITER_STALL_TIMEOUT_SECS),
            Arc::clone(&ledger),
        ));
        Self {
            control,
            interactive,
            normal,
            background,
            ledger,
            engine_stdin,
        }
    }

    /// Enqueue a delivery of `content` for `key` on the interactive lane;
    /// the receiver resolves once it is flushed.
    async fn send_delivery(&self, key: &str, content: &str) -> oneshot::Receiver<()> {
        let versions = Arc::new(Mutex::new(HashMap::new()));
        let (done, delivered) = oneshot::channel();
        self.interactive
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
        delivered
    }

    /// Place a delivery of `content` for `key` and wait for it to flush.
    async fn deliver(&self, key: &str, content: &str) {
        self.send_delivery(key, content)
            .await
            .await
            .expect("the delivery is flushed");
    }

    /// Prepare `query` on `key` now and enqueue it on `lane`; the frame is the
    /// bytes it converted against, and the receiver resolves when the writer
    /// reaches it.
    async fn send_query(
        &self,
        lane: &mpsc::Sender<StdinMessage>,
        query: &ProviderQuery,
        key: &str,
    ) -> oneshot::Receiver<QueryPlaced> {
        let prepared = self.ledger.prepare(query, key).expect("prepare");
        let (placed, bound) = oneshot::channel();
        lane.send(StdinMessage::Query(Box::new(QueryAnchor {
            prepared,
            frame: Box::new(|requested| Ok(format!("QUERY[{requested}];").into_bytes())),
            placed,
        })))
        .await
        .unwrap();
        bound
    }

    /// Enqueue `query` on `key`; the frame is the bytes it converted against.
    async fn query(&self, query: &ProviderQuery, key: &str) -> QueryPlaced {
        self.send_query(&self.interactive, query, key)
            .await
            .await
            .expect("the writer answers the anchor")
    }

    async fn written(self) -> String {
        let Self {
            control,
            interactive,
            normal,
            background,
            mut engine_stdin,
            ..
        } = self;
        drop((control, interactive, normal, background));
        let mut written = Vec::new();
        engine_stdin.read_to_end(&mut written).await.unwrap();
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
        .expect("never a conflict on a wire-ordered route");
    assert_eq!(&**bound.requested(), DISPATCHED);
    writer.deliver(&key, REPLACED).await;
    let bound = writer
        .query(&ProviderQuery::at_engine_surface(&key), &key)
        .await
        .expect("placed")
        .expect("bound");
    assert_eq!(&**bound.requested(), REPLACED);
    assert_eq!(
        writer.written().await,
        format!("DELIVERY;DELIVERY;QUERY[{DISPATCHED}];DELIVERY;QUERY[{REPLACED}];")
    );
}

#[tokio::test]
async fn a_higher_priority_delivery_overtaking_a_prepared_query_is_what_its_frame_converts_against()
{
    const BLOCKER_LEN: usize = 256;
    // The pipe holds less than the blocker, so the writer stays inside that
    // flush until the engine side reads.
    let writer = Writer::spawn_with_pipe(BLOCKER_LEN / 2);
    let key = contents_key(&path("a.ts"));
    writer.deliver(&key, DISPATCHED).await;
    writer
        .control
        .send(StdinMessage::Frame(vec![b'#'; BLOCKER_LEN]))
        .unwrap();

    // Both queries are prepared while the engine holds A and enqueued on the
    // lowest lane; B is enqueued on the interactive lane after them, so the
    // writer places B first.
    let intending_a = ProviderQuery::intending(
        &key,
        DeliveredSurfaceId {
            generation: 1,
            content_epoch: 1,
            incarnation: 1,
        },
        Arc::from(DISPATCHED),
    );
    let at_engine = writer
        .send_query(
            &writer.background,
            &ProviderQuery::at_engine_surface(&key),
            &key,
        )
        .await;
    let intended = writer
        .send_query(&writer.background, &intending_a, &key)
        .await;
    let replaced = writer.send_delivery(&key, REPLACED).await;

    let Writer {
        control,
        interactive,
        normal,
        background,
        mut engine_stdin,
        ..
    } = writer;
    let engine = tokio::spawn(async move {
        let mut written = Vec::new();
        engine_stdin.read_to_end(&mut written).await.unwrap();
        String::from_utf8(written).unwrap()
    });
    replaced.await.expect("B is flushed");
    let at_engine = at_engine
        .await
        .expect("reached")
        .expect("placed")
        .expect("an at-engine query binds what its frame meets");
    assert_eq!(
        &**at_engine.requested(),
        REPLACED,
        "the frame converts against B, which the writer placed before it"
    );
    assert_eq!(
        intended
            .await
            .expect("reached")
            .expect("placed")
            .unwrap_err()
            .kind(),
        ConflictKind::IntendedSurface,
        "a position computed against A never reaches an engine holding B"
    );
    drop((control, interactive, normal, background));
    let written = engine.await.expect("engine side");
    assert!(
        written.ends_with(&format!("DELIVERY;QUERY[{REPLACED}];")),
        "B precedes the only query frame placed: {written}"
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
        .expect("A binds");
    assert_eq!(
        writer.written().await,
        format!("DELIVERY;DELIVERY;DELIVERY;QUERY[{DISPATCHED}];")
    );
}

#[tokio::test]
async fn signature_help_over_republished_out_of_band_bytes_is_a_typed_conflict() {
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    inject(&provider, &file, DISPATCHED).await;

    let engine_side = async {
        let request = engine.expect("textDocument/signatureHelp").await;
        inject(&provider, &file, REPLACED).await;
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

/// Pin `path`'s modification time a minute in the past.
fn age(path: &std::path::Path) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| {
            file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        })
        .expect("age the fixture");
}

/// References from an opened origin into a disk file the engine was never
/// handed, answered in the coordinates of [`DISPATCHED`] while the file's
/// disk bytes move: replaced after evaluation with its timestamp restored, or
/// edited before dispatch without the engine having re-read it.
async fn references_into_an_undelivered_disk_target(
    edited_before_dispatch: bool,
) -> Result<Vec<TypeLocation>, TypeProviderError> {
    let dir = tempfile::tempdir().expect("tempdir");
    let target = dir.path().join("b.ts");
    std::fs::write(&target, DISPATCHED).expect("write target");
    age(&target);
    if edited_before_dispatch {
        std::fs::write(&target, REPLACED).expect("edit target");
        age(&target);
    }
    let (provider, mut engine) = provider();
    let origin = path("a.ts");
    provider
        .update_file(&origin, "import { beta } from './b';\nbeta;\n")
        .await
        .expect("open origin");
    engine.expect("textDocument/didOpen").await;

    let engine_side = async {
        let references = engine.expect("textDocument/references").await;
        if !edited_before_dispatch {
            let modified = std::fs::metadata(&target)
                .and_then(|metadata| metadata.modified())
                .expect("modification time");
            std::fs::write(&target, REPLACED).expect("replace target");
            std::fs::File::options()
                .write(true)
                .open(&target)
                .and_then(|file| file.set_modified(modified))
                .expect("restore modification time");
        }
        engine
            .answer(
                &references,
                serde_json::json!([{
                    "uri": TsgoTypeProvider::path_to_uri(&target.to_string_lossy()),
                    "range": beta_range(),
                }]),
            )
            .await;
    };
    let (locations, ()) = tokio::join!(
        provider.get_references(&ProviderQuery::at_engine_surface(&origin), 29),
        engine_side
    );
    locations
}

#[tokio::test]
async fn an_undelivered_disk_target_is_a_typed_conflict_whatever_its_timestamps_say() {
    for edited_before_dispatch in [false, true] {
        let error = references_into_an_undelivered_disk_target(edited_before_dispatch)
            .await
            .expect_err("no disk read identifies the bytes the engine evaluated");
        assert!(
            error.query_conflict,
            "edited before dispatch: {edited_before_dispatch}; typed conflict, got {error}"
        );
    }
}

#[tokio::test]
async fn a_null_answer_over_republished_out_of_band_bytes_is_a_typed_conflict() {
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    inject(&provider, &file, DISPATCHED).await;

    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        inject(&provider, &file, REPLACED).await;
        engine.answer(&hover, serde_json::Value::Null).await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(&file), 3),
        engine_side
    );
    let error = hover.expect_err("\"nothing here\" over moved bytes is not an answer");
    assert!(error.query_conflict, "typed conflict, got {error}");

    // Unchanged bytes: an empty answer is a successful absence.
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        engine.answer(&hover, serde_json::Value::Null).await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(&ProviderQuery::at_engine_surface(&file), 3),
        engine_side
    );
    assert!(hover.expect("an unchanged surface answers").is_none());
}

#[tokio::test]
async fn a_relay_injection_in_flight_binds_nothing_and_unsettles_answers_bound_before_it() {
    let (provider, mut engine) = provider();
    let file = path("Comp.vue.tsx");
    inject(&provider, &file, DISPATCHED).await;

    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        // The relay starts injecting B through its own channel; the engine may
        // apply it before evaluating the hover.
        provider.begin_injection(&file);
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
        provider.get_hover(
            &ProviderQuery::at_engine_surface(&file),
            beta_offset(DISPATCHED)
        ),
        engine_side
    );
    assert!(
        hover
            .expect_err("bound before the injection")
            .query_conflict
    );

    let error = provider
        .get_hover(
            &ProviderQuery::at_engine_surface(&file),
            beta_offset(DISPATCHED),
        )
        .await
        .expect_err("which bytes the engine holds is unknown");
    assert!(error.query_conflict, "typed conflict, got {error}");

    // The relay confirms B: the file binds again, against B.
    provider
        .finish_injection(&file, InjectionOutcome::Applied(Arc::from(REPLACED)))
        .await;
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        assert_eq!(hover["params"]["position"]["line"], 2);
        engine.answer(&hover, serde_json::Value::Null).await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(&file),
            beta_offset(REPLACED)
        ),
        engine_side
    );
    assert!(hover.expect("bound to the confirmed bytes").is_none());
}

#[tokio::test]
async fn a_cache_only_load_binds_nothing_until_the_file_is_delivered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("loaded.ts");
    std::fs::write(&file, DISPATCHED).expect("write");
    age(&file);
    let file = file.to_string_lossy().to_string();
    let (provider, mut engine) = provider();
    // The local cache holds A and the disk agrees, but the engine reads the
    // file itself: neither identifies the bytes it would evaluate.
    provider.load_file(&file, DISPATCHED).await.expect("load");
    let error = provider
        .get_hover(
            &ProviderQuery::at_engine_surface(&file),
            beta_offset(DISPATCHED),
        )
        .await
        .expect_err("a cache-only load is not a delivery");
    assert!(error.query_conflict, "typed conflict, got {error}");

    // Delivered, it binds: the delivery is the first frame the engine sees.
    provider
        .update_file(&file, DISPATCHED)
        .await
        .expect("deliver");
    engine.expect("textDocument/didOpen").await;
    let engine_side = async {
        let hover = engine.expect("textDocument/hover").await;
        engine.answer(&hover, serde_json::Value::Null).await;
    };
    let (hover, ()) = tokio::join!(
        provider.get_hover(
            &ProviderQuery::at_engine_surface(&file),
            beta_offset(DISPATCHED)
        ),
        engine_side
    );
    assert!(hover.expect("a delivered file binds").is_none());
}
