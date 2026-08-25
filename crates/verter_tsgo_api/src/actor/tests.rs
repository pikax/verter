//! Actor tests driven by a mock engine over an in-memory [`FrameStream`].
//!
//! A mock engine task reads the actor's outbound frames and replies with
//! crafted inbound frames, letting us exercise request/response correlation,
//! inline host-callback servicing (the deadlock guard), cancellation, restart,
//! and backpressure deterministically — without a real tsgo process.

use std::time::Duration;

use tokio::sync::mpsc;

use super::{service_fs_callback, spawn_actor, ActorRequest, CancelToken, RequestOptions};
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
        terminated_notify: Arc<tokio::sync::Notify>,
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
            self.terminated_notify.notify_waiters();
        }
    }

    let terminated = Arc::new(AtomicBool::new(false));
    let terminated_notify = Arc::new(tokio::sync::Notify::new());
    let (outbound_tx, mut to_engine) = mpsc::channel::<Vec<u8>>(64);
    let transport = WedgedTransport {
        sent: outbound_tx,
        terminated: Arc::clone(&terminated),
        terminated_notify: Arc::clone(&terminated_notify),
    };
    let handle = spawn_actor(transport, OverlaySnapshot::builder().build(), 8);

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
    if !terminated.load(Ordering::SeqCst) {
        let notified = terminated_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !terminated.load(Ordering::SeqCst) {
            tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .expect("an admitted deadline must terminate the transport");
        }
    }
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

    // Deterministic ordering AND a deterministic observation point:
    //
    // (1) Cancel must happen only once the request is genuinely in flight
    // (received by the engine), not before it is even sent — that earlier
    // case is `cancel_before_send_skips_the_request`, a different code path.
    // `received_tx`/`received_rx` proves that ordering.
    //
    // (2) The engine sends its (late) response only AFTER `cancelled_rx`
    // resolves, and the canceller only signals `cancelled_tx` AFTER calling
    // `tok2.cancel()`. A oneshot channel's send/receive is a real
    // synchronization edge (not a timing guess), so by the time the actor
    // reads the response frame off the wire, `tok.is_cancelled()` is
    // guaranteed to observe `true`.
    //
    // (3) Critically, we bypass `ClientHandle::request` and its
    // `select! { reply_rx, wait_cancelled(tok) }` entirely, observing the
    // actor's raw reply oneshot directly. Going through `request()` would
    // reintroduce a genuine, scheduling-dependent race on the CLIENT side:
    // `wait_cancelled` is a pin-enable-recheck `Notify` loop over
    // `tok.is_cancelled()`, which — with `biased` ordering — only loses
    // to `reply_rx` when the actor's send is already ready at the select.
    // Under normal scheduling the cancel notify frequently wins FIRST
    // (the engine round-trip above crosses several task-scheduling hops),
    // so `wait_cancelled` alone can produce `Cancelled` even from the OLD,
    // buggy `Ok(None) => Ok(())` actor arm that silently dropped the reply
    // channel — the client-side race can mask the exact bug this test
    // exists to catch. Awaiting the raw reply channel directly removes that
    // masking: the assertion below reflects ONLY what the actor itself sent
    // on the reply channel, unconditionally.
    let (received_tx, received_rx) = tokio::sync::oneshot::channel::<()>();
    let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel::<()>();

    // Engine: receive the request, signal it is in flight, then wait for
    // confirmation that the token has been cancelled before responding, then
    // serve one more, differently-named request — proving the actor actually
    // DRAINED (consumed) the late response rather than leaving it on the wire
    // to be misdelivered as the next request's reply (which would trip the
    // name-correlation check in `serve_frames`).
    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        let _ = received_tx.send(());
        cancelled_rx
            .await
            .expect("canceller must signal before dropping");
        let resp = encode_frame(MessageType::Response, req.name, b"late");
        // The actor must drain (discard) this response after cancellation.
        let _ = from_engine.send(resp).await;

        let raw2 = to_engine.recv().await.unwrap();
        let (req2, _) = decode_frame(&raw2, 0).unwrap();
        assert_eq!(
            req2.name, b"followUp",
            "the follow-up request must arrive un-corrupted: a stale late \
             response left undrained would desync the wire"
        );
        let resp2 = encode_frame(MessageType::Response, req2.name, b"ok");
        from_engine.send(resp2).await.unwrap();
    });

    let canceller = tokio::spawn(async move {
        received_rx
            .await
            .expect("engine must signal before dropping");
        tok2.cancel();
        let _ = cancelled_tx.send(());
    });

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let req = ActorRequest {
        method: "getSemanticDiagnostics".to_string(),
        payload: b"{}".to_vec(),
        cancel: Some(tok),
        deadline_at: None,
        reply: reply_tx,
    };
    handle
        .interactive_tx
        .send(req)
        .await
        .expect("actor lane must accept the request");

    let result = reply_rx
        .await
        .expect("the actor must answer the reply channel, not drop it");
    assert!(
        matches!(result, Err(crate::error::TsgoApiError::Cancelled)),
        "cancelled in-flight request must resolve Cancelled on the actor's \
         own reply channel, got {result:?}"
    );

    // The wire must be clean: a follow-up request on the SAME handle proves
    // the actor actually consumed (drained) the late response above, rather
    // than merely racing the client to a `Cancelled` outcome while leaving
    // the response frame unread.
    let follow_up = handle
        .request("followUp", b"{}".to_vec(), RequestOptions::default())
        .await
        .expect("the wire must be clean for the next request after a drained cancellation");
    assert_eq!(follow_up, b"ok");

    engine.await.unwrap();
    canceller.await.unwrap();
}

