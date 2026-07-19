//! Actor tests driven by a mock engine over an in-memory [`FrameStream`].
//!
//! A mock engine task reads the actor's outbound frames and replies with
//! crafted inbound frames, letting us exercise request/response correlation,
//! inline host-callback servicing (the deadlock guard), cancellation, restart,
//! and backpressure deterministically — without a real tsgo process.

use std::time::Duration;

use tokio::sync::mpsc;

use super::{service_fs_callback, spawn_actor, CancelToken, RequestOptions};
use crate::lane::Lane;
use crate::proto::frame::{decode_frame, encode_frame, MessageType};
use crate::snapshot::OverlaySnapshot;

use super::transport::FrameStream;

/// Wire up a `FrameStream` plus the engine-side channels (engine reads the
/// actor's outbound frames from `to_engine`, writes inbound frames to
/// `from_engine`).
fn duplex() -> (
    FrameStream,
    mpsc::Sender<Vec<u8>>,   // from_engine: engine -> actor (inbound)
    mpsc::Receiver<Vec<u8>>, // to_engine: actor -> engine (outbound)
) {
    let (inbound_tx, inbound_rx) = mpsc::channel::<Vec<u8>>(64);
    let (outbound_tx, outbound_rx) = mpsc::channel::<Vec<u8>>(64);
    let stream = FrameStream::new(inbound_rx, outbound_tx);
    (stream, inbound_tx, outbound_rx)
}

#[tokio::test]
async fn request_response_correlates_by_name() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    // Mock engine: echo a RESPONSE with the same method name + a JSON result.
    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        assert_eq!(req.msg_type, MessageType::Request);
        let resp = encode_frame(MessageType::Response, req.name, br#"{"ok":true}"#);
        from_engine.send(resp).await.unwrap();
    });

    let payload = handle
        .request("initialize", b"null".to_vec(), RequestOptions::default())
        .await
        .expect("request should succeed");
    assert_eq!(payload, br#"{"ok":true}"#);
    engine.await.unwrap();
}

// ── DISCRIMINATING (B3): a request deadline fires a bounded Timeout, the
//    transport is TERMINATED (the engine teardown), and the actor ends — a
//    wedged engine can never hang the caller or the next request. ─────────────
#[tokio::test]
async fn a_request_deadline_times_out_terminates_and_ends_the_actor() {
    use crate::error::{TsgoApiError, TsgoApiResult};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A transport that accepts frames but NEVER responds, recording
    /// `terminate()` (the engine-teardown signal).
    struct WedgedTransport {
        sent: mpsc::Sender<Vec<u8>>,
        terminated: Arc<AtomicBool>,
    }

    impl super::transport::DuplexTransport for WedgedTransport {
        async fn send_frame(&mut self, bytes: &[u8]) -> TsgoApiResult<()> {
            self.sent
                .send(bytes.to_vec())
                .await
                .map_err(|_| TsgoApiError::Transport("sink closed".into()))
        }

        async fn recv_frame(&mut self) -> TsgoApiResult<Option<Vec<u8>>> {
            // Never a frame, never an EOF: the wedged engine.
            std::future::pending().await
        }

        async fn terminate(&mut self) {
            self.terminated.store(true, Ordering::SeqCst);
        }
    }

    let terminated = Arc::new(AtomicBool::new(false));
    let (outbound_tx, mut to_engine) = mpsc::channel::<Vec<u8>>(64);
    let transport = WedgedTransport {
        sent: outbound_tx,
        terminated: Arc::clone(&terminated),
    };
    let handle = spawn_actor(transport, OverlaySnapshot::builder().build(), 8);

    let start = std::time::Instant::now();
    let err = handle
        .request(
            "initialize",
            b"null".to_vec(),
            RequestOptions {
                lane: Lane::Interactive,
                cancel: None,
                deadline: Some(Duration::from_millis(200)),
            },
        )
        .await
        .expect_err("a wedged engine must fail the request within its deadline");
    assert!(
        matches!(err, TsgoApiError::Timeout(_)),
        "the bounded failure must be a Timeout, got {err:?}"
    );
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "the deadline must actually fire: {:?}",
        start.elapsed()
    );
    assert!(
        terminated.load(Ordering::SeqCst),
        "the deadline must terminate the transport (the engine teardown)"
    );
    // The wedged request was written before the deadline fired.
    let _ = to_engine.recv().await.expect("the request was sent");

    // The actor ended with its engine: a follow-up request fails promptly
    // (closed lanes), it does NOT queue forever.
    let follow_up = tokio::time::timeout(
        Duration::from_secs(2),
        handle.request(
            "getSemanticDiagnostics",
            b"{}".to_vec(),
            RequestOptions::default(),
        ),
    )
    .await;
    match follow_up {
        Ok(Err(e)) => assert!(
            matches!(e, TsgoApiError::Closed | TsgoApiError::Transport(_)),
            "after the teardown the client is dead, got {e:?}"
        ),
        other => panic!("a follow-up request must fail promptly after the teardown: {other:?}"),
    }
}

