//! Tests for [`ApiAttachClient`] driven over an in-memory duplex by a fake `--api`
//! server. NON-VACUOUS: real framing + real typed DTO (de)serialization; asserts
//! the EXACT param shapes the attach wire requires (e.g. `updateSnapshot` carries
//! `openProject` ONLY) and that diagnostics decode with their TS codes.

use super::*;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A shared recorder of the (method, params) the client sent — lets a test assert
/// the exact wire shape.
type Recorder = Arc<parking_lot::Mutex<Vec<(String, serde_json::Value)>>>;

/// A fake `--api` server: for each request, records the (method, params) and
/// replies with a canned result keyed on the method. The recorded params let the
/// test assert the exact wire shape the client sent.
async fn fake_api_server(
    mut reader: tokio::io::ReadHalf<tokio::io::DuplexStream>,
    mut writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    recorder: Recorder,
) {
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk).await {
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
            let params = msg
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let (Some(id), Some(method)) = (id, method) else {
                continue;
            };
            if id.is_null() {
                continue;
            }
            recorder.lock().push((method.clone(), params.clone()));
            let result = match method.as_str() {
                "initialize" => serde_json::json!({
                    "useCaseSensitiveFileNames": false,
                    "currentDirectory": "/ws"
                }),
                "updateSnapshot" => serde_json::json!({
                    "snapshot": 1,
                    "projects": [{
                        "id": "proj-1",
                        "configFileName": "/ws/tsconfig.json",
                        "compilerOptions": {},
                        "rootFiles": ["/ws/src/Widget.vue.tsx"]
                    }]
                }),
                "getSemanticDiagnostics" => serde_json::json!([
                    { "code": 2345, "category": 1, "text": "Argument of type 'string'...", "pos": 10, "end": 15, "fileName": "/ws/src/Widget.vue.tsx" }
                ]),
                "getTypeAtPosition" => serde_json::json!({ "id": 7, "flags": 1 }),
                "typeToString" => serde_json::json!("number"),
                _ => serde_json::Value::Null,
            };
            let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = writer.write_all(&encode_message(&reply)).await;
            let _ = writer.flush().await;
        }
    }
}

fn setup() -> (ApiAttachClient, Recorder) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (sr, sw) = tokio::io::split(server);
    let recorder = Arc::new(parking_lot::Mutex::new(Vec::new()));
    tokio::spawn(fake_api_server(sr, sw, Arc::clone(&recorder)));
    let conn = JsonRpcConnection::connect(cr, cw);
    (ApiAttachClient::new(conn), recorder)
}

#[tokio::test]
async fn initialize_decodes_typed_response() {
    let (client, _rec) = setup();
    let init = client.initialize().await.expect("initialize ok");
    assert!(!init.use_case_sensitive_file_names);
    assert_eq!(init.current_directory, "/ws");
}

#[tokio::test]
async fn update_snapshot_sends_open_project_only() {
    let (client, recorder) = setup();
    let snap = client
        .update_snapshot_open_project("/ws/tsconfig.json")
        .await
        .expect("updateSnapshot ok");
    // The fake server issues an INTEGER handle (1); it decodes into OpaqueHandle.
    assert_eq!(snap.snapshot, OpaqueHandle(1));
    assert_eq!(snap.projects.len(), 1);
    // The path-bearing project id stays a STRING.
    assert_eq!(snap.projects[0].id, "proj-1");
    assert_eq!(snap.projects[0].root_files, vec!["/ws/src/Widget.vue.tsx"]);

    // DISCRIMINATING: the attach `updateSnapshot` carries `openProject` ONLY — no
    // `fileChanges`, no `openFiles` (the --lsp server owns documents).
    let rec = recorder.lock();
    let (m, params) = rec
        .iter()
        .find(|(m, _)| m == "updateSnapshot")
        .expect("recorded");
    assert_eq!(m, "updateSnapshot");
    assert_eq!(
        params["openProject"],
        serde_json::json!("/ws/tsconfig.json")
    );
    assert!(
        params.get("fileChanges").is_none(),
        "attach updateSnapshot must NOT send fileChanges: {params}"
    );
    assert!(
        params.get("openFiles").is_none(),
        "attach updateSnapshot must NOT send openFiles: {params}"
    );
    let obj = params.as_object().expect("params is an object");
    assert_eq!(obj.len(), 1, "openProject must be the ONLY param: {params}");
}

#[tokio::test]
async fn semantic_diagnostics_decode_with_codes() {
    let (client, recorder) = setup();
    let snap = OpaqueHandle(1);
    let diags = client
        .get_semantic_diagnostics(&snap, "proj-1", "/ws/src/Widget.vue.tsx")
        .await
        .expect("diagnostics ok");
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, 2345);
    assert_eq!(
        diags[0].file_name.as_deref(),
        Some("/ws/src/Widget.vue.tsx")
    );

    // DISCRIMINATING: the request carried snapshot + project + file.
    let rec = recorder.lock();
    let (_m, params) = rec
        .iter()
        .find(|(m, _)| m == "getSemanticDiagnostics")
        .expect("recorded");
    assert_eq!(params["snapshot"], serde_json::json!(1));
    assert!(
        params["snapshot"].is_number(),
        "the snapshot handle rides the wire as a JSON integer, not a string: {params}"
    );
    assert_eq!(params["project"], serde_json::json!("proj-1"));
    assert_eq!(params["file"], serde_json::json!("/ws/src/Widget.vue.tsx"));
}

#[tokio::test]
async fn type_at_position_and_type_to_string() {
    let (client, _rec) = setup();
    let snap = OpaqueHandle(1);
    let ty = client
        .get_type_at_position(&snap, "proj-1", "/ws/src/Widget.vue.tsx", 42)
        .await
        .expect("type ok")
        .expect("some type");
    assert_eq!(ty.id, OpaqueHandle(7));
    let display = client
        .type_to_string(&snap, "proj-1", &ty.id)
        .await
        .expect("typeToString ok");
    assert_eq!(display, "number");
}

#[tokio::test]
async fn project_for_config_predicate() {
    let (client, _rec) = setup();
    let snap = client
        .update_snapshot_open_project("/ws/tsconfig.json")
        .await
        .unwrap();
    let proj = snap
        .project_for_config(|c| c == "/ws/tsconfig.json")
        .expect("found the configured project");
    assert_eq!(proj.id, "proj-1");
    assert!(
        snap.project_for_config(|c| c == "/nope/tsconfig.json")
            .is_none(),
        "an unmatched config must yield None"
    );
}