// ── A THIRD, distinct cancellation entrance: a request cancelled while it is
//    genuinely QUEUED — enqueued but not yet dequeued by the actor — must be
//    skipped by the pre-send check in `Actor::run` (mod.rs) rather than
//    written to the wire. This is neither `cancel_before_send_skips_the_request`
//    (cancelled before `ClientHandle::request` even enqueues) nor
//    `cancel_in_flight_resolves_cancelled_and_drains_response` (cancelled after
//    the request frame is already on the wire). We bypass `ClientHandle::request`
//    and enqueue an already-cancelled `ActorRequest` directly so there is no
//    ordering ambiguity about when the cancellation took effect relative to
//    enqueue; a first "hold" request keeps the actor busy (blocked reading its
//    response) so the queued, cancelled request cannot be dequeued until we
//    release the hold — giving full deterministic control with real channel
//    rendezvous only, no sleeps.
#[tokio::test]
async fn cancel_while_queued_is_never_written_to_the_wire() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let (hold_received_tx, hold_received_rx) = tokio::sync::oneshot::channel::<()>();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();

    let engine = tokio::spawn(async move {
        // 1. The actor writes the "hold" request; signal it is genuinely
        // in-flight (blocked reading a response), then wait to be released.
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        assert_eq!(req.name, b"hold");
        let _ = hold_received_tx.send(());
        release_rx.await.expect("test must release the hold");
        let resp = encode_frame(MessageType::Response, req.name, b"held");
        from_engine.send(resp).await.unwrap();

        // 2. The NEXT frame the engine sees must be "followUp" — the
        // cancelled "toCancel" request must never reach the wire.
        let raw2 = to_engine.recv().await.unwrap();
        let (req2, _) = decode_frame(&raw2, 0).unwrap();
        assert_eq!(
            req2.name, b"followUp",
            "a request cancelled while still queued must never be written to \
             the wire"
        );
        let resp2 = encode_frame(MessageType::Response, req2.name, b"ok");
        from_engine.send(resp2).await.unwrap();
    });

    let hold_task = tokio::spawn({
        let handle = handle.clone();
        async move {
            handle
                .request("hold", b"{}".to_vec(), RequestOptions::default())
                .await
        }
    });

    // Wait until the actor is genuinely blocked serving "hold" before
    // queueing the cancelled request — otherwise it could race the actor's
    // own dequeue and land on the `cancel_before_send_skips_the_request`
    // path inside `ClientHandle::request` instead of the actor-side check
    // this test targets.
    hold_received_rx
        .await
        .expect("engine must signal hold is in flight");

    // Enqueue an already-cancelled request directly (bypassing
    // `ClientHandle::request`'s own pre-submission check) so there is no
    // ambiguity about when cancellation took effect: the actor can only ever
    // observe it as already-cancelled once it dequeues.
    let tok = CancelToken::new();
    tok.cancel();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let queued = ActorRequest {
        method: "toCancel".to_string(),
        payload: b"{}".to_vec(),
        cancel: Some(tok),
        deadline_at: None,
        reply: reply_tx,
    };
    handle
        .interactive_tx
        .send(queued)
        .await
        .expect("actor lane must accept the queued request");

    // Release the hold: the actor loops back, dequeues the cancelled
    // request, and must skip it without writing a frame.
    let _ = release_tx.send(());

    let queued_result = reply_rx
        .await
        .expect("the actor must answer the queued reply channel, not drop it");
    assert!(
        matches!(queued_result, Err(crate::error::TsgoApiError::Cancelled)),
        "a request cancelled while queued must resolve Cancelled, got {queued_result:?}"
    );

    assert_eq!(
        hold_task
            .await
            .unwrap()
            .expect("the hold request itself must still complete normally"),
        b"held"
    );

    // A genuinely fresh request proves the wire is clean: the actor moved
    // straight from "hold" to "followUp" without ever writing "toCancel".
    let follow_up = handle
        .request("followUp", b"{}".to_vec(), RequestOptions::default())
        .await
        .expect("the wire must be clean after skipping the queued cancellation");
    assert_eq!(follow_up, b"ok");

    engine.await.unwrap();
}

