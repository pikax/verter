//! Tests for the control endpoint: a REAL platform pipe/socket round-trip
//! through the shim-side [`ControlListener`] + the client-side connect, plus
//! the portable endpoint-path minting and a discriminating connect failure.

use super::*;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn unique_disamb() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[test]
fn endpoint_path_is_portable_and_platform_selected() {
    let dir = std::env::temp_dir();
    let endpoint = control_endpoint_path(&dir, r"C:\weird:session", 4242, "abc");
    #[cfg(windows)]
    {
        assert!(
            endpoint.starts_with(r"\\.\pipe\"),
            "a Windows control endpoint is a named pipe: {endpoint:?}"
        );
        // The name segment (after the pipe prefix) carries no NTFS-illegal chars.
        let name = endpoint.strip_prefix(r"\\.\pipe\").unwrap();
        for bad in ['<', '>', ':', '"', '|', '?', '*'] {
            assert!(!name.contains(bad), "pipe name must not contain {bad:?}");
        }
    }
    #[cfg(unix)]
    {
        assert!(
            endpoint.ends_with(".sock"),
            "a Unix control endpoint is a UDS path: {endpoint:?}"
        );
        assert!(
            endpoint.len() <= 108,
            "the UDS path must fit the sockaddr_un budget: {endpoint:?}"
        );
    }
    assert!(endpoint.contains("4242"), "the pid keys the endpoint");
}

/// A REAL control endpoint accepts the client connect and a JSON-RPC round-trip
/// crosses the actual OS transport — the net-new SERVER (listener) side proven
/// end-to-end (not just the codec).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_endpoint_round_trips_over_real_transport() {
    let dir = std::env::temp_dir();
    let endpoint =
        control_endpoint_path(&dir, "ctl-roundtrip", std::process::id(), &unique_disamb());
    let mut listener = ControlListener::bind(&endpoint).expect("bind control endpoint");
    let server_endpoint = listener.endpoint().to_string();

    // Server: accept one control connection, echo a framed request as `{ ok: method }`.
    let server_task = tokio::spawn(async move {
        let (mut read, mut write) = listener.accept().await.expect("accept control connection");
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match read.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let (Some(id), Some(method)) = (
                    msg.get("id").cloned(),
                    msg.get("method").and_then(|m| m.as_str()),
                ) {
                    let reply = serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": { "ok": method }
                    });
                    let _ = write.write_all(&encode_message(&reply)).await;
                    let _ = write.flush().await;
                    return;
                }
            }
        }
    });

    let (read, write) = connect_control_endpoint(&server_endpoint)
        .await
        .expect("client connect");
    let conn = JsonRpcConnection::connect(read, write);
    let result = conn
        .request("verter/hello", serde_json::json!({ "probe": true }))
        .await
        .expect("round-trip over the control endpoint");
    assert_eq!(result["ok"], serde_json::json!("verter/hello"));
    conn.close().await.unwrap();
    let _ = server_task.await;
}

/// A connect to a non-existent control endpoint is a typed error, never a panic
/// or a false success (fail closed).
#[tokio::test]
async fn connect_to_missing_control_endpoint_is_a_typed_error() {
    #[cfg(windows)]
    let bogus = format!(r"\\.\pipe\verter-relay-ctl-missing-{}", std::process::id());
    #[cfg(unix)]
    let bogus = format!("/tmp/verter-relay-ctl-missing-{}.sock", std::process::id());

    match connect_control_endpoint(&bogus).await {
        Ok(_) => panic!("connecting to a missing control endpoint must not succeed"),
        Err(crate::error::TsgoApiError::Transport(_)) => {}
        Err(other) => panic!("a missing endpoint must be a typed Transport error, got {other:?}"),
    }
}
