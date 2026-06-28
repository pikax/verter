//! Unit tests for the attach orchestration's pure parsing + seam shape. The
//! LIVE end-to-end attach proof (against a real tsgo) lives in
//! `tests/attach_live.rs` (gated on `VERTER_REQUIRE_TSGO`).

use super::*;

#[test]
fn initialize_api_session_method_string_is_exact() {
    // The method string is server-side (Go binary), verified against the shipped
    // native-preview binary. Pin it so a typo cannot silently break the attach.
    assert_eq!(INITIALIZE_API_SESSION_METHOD, "custom/initializeAPISession");
}

#[tokio::test]
async fn initialize_api_session_parses_session_and_pipe() {
    // Drive the handshake parse over an in-memory duplex with a fake server that
    // answers `custom/initializeAPISession` with `{ sessionId, pipe }` (the exact
    // shape the live binary returns).
    use crate::jsonrpc::framing::{encode_message, MessageFramer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);

    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let (Some(id), Some(method)) = (
                    msg.get("id").cloned(),
                    msg.get("method").and_then(|m| m.as_str()),
                ) {
                    if method == INITIALIZE_API_SESSION_METHOD {
                        let reply = serde_json::json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": { "sessionId": "api-session-1", "pipe": r"\\.\pipe\tsgo-api-abc-def" }
                        });
                        let _ = sw.write_all(&encode_message(&reply)).await;
                        let _ = sw.flush().await;
                    }
                }
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let handle = TsgoAttach::initialize_api_session(&conn)
        .await
        .expect("attach handshake ok");
    assert_eq!(handle.session_id, "api-session-1");
    assert_eq!(handle.pipe, r"\\.\pipe\tsgo-api-abc-def");
    conn.close().await.unwrap();
}

#[tokio::test]
async fn initialize_api_session_missing_pipe_is_a_typed_error() {
    use crate::jsonrpc::framing::{encode_message, MessageFramer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);

    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let Some(id) = msg.get("id").cloned() {
                    // Answer with NO pipe field (a malformed/old server).
                    let reply = serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": { "sessionId": "x" }
                    });
                    let _ = sw.write_all(&encode_message(&reply)).await;
                    let _ = sw.flush().await;
                }
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let err = TsgoAttach::initialize_api_session(&conn)
        .await
        .expect_err("a result without `pipe` must fail");
    assert!(
        matches!(err, TsgoApiError::Transport(_)),
        "a missing `pipe` must be a typed Transport error, got {err:?}"
    );
    conn.close().await.unwrap();
}