#[tokio::test]
async fn cancel_in_flight_error_frame_resolves_cancelled() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let tok = CancelToken::new();
    let tok2 = tok.clone();
    let (received_tx, received_rx) = tokio::sync::oneshot::channel::<()>();
    let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel::<()>();

    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        let _ = received_tx.send(());
        cancelled_rx
            .await
            .expect("canceller must signal before dropping");
        let err = encode_frame(MessageType::Error, req.name, b"late-error");
        let _ = from_engine.send(err).await;

        let raw2 = to_engine.recv().await.unwrap();
        let (req2, _) = decode_frame(&raw2, 0).unwrap();
        assert_eq!(
            req2.name, b"followUp",
            "a cancelled error frame must be drained, not misdelivered"
        );
        let resp2 = encode_frame(MessageType::Response, req2.name, b"ok");
        from_engine.send(resp2).await.unwrap();
    });

    let canceller = tokio::spawn(async move {
        received_rx
            .await
            .expect("engine must signal before dropping");
        tok2.cancel();
        let _ = cancelled_tx.send(());
    });

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    handle
        .interactive_tx
        .send(ActorRequest {
            method: "getSemanticDiagnostics".to_string(),
            payload: b"{}".to_vec(),
            cancel: Some(tok),
            deadline_at: None,
            reply: reply_tx,
        })
        .await
        .expect("actor lane must accept the request");

    let result = reply_rx
        .await
        .expect("the actor must answer the reply channel, not drop it");
    assert!(
        matches!(result, Err(crate::error::TsgoApiError::Cancelled)),
        "a cancelled in-flight request that completes with an Error frame \
         must still resolve Cancelled, got {result:?}"
    );

    let follow_up = handle
        .request("followUp", b"{}".to_vec(), RequestOptions::default())
        .await
        .expect("the wire must be clean after draining a cancelled error frame");
    assert_eq!(follow_up, b"ok");

    engine.await.unwrap();
    canceller.await.unwrap();
}

#[tokio::test]
async fn cancel_in_flight_via_client_handle_resolves_cancelled() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    let tok = CancelToken::new();
    let tok2 = tok.clone();
    let (received_tx, received_rx) = tokio::sync::oneshot::channel::<()>();
    let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel::<()>();

    let engine = tokio::spawn(async move {
        let raw = to_engine.recv().await.unwrap();
        let (req, _) = decode_frame(&raw, 0).unwrap();
        let _ = received_tx.send(());
        cancelled_rx
            .await
            .expect("canceller must signal before dropping");
        let resp = encode_frame(MessageType::Response, req.name, b"late");
        let _ = from_engine.send(resp).await;
    });

    let canceller = tokio::spawn(async move {
        received_rx
            .await
            .expect("engine must signal before dropping");
        tok2.cancel();
        let _ = cancelled_tx.send(());
    });

    let result = handle
        .request(
            "getSemanticDiagnostics",
            b"{}".to_vec(),
            RequestOptions {
                lane: Lane::Interactive,
                cancel: Some(tok),
                deadline: None,
            },
        )
        .await;
    assert!(
        matches!(result, Err(crate::error::TsgoApiError::Cancelled)),
        "ClientHandle::request must resolve Cancelled via the Notify wake, \
         not a 2ms poll race, got {result:?}"
    );

    engine.await.unwrap();
    canceller.await.unwrap();
}

