//! Discriminating tests for [`JsonRpcConnection`] over an in-memory duplex.
//!
//! A "fake server" task reads framed requests off one end of a `tokio::io::duplex`
//! pair and writes framed responses back, so the connection is exercised
//! end-to-end (NON-VACUOUS: real framing, real id-correlation, real async I/O)
//! without a live tsgo process.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Split a `duplex` pair into the two halves the connection + fake server use.
/// Returns `(client_read, client_write, server_read, server_write)`.
fn duplex_pair() -> (
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (sr, sw) = tokio::io::split(server);
    (cr, cw, sr, sw)
}

/// A fake server that echoes each request as a response, mapping the request
/// `method` to a deterministic `result`. Calls `on_request` for each.
async fn fake_server<F>(
    mut reader: tokio::io::ReadHalf<tokio::io::DuplexStream>,
    mut writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    mut respond: F,
) where
    F: FnMut(&str, &serde_json::Value) -> serde_json::Value + Send,
{
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        framer.push(&chunk[..n]);
        while let Ok(Some(msg)) = framer.next_message() {
            // Only requests (with id + method) get a response.
            let id = msg.get("id").cloned();
            let method = msg.get("method").and_then(|m| m.as_str());
            if let (Some(id), Some(method)) = (id, method) {
                if id.is_null() {
                    continue;
                }
                let params = msg
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let result = respond(method, &params);
                let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
                if writer.write_all(&encode_message(&reply)).await.is_err() {
                    break;
                }
                let _ = writer.flush().await;
            }
        }
    }
}