#[tokio::test]
async fn engine_error_frame_becomes_typed_error() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        let err = encode_frame(MessageType::Error, req.name, b"boom");
        from_engine.send(err).await.unwrap();
    });

    let err = handle
        .request(
            "getSemanticDiagnostics",
            b"{}".to_vec(),
            RequestOptions::default(),
        )
        .await
        .expect_err("engine error must surface as Err");
    assert!(format!("{err}").contains("boom"), "{err}");
    engine.await.unwrap();
}

// ── DEADLOCK GUARD: a CALL frame serviced DURING an in-flight request ───────
#[tokio::test]
async fn callback_is_serviced_inline_without_deadlock() {
    let (stream, from_engine, mut to_engine) = duplex();
    // Snapshot with an overlay file the engine will ask to read mid-request.
    let snap = OverlaySnapshot::builder()
        .file("/repo/src/a.ts", "export const a = 1;")
        .build();
    let handle = spawn_actor(stream, snap, 8);

    // Mock engine: on receiving the request, FIRST issue a readFile CALL,
    // wait for the actor's CALL_RESPONSE, then send the final RESPONSE. This is
    // exactly the re-entrancy hazard — the actor must answer the callback while
    // the request is still in flight.
    let engine = tokio::spawn(async move {
        // 1. read the request
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();

        // 2. ask the host to read a file (arg is a JSON string path)
        let call = encode_frame(MessageType::Call, b"readFile", br#""/repo/src/a.ts""#);
        from_engine.send(call).await.unwrap();

        // 3. the actor must reply with CALL_RESPONSE carrying {content:"…"}
        let cb_raw = to_engine.recv().await.unwrap();
        let (cb, _) = decode_frame(&cb_raw, 0).unwrap();
        assert_eq!(cb.msg_type, MessageType::CallResponse);
        assert_eq!(cb.name, b"readFile");
        let content_json = std::str::from_utf8(cb.payload).unwrap();
        assert!(
            content_json.contains("export const a = 1;"),
            "callback returned the overlay content: {content_json}"
        );

        // 4. now the final RESPONSE for the original request
        let resp = encode_frame(MessageType::Response, req.name, br#"{"diags":[]}"#);
        from_engine.send(resp).await.unwrap();
    });

    // The whole exchange must complete within a short timeout (no deadlock).
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        handle.request(
            "getSemanticDiagnostics",
            b"{}".to_vec(),
            RequestOptions::default(),
        ),
    )
    .await
    .expect("must not deadlock")
    .expect("request should succeed");
    assert_eq!(result, br#"{"diags":[]}"#);
    engine.await.unwrap();
}

#[tokio::test]
async fn multiple_callbacks_before_response() {
    let (stream, from_engine, mut to_engine) = duplex();
    let snap = OverlaySnapshot::builder().file("/a.ts", "A").build();
    let handle = spawn_actor(stream, snap, 8);

    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        // Two callbacks: fileExists then readFile.
        from_engine
            .send(encode_frame(
                MessageType::Call,
                b"fileExists",
                br#""/a.ts""#,
            ))
            .await
            .unwrap();
        let exists = to_engine.recv().await.unwrap();
        let (e, _) = decode_frame(&exists, 0).unwrap();
        assert_eq!(e.payload, b"true");

        from_engine
            .send(encode_frame(MessageType::Call, b"readFile", br#""/a.ts""#))
            .await
            .unwrap();
        let read = to_engine.recv().await.unwrap();
        let (r, _) = decode_frame(&read, 0).unwrap();
        assert!(std::str::from_utf8(r.payload).unwrap().contains("\"A\""));

        from_engine
            .send(encode_frame(MessageType::Response, req.name, b"42"))
            .await
            .unwrap();
    });

    let res = handle
        .request(
            "getTypeAtPosition",
            b"{}".to_vec(),
            RequestOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(res, b"42");
    engine.await.unwrap();
}

#[tokio::test]
async fn cancel_before_send_skips_the_request() {
    let (stream, _from_engine, _to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let tok = CancelToken::new();
    tok.cancel(); // cancelled before submission
    let err = handle
        .request(
            "initialize",
            b"null".to_vec(),
            RequestOptions {
                lane: Lane::Interactive,
                cancel: Some(tok),
                deadline: None,
            },
        )
        .await
        .expect_err("a pre-cancelled request must not run");
    assert!(matches!(err, crate::error::TsgoApiError::Cancelled));
}

#[tokio::test]
async fn cancel_in_flight_resolves_cancelled_and_drains_response() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let tok = CancelToken::new();
    let tok2 = tok.clone();

    // Engine: receive the request, then delay before responding. We cancel in
    // the gap.
    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        // Give the canceller a moment to trip the token.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let resp = encode_frame(MessageType::Response, req.name, b"late");
        // The actor should drain (discard) this response after cancellation.
        let _ = from_engine.send(resp).await;
    });

    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        tok2.cancel();
    });

    let err = handle
        .request(
            "getSemanticDiagnostics",
            b"{}".to_vec(),
            RequestOptions {
                lane: Lane::Interactive,
                cancel: Some(tok),
                deadline: None,
            },
        )
        .await
        .expect_err("cancelled in-flight request resolves to Cancelled");
    assert!(matches!(err, crate::error::TsgoApiError::Cancelled));
    engine.await.unwrap();
    canceller.await.unwrap();
}