#[tokio::test]
async fn interactive_lane_drains_before_batch() {
    let (stream, from_engine, mut to_engine) = duplex();
    let handle = spawn_actor(stream, OverlaySnapshot::builder().build(), 8);

    // Engine: record the order methods arrive in and echo a reply. The FIRST
    // request is held unanswered until the test releases it — that wedge is
    // what makes the ordering deterministic rather than a race, because it
    // keeps the actor inside `serve_one` while both lanes fill.
    let order = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let order2 = order.clone();
    let (release_wedge_tx, release_wedge_rx) = tokio::sync::oneshot::channel::<()>();
    let engine = tokio::spawn(async move {
        let mut release = Some(release_wedge_rx);
        for _ in 0..3 {
            let raw = to_engine.recv().await.unwrap();
            let (req, _) = decode_frame(&raw, 0).unwrap();
            order2
                .lock()
                .await
                .push(String::from_utf8_lossy(req.name).into_owned());
            if let Some(release) = release.take() {
                release.await.expect("the test releases the wedge");
            }
            from_engine
                .send(encode_frame(MessageType::Response, req.name, b"ok"))
                .await
                .unwrap();
        }
    });

    // Wedge the wire: this request reaches the engine, which holds it. The
    // actor is now inside `serve_one` and will not pick from either lane
    // until it is answered.
    let wedge_handle = handle.clone();
    let wedge = tokio::spawn(async move {
        wedge_handle
            .request(
                "wedgeOp",
                b"{}".to_vec(),
                RequestOptions {
                    lane: Lane::Interactive,
                    cancel: None,
                    deadline: None,
                },
            )
            .await
    });
    handle.wait_admitted().await;

    // Fill the BATCH lane first, then the INTERACTIVE lane. Both are admitted
    // receipts, so both requests are provably sitting in their lanes before
    // the actor is free to choose — which is what makes the choice, rather
    // than the arrival race, the thing under test.
    let batch_handle = handle.clone();
    let batch = tokio::spawn(async move {
        batch_handle
            .request(
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
    handle.wait_admitted().await;
    let interactive_handle = handle.clone();
    let inter = tokio::spawn(async move {
        interactive_handle
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
    });
    handle.wait_admitted().await;

    // Release the wedge. The actor now picks between two FULL lanes.
    release_wedge_tx
        .send(())
        .expect("the engine holds the wedge");
    assert_eq!(wedge.await.unwrap().unwrap(), b"ok");
    assert_eq!(inter.await.unwrap().unwrap(), b"ok");
    batch.await.unwrap().unwrap();
    engine.await.unwrap();

    let seen = order.lock().await.clone();
    assert_eq!(
        seen,
        vec![
            "wedgeOp".to_string(),
            "interactiveOp".to_string(),
            "batchOp".to_string(),
        ],
        "with BOTH lanes provably full, the actor must take the interactive \
         lane first — asserting only that both methods appear passes just as \
         well with the priority bias removed or reversed"
    );
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

/// A transport that accepts frames but NEVER responds.
struct WedgedTransport {
    sent: mpsc::Sender<Vec<u8>>,
    terminated: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl super::transport::DuplexTransport for WedgedTransport {
    async fn send_frame(&mut self, bytes: &[u8]) -> crate::error::TsgoApiResult<()> {
        self.sent
            .send(bytes.to_vec())
            .await
            .map_err(|_| crate::error::TsgoApiError::Transport("sink closed".into()))
    }

    async fn recv_frame(&mut self) -> crate::error::TsgoApiResult<Option<Vec<u8>>> {
        std::future::pending().await
    }

    async fn terminate(&mut self) {
        self.terminated
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Fill a depth-1 queue (A in serve, B occupying the slot) so C blocks
/// on reservation. Cancelling C must complete before A ever replies.
#[tokio::test(start_paused = true)]
async fn full_queue_cancel_completes_before_admission() {
    use crate::error::TsgoApiError;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let terminated = Arc::new(AtomicBool::new(false));
    let (outbound_tx, mut to_engine) = mpsc::channel::<Vec<u8>>(8);
    let handle = spawn_actor(
        WedgedTransport {
            sent: outbound_tx,
            terminated: Arc::clone(&terminated),
        },
        OverlaySnapshot::builder().build(),
        1,
    );

    let a = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .request("initialize", b"null".to_vec(), RequestOptions::default())
                .await
        })
    };
    handle.wait_admitted().await;
    let _written = to_engine
        .recv()
        .await
        .expect("A must have been written to the wedged engine");

    let b = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .request(
                    "getSemanticDiagnostics",
                    b"{}".to_vec(),
                    RequestOptions::default(),
                )
                .await
        })
    };
    handle.wait_admitted().await;

    let tok = CancelToken::new();
    let c = {
        let handle = handle.clone();
        let tok = tok.clone();
        tokio::spawn(async move {
            handle
                .request(
                    "getCompletions",
                    b"{}".to_vec(),
                    RequestOptions {
                        lane: Lane::Interactive,
                        cancel: Some(tok),
                        deadline: None,
                    },
                )
                .await
        })
    };
    handle.wait_reserve_pending().await;
    assert!(
        !c.is_finished(),
        "C must still be waiting for queue admission"
    );
    tok.cancel();
    let err = c
        .await
        .expect("cancel task")
        .expect_err("full-queue cancel must complete before admission");
    assert!(matches!(err, TsgoApiError::Cancelled), "got {err:?}");
    assert!(
        to_engine.try_recv().is_err(),
        "C must never have reached the wire"
    );
    assert!(
        !terminated.load(Ordering::SeqCst),
        "cancel-before-admission must not tear the engine down"
    );
    assert!(!a.is_finished(), "A is still wedged");
    assert!(!b.is_finished(), "B is still queued behind A");
}

/// Same full queue, but the waiter carries a deadline. The deadline
/// must fire on the original Instant (paused time) without waiting
/// for the wedged request to complete, and without tearing the
/// engine down — the actor never began serving the waiter.
#[tokio::test(start_paused = true)]
async fn full_queue_deadline_completes_before_admission() {
    use crate::error::TsgoApiError;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let terminated = Arc::new(AtomicBool::new(false));
    let (outbound_tx, mut to_engine) = mpsc::channel::<Vec<u8>>(8);
    let handle = spawn_actor(
        WedgedTransport {
            sent: outbound_tx,
            terminated: Arc::clone(&terminated),
        },
        OverlaySnapshot::builder().build(),
        1,
    );

    let _a = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .request("initialize", b"null".to_vec(), RequestOptions::default())
                .await
        })
    };
    handle.wait_admitted().await;
    let _ = to_engine
        .recv()
        .await
        .expect("A must have been written to the wedged engine");

    let _b = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .request(
                    "getSemanticDiagnostics",
                    b"{}".to_vec(),
                    RequestOptions::default(),
                )
                .await
        })
    };
    handle.wait_admitted().await;

    let start = tokio::time::Instant::now();
    let c = {
        let handle = handle.clone();
        tokio::spawn(async move {
            handle
                .request(
                    "getCompletions",
                    b"{}".to_vec(),
                    RequestOptions {
                        lane: Lane::Interactive,
                        cancel: None,
                        deadline: Some(Duration::from_millis(50)),
                    },
                )
                .await
        })
    };
    handle.wait_reserve_pending().await;
    assert!(!c.is_finished(), "C must be waiting for queue admission");
    tokio::time::advance(Duration::from_millis(50)).await;
    let err = c
        .await
        .expect("deadline task")
        .expect_err("full-queue deadline must complete before admission");
    assert!(matches!(err, TsgoApiError::Timeout(_)), "got {err:?}");
    assert_eq!(
        tokio::time::Instant::now().saturating_duration_since(start),
        Duration::from_millis(50),
        "the deadline must be the original Instant, not a fresh timeout after admission"
    );
    assert!(
        to_engine.try_recv().is_err(),
        "C must never have reached the wire"
    );
    assert!(
        !terminated.load(Ordering::SeqCst),
        "deadline-before-admission must not tear the engine down"
    );
}
