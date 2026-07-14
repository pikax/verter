//! Tests for the portable `--api` attach pipe transport.
//!
//! These stand up a REAL OS pipe/socket SERVER (not a tsgo process) to prove the
//! client connect + a JSON-RPC round-trip works end-to-end on the actual platform
//! transport, plus a DISCRIMINATING connect-failure (a non-existent pipe → typed
//! `Transport` error, never a panic or a false success).

use super::*;
use crate::jsonrpc::JsonRpcConnection;

/// A non-existent pipe path yields a typed `Transport` error.
#[tokio::test]
async fn connect_to_missing_pipe_is_a_typed_error() {
    #[cfg(windows)]
    let bogus = format!(
        r"\\.\pipe\verter-tsgo-attach-test-does-not-exist-{}",
        std::process::id()
    );
    #[cfg(unix)]
    let bogus = format!(
        "/tmp/verter-tsgo-attach-test-does-not-exist-{}.sock",
        std::process::id()
    );

    // The Ok variant (boxed trait-object halves) is not `Debug`, so match the
    // result directly rather than via `expect_err`.
    match connect_attach_pipe(&bogus).await {
        Ok(_) => panic!("connecting to a non-existent pipe must not succeed"),
        Err(TsgoApiError::Transport(_)) => {}
        Err(other) => panic!("a missing pipe must be a typed Transport error, got {other:?}"),
    }
}

/// A REAL platform pipe/socket server accepts the client connect and a JSON-RPC
/// `initialize` round-trips through `JsonRpcConnection` over the actual OS
/// transport. This proves the transport (not just the codec) carries the wire.
#[cfg(windows)]
#[tokio::test]
async fn windows_named_pipe_roundtrip() {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe = format!(
        r"\\.\pipe\verter-tsgo-attach-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    // Server: accept one client, echo each framed request as a `{ ok: method }` response.
    let server = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe)
        .expect("create named pipe server");
    let pipe_for_server = pipe.clone();
    let server_task = tokio::spawn(async move {
        server.connect().await.expect("server accept");
        echo_one_request(server).await;
        drop(pipe_for_server);
    });

    let (read, write) = connect_attach_pipe(&pipe).await.expect("client connect");
    let conn = JsonRpcConnection::connect(read, write);
    let result = conn
        .request("initialize", serde_json::Value::Null)
        .await
        .expect("initialize over named pipe");
    assert_eq!(result["ok"], serde_json::json!("initialize"));
    conn.close().await.unwrap();
    let _ = server_task.await;
}

/// The Unix-domain-socket counterpart of the round-trip proof.
#[cfg(unix)]
#[tokio::test]
async fn unix_socket_roundtrip() {
    use tokio::net::UnixListener;

    let dir = std::env::temp_dir().join(format!(
        "verter-tsgo-attach-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let sock = dir.join("api.sock");
    let sock_str = sock.to_string_lossy().into_owned();

    let listener = UnixListener::bind(&sock).expect("bind unix socket");
    let server_task = tokio::spawn(async move {
        let (stream, _addr) = listener.accept().await.expect("server accept");
        echo_one_request(stream).await;
    });

    let (read, write) = connect_attach_pipe(&sock_str)
        .await
        .expect("client connect");
    let conn = JsonRpcConnection::connect(read, write);
    let result = conn
        .request("initialize", serde_json::Value::Null)
        .await
        .expect("initialize over unix socket");
    assert_eq!(result["ok"], serde_json::json!("initialize"));
    conn.close().await.unwrap();
    let _ = server_task.await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Read one framed JSON-RPC request off `stream` and reply with `{ ok: <method> }`.
#[cfg(any(windows, unix))]
async fn echo_one_request<S>(mut stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use crate::jsonrpc::framing::{encode_message, MessageFramer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        framer.push(&chunk[..n]);
        while let Ok(Some(msg)) = framer.next_message() {
            if let (Some(id), Some(method)) = (
                msg.get("id").cloned(),
                msg.get("method").and_then(|m| m.as_str()),
            ) {
                let reply =
                    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "ok": method } });
                let _ = stream.write_all(&encode_message(&reply)).await;
                let _ = stream.flush().await;
                return;
            }
        }
    }
}