#[tokio::test]
async fn request_response_roundtrip() {
    let (cr, cw, sr, sw) = duplex_pair();
    tokio::spawn(fake_server(
        sr,
        sw,
        |method, params| serde_json::json!({ "echoed_method": method, "echoed_params": params }),
    ));

    let conn = JsonRpcConnection::connect(cr, cw);
    let result = conn
        .request("initialize", serde_json::json!({ "x": 1 }))
        .await
        .expect("request ok");
    assert_eq!(result["echoed_method"], serde_json::json!("initialize"));
    assert_eq!(result["echoed_params"], serde_json::json!({ "x": 1 }));
    conn.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_requests_correlate_by_id() {
    // The fake server replies with the method name; if id-correlation were broken,
    // overlapping in-flight requests would cross their responses.
    let (cr, cw, sr, sw) = duplex_pair();
    tokio::spawn(fake_server(
        sr,
        sw,
        |method, _params| serde_json::json!({ "who": method }),
    ));

    let conn = JsonRpcConnection::connect(cr, cw);
    let a = conn.request("alpha", serde_json::Value::Null);
    let b = conn.request("beta", serde_json::Value::Null);
    let c = conn.request("gamma", serde_json::Value::Null);
    let (ra, rb, rc) = tokio::join!(a, b, c);
    assert_eq!(ra.unwrap()["who"], serde_json::json!("alpha"));
    assert_eq!(rb.unwrap()["who"], serde_json::json!("beta"));
    assert_eq!(rc.unwrap()["who"], serde_json::json!("gamma"));
    conn.close().await.unwrap();
}

#[tokio::test]
async fn server_to_client_request_is_auto_answered() {
    // The fake server, on receiving `initialize`, FIRST sends a server→client
    // request (`client/registerCapability`) and waits for its answer before
    // replying to `initialize`. If the connection did not auto-answer the
    // server→client request, the server would block and `initialize` would hang.
    let (cr, cw, mut sr, mut sw) = duplex_pair();
    let saw_answer = Arc::new(AtomicUsize::new(0));
    let saw_answer_task = Arc::clone(&saw_answer);

    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        let mut server_req_id = 1000;
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                let id = msg.get("id").cloned();
                let method = msg
                    .get("method")
                    .and_then(|m| m.as_str())
                    .map(str::to_owned);
                match (id, method) {
                    // Client request `initialize`: first ask the client a question.
                    (Some(client_id), Some(m)) if !client_id.is_null() && m == "initialize" => {
                        let server_req = serde_json::json!({
                            "jsonrpc": "2.0", "id": server_req_id,
                            "method": "client/registerCapability", "params": {}
                        });
                        server_req_id += 1;
                        sw.write_all(&encode_message(&server_req)).await.unwrap();
                        sw.flush().await.unwrap();
                        // Reply to initialize only AFTER the question is sent.
                        let reply = serde_json::json!({
                            "jsonrpc": "2.0", "id": client_id, "result": { "ok": true }
                        });
                        sw.write_all(&encode_message(&reply)).await.unwrap();
                        sw.flush().await.unwrap();
                    }
                    // The client's ANSWER to our server→client request (a response,
                    // no method, id == our server_req_id).
                    (Some(answer_id), None) if answer_id.as_i64() == Some(1000) => {
                        saw_answer_task.fetch_add(1, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let result = conn
        .request("initialize", serde_json::Value::Null)
        .await
        .expect("initialize ok");
    assert_eq!(result["ok"], serde_json::json!(true));
    // Give the server task a moment to observe the auto-answer.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        saw_answer.load(Ordering::SeqCst),
        1,
        "the connection must auto-answer the server→client request"
    );
    conn.close().await.unwrap();
}

#[tokio::test]
async fn abandoned_request_is_pruned() {
    // A server that NEVER replies. Dropping the request future must prune the
    // pending entry (abandon-only cancel) so it does not leak.
    let (cr, cw, mut sr, _sw) = duplex_pair();
    tokio::spawn(async move {
        // Drain forever, never reply.
        let mut buf = [0u8; 1024];
        while let Ok(n) = sr.read(&mut buf).await {
            if n == 0 {
                break;
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    {
        let fut = conn.request("getSemanticDiagnostics", serde_json::Value::Null);
        // Abandon it: time out the await and drop the future.
        let timed = tokio::time::timeout(std::time::Duration::from_millis(50), fut).await;
        assert!(timed.is_err(), "the never-answered request must time out");
    }
    // After dropping the abandoned future, a subsequent close must not deadlock
    // and the connection is still usable for shutdown.
    conn.close().await.unwrap();
}

#[tokio::test]
async fn closed_connection_fails_request() {
    let (cr, cw, sr, sw) = duplex_pair();
    // Server drops both ends immediately → EOF on the client read.
    drop(sr);
    drop(sw);

    let conn = JsonRpcConnection::connect(cr, cw);
    // The read task hits EOF and clears waiters; the request fails Closed.
    let err = conn
        .request("initialize", serde_json::Value::Null)
        .await
        .expect_err("request on a closed connection must fail");
    assert!(
        matches!(err, TsgoApiError::Closed | TsgoApiError::Transport(_)),
        "a closed connection must fail with Closed/Transport, got {err:?}"
    );
}

/// A peer NOTIFICATION (a `method` with no `id`, e.g. the control server's
/// `verter/fatal` liveness signal) is SURFACED to the installed notification handler.
///
/// RED before the fix: `route_message` dropped every notification on the floor
/// (`(false, _) => {}`), so a dead-relay `verter/fatal` never reached the client — the
/// SHARED overlay could not learn the transport died and occupied a dead provider
/// until LSP restart.
#[tokio::test]
async fn peer_notification_is_surfaced_to_handler() {
    let (cr, cw, _sr, mut sw) = duplex_pair();
    let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen_handler = Arc::clone(&seen);

    let conn = JsonRpcConnection::connect_with_handlers(
        cr,
        cw,
        Arc::new(|_method, _params| serde_json::Value::Null),
        Arc::new(move |method, _params| {
            seen_handler.lock().unwrap().push(method.to_string());
        }),
    );

    // The server sends a `verter/fatal` NOTIFICATION (no id) to the client.
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "verter/fatal",
        "params": { "reason": "relay_death", "detail": "relay stopped pumping" }
    });
    sw.write_all(&encode_message(&notification)).await.unwrap();
    sw.flush().await.unwrap();

    // Give the reader task a moment to route it.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(
        *seen.lock().unwrap(),
        vec!["verter/fatal".to_string()],
        "a peer notification must be surfaced to the notification handler, not ignored"
    );
    conn.close().await.unwrap();
}

/// The connection-death liveness signal: `is_closed()` is `false` while live and
/// flips to `true` after the peer EOFs (the reader task ends), so a caller can detect a
/// dead transport WITHOUT issuing a request.
#[tokio::test]
async fn is_closed_reflects_connection_death() {
    let (cr, cw, sr, sw) = duplex_pair();
    let conn = JsonRpcConnection::connect(cr, cw);
    assert!(!conn.is_closed(), "a fresh live connection is not closed");

    // The peer drops both ends → EOF on the client read → the reader task ends.
    drop(sr);
    drop(sw);
    // Give the reader task a moment to observe EOF.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        conn.is_closed(),
        "a connection whose peer EOFed must report is_closed() == true"
    );
}