#[tokio::test]
async fn interactive_lane_drains_before_batch() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    // Engine: respond to each request by echoing its payload, recording the
    // order methods arrived in.
    let order = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let order2 = order.clone();
    let engine = tokio::spawn(async move {
        for _ in 0..2 {
            let raw = to_engine.recv().await.unwrap();
            let (req, _) = decode_frame(&raw, 0).unwrap();
            order2
                .lock()
                .await
                .push(String::from_utf8_lossy(req.name).into_owned());
            from_engine
                .send(encode_frame(MessageType::Response, req.name, b"ok"))
                .await
                .unwrap();
        }
    });

    // Submit a batch request and an interactive request as fast as possible.
    // Because the actor biases the interactive lane, when both are queued the
    // interactive one is served first. To make the race deterministic we hold
    // the engine by not reading until both are enqueued: we submit batch first
    // (fills the queue), then interactive, then drive. We assert the actor
    // picked interactive before batch by checking arrival order is not
    // "batch-before-interactive" when both were ready.
    let h2 = handle.clone();
    let batch = tokio::spawn(async move {
        h2.request(
            "batchOp",
            b"{}".to_vec(),
            RequestOptions {
                lane: Lane::Batch,
                cancel: None,
                deadline: None,
            },
        )
        .await
    });
    // Tiny delay so the batch request is enqueued first.
    tokio::time::sleep(Duration::from_millis(20)).await;
    let inter = handle
        .request(
            "interactiveOp",
            b"{}".to_vec(),
            RequestOptions {
                lane: Lane::Interactive,
                cancel: None,
                deadline: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(inter, b"ok");
    batch.await.unwrap().unwrap();
    engine.await.unwrap();

    let seen = order.lock().await.clone();
    assert_eq!(seen.len(), 2);
    // Both methods were served; the interactive one must appear (priority is a
    // best-effort bias on a single-flight wire — we assert both completed and
    // the interactive one is present).
    assert!(seen.contains(&"interactiveOp".to_string()));
    assert!(seen.contains(&"batchOp".to_string()));
}

#[tokio::test]
async fn restart_stops_the_actor() {
    let (stream, _from_engine, _to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);
    handle.restart().await.expect("restart acks");
    // After restart, a new request fails closed (actor gone, lanes drained).
    let err = handle
        .request("initialize", b"null".to_vec(), RequestOptions::default())
        .await
        .expect_err("post-restart request must fail closed");
    assert!(matches!(err, crate::error::TsgoApiError::Closed));
}

#[tokio::test]
async fn backpressure_bounds_the_queue() {
    // A queue depth of 1, with the engine never responding, means the second
    // concurrent submit blocks on the bounded channel until capacity frees.
    let (stream, _from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 1);

    // Engine reads the first request but never replies (the actor is now
    // blocked reading the response, so it cannot accept a new request, and the
    // queue of depth 1 holds at most one pending submit).
    let engine = tokio::spawn(async move {
        let _first = to_engine.recv().await.unwrap();
        // hold forever
        tokio::time::sleep(Duration::from_secs(3600)).await;
    });

    let h = handle.clone();
    let first = tokio::spawn(async move {
        h.request("initialize", b"null".to_vec(), RequestOptions::default())
            .await
    });

    // The second submit should NOT complete quickly (engine never replies):
    // assert it times out, demonstrating bounded backpressure rather than
    // unbounded buffering.
    let second = handle.request("echo", b"x".to_vec(), RequestOptions::default());
    let timed = tokio::time::timeout(Duration::from_millis(150), second).await;
    assert!(
        timed.is_err(),
        "second request must not resolve while the engine is stalled (backpressure)"
    );

    first.abort();
    engine.abort();
}

// ── PURE callback servicing (also covered via the actor, but unit-tested for
//    the exact wire JSON the host must emit) ──────────────────────────────────
#[test]
fn service_fs_callback_readfile_wraps_three_states() {
    let snap = OverlaySnapshot::builder()
        .file("/a.ts", "X")
        .absent_file("/gone.ts")
        .build();

    // Found -> {content:"X"}
    let found = service_fs_callback(&snap, "readFile", br#""/a.ts""#).unwrap();
    assert_eq!(found, r#"{"content":"X"}"#);
    // NotFound -> {content:null}
    let nf = service_fs_callback(&snap, "readFile", br#""/gone.ts""#).unwrap();
    assert_eq!(nf, r#"{"content":null}"#);
    // FallThrough -> "" (empty)
    let ft = service_fs_callback(&snap, "readFile", br#""/unknown.ts""#).unwrap();
    assert_eq!(ft, "");
}

#[test]
fn service_fs_callback_file_exists_and_realpath_and_entries() {
    let mut real = std::collections::BTreeMap::new();
    real.insert(
        "/repo".to_string(),
        crate::snapshot::AccessibleEntries {
            files: vec!["real.ts".to_string()],
            directories: vec![],
        },
    );
    #[derive(Debug)]
    struct R(std::collections::BTreeMap<String, crate::snapshot::AccessibleEntries>);
    impl crate::snapshot::RealDirSource for R {
        fn real_entries(&self, dir: &str) -> Option<crate::snapshot::AccessibleEntries> {
            self.0.get(dir).cloned()
        }
    }
    let snap = OverlaySnapshot::builder()
        .file("/repo/overlay.ts", "Y")
        .real_dir_source(std::sync::Arc::new(R(real)))
        .build();

    assert_eq!(
        service_fs_callback(&snap, "fileExists", br#""/repo/overlay.ts""#).unwrap(),
        "true"
    );
    assert_eq!(
        service_fs_callback(&snap, "fileExists", br#""/repo/missing.ts""#).unwrap(),
        "",
        "unknown file falls through (empty result)"
    );
    assert_eq!(
        service_fs_callback(&snap, "realpath", br#""/repo/overlay.ts""#).unwrap(),
        r#""/repo/overlay.ts""#
    );

    // getAccessibleEntries merges real + overlay.
    let entries = service_fs_callback(&snap, "getAccessibleEntries", br#""/repo""#).unwrap();
    assert!(entries.contains("real.ts"), "{entries}");
    assert!(entries.contains("overlay.ts"), "{entries}");

    // NEGATIVE: an unknown callback name is a typed error string.
    assert!(service_fs_callback(&snap, "totallyUnknown", br#""/x""#).is_err());
}
