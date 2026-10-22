//! Tests for [`ApiAttachClient`] driven over an in-memory duplex by a fake `--api`
//! server. NON-VACUOUS: real framing + real typed DTO (de)serialization; asserts
//! the EXACT param shapes the attach wire requires (e.g. the FIRST
//! `getCurrentLanguageServerSnapshot` for a project adopts
//! `changes.openProjects: [tsconfig]` and later snapshots OMIT it — the
//! ref-counted open persists), that snapshots are released by handle, and that
//! diagnostics decode with their TS codes.

use super::*;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;
use crate::toolchain::policy::TYPESCRIPT_PIN;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A shared recorder of the (method, params) the client sent — lets a test assert
/// the exact wire shape.
type Recorder = Arc<parking_lot::Mutex<Vec<(String, serde_json::Value)>>>;

/// The project entry the fake server reports for `config`, in the 7.1
/// `ProjectResponse` shape (proto.generated.d.ts:423-429). The workspace root
/// config is `proj-1`; any other config is `proj:<config>`.
fn fake_project(config: &str) -> serde_json::Value {
    let id = if config == "/ws/tsconfig.json" {
        "proj-1".to_string()
    } else {
        format!("proj:{config}")
    };
    serde_json::json!({
        "id": id,
        "configFileName": config,
        "currentDirectory": "/ws",
        "dirty": false,
        "parsedCommandLine": {
            "fileNames": ["/ws/src/Widget.vue.tsx"],
            "options": {},
            "errors": []
        }
    })
}

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
                    "caseSensitivity": 0,
                    "currentDirectory": "/ws"
                }),
                "getCurrentLanguageServerSnapshot" => {
                    // Echo the opened project (or the root one once leased).
                    let config = params["changes"]["openProjects"][0]
                        .as_str()
                        .unwrap_or("/ws/tsconfig.json")
                        .to_string();
                    serde_json::json!({
                        "snapshot": 1,
                        "projects": [fake_project(&config)],
                        "operation": {}
                    })
                }
                "release" => serde_json::json!(true),
                "getSemanticDiagnostics" => serde_json::json!([
                    { "code": 2345, "category": 1, "text": "Argument of type 'string'...", "pos": 10, "end": 15, "fileName": "/ws/src/Widget.vue.tsx" }
                ]),
                "getTypeAtPosition" => serde_json::json!({ "id": 7, "flags": 1, "value": null }),
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
    assert!(!init.case_sensitive());
    assert_eq!(init.current_directory, "/ws");
}

#[tokio::test]
async fn first_snapshot_for_a_project_adopts_open_projects() {
    let (client, recorder) = setup();
    let snap = client
        .current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
        .await
        .expect("snapshot ok");
    // The fake server issues an INTEGER handle (1); it decodes into OpaqueHandle.
    assert_eq!(snap.snapshot, OpaqueHandle(1));
    assert_eq!(snap.projects.len(), 1);
    // The path-bearing project id stays a STRING.
    assert_eq!(snap.projects[0].id, "proj-1");
    assert_eq!(
        snap.projects[0].root_files(),
        ["/ws/src/Widget.vue.tsx".to_string()]
    );

    // DISCRIMINATING: the attach session reads the LANGUAGE SERVER's snapshot
    // (never `createSnapshot`, which would not see the --lsp overlays), names
    // no response base (so every project is listed), and adopts exactly the
    // ref-counted `openProjects: [tsconfig]` change — no file changes (the
    // --lsp server owns documents).
    let rec = recorder.lock();
    let (m, params) = rec
        .iter()
        .find(|(m, _)| m == "getCurrentLanguageServerSnapshot")
        .expect("recorded");
    assert_eq!(m, "getCurrentLanguageServerSnapshot");
    assert!(
        rec.iter()
            .all(|(m, _)| m != "createSnapshot" && m != "updateSnapshot"),
        "an attached session never mints an independent snapshot: {rec:?}"
    );
    assert_eq!(
        params,
        &serde_json::json!({
            "changes": { "openProjects": ["/ws/tsconfig.json"], "ensurePrograms": true }
        }),
        "the first open adopts openProjects and brings programs up to date, with no \
         response base: {params}"
    );
}

/// DISCRIMINATING (project-keyed lease): `openProjects` opens are ref-counted,
/// ADDITIVE, and PER-PROJECT. One `ApiAttachClient` serves MULTIPLE projects
/// (per-carrier), so the lease must be keyed on the tsconfig path, NOT a single
/// global "first snapshot" latch. Each DISTINCT project adopts `openProjects`
/// exactly once; a later snapshot of an ALREADY-open project omits it (the
/// refcount persists) and brings exactly that project's program up to date by
/// the id its opening snapshot reported.
#[tokio::test]
async fn open_projects_lease_is_per_project() {
    let (client, recorder) = setup();
    for tsconfig in [
        "/ws/A/tsconfig.json",
        "/ws/B/tsconfig.json",
        "/ws/A/tsconfig.json",
    ] {
        client
            .current_snapshot_for_project(tsconfig, TYPESCRIPT_PIN)
            .await
            .expect("snapshot ok");
    }

    let rec = recorder.lock();
    let snaps: Vec<&serde_json::Value> = rec
        .iter()
        .filter(|(m, _)| m == "getCurrentLanguageServerSnapshot")
        .map(|(_, p)| p)
        .collect();
    assert_eq!(snaps.len(), 3, "exactly three snapshot calls recorded");
    assert_eq!(
        snaps[0]["changes"]["openProjects"],
        serde_json::json!(["/ws/A/tsconfig.json"]),
        "call #1 leases openProjects[A]: {}",
        snaps[0]
    );
    assert_eq!(
        snaps[1]["changes"]["openProjects"],
        serde_json::json!(["/ws/B/tsconfig.json"]),
        "call #2 (a DIFFERENT project) MUST lease openProjects[B]: {}",
        snaps[1]
    );
    assert_eq!(
        snaps[0]["changes"]["ensurePrograms"],
        serde_json::json!(true),
        "an opening request cannot name the project id yet: {}",
        snaps[0]
    );
    assert_eq!(
        snaps[2],
        &serde_json::json!({ "changes": { "ensurePrograms": ["proj:/ws/A/tsconfig.json"] } }),
        "call #3 (A already leased) omits openProjects (the refcount persists) and \
         ensures A's program by id: {}",
        snaps[2]
    );
}

/// DISCRIMINATING: `release` hands a snapshot back by handle
/// (`{ snapshot: N }`, sync/api.js:1026).
#[tokio::test]
async fn release_sends_the_snapshot_handle() {
    let (client, recorder) = setup();
    let snap = client
        .current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
        .await
        .expect("snapshot ok");
    client.release(&snap.snapshot).await.expect("release ok");
    let rec = recorder.lock();
    let (_m, params) = rec
        .iter()
        .find(|(m, _)| m == "release")
        .expect("release recorded");
    assert_eq!(params, &serde_json::json!({ "snapshot": 1 }));
}

/// A fake `--api` server whose snapshot reply encodes a STRING snapshot handle
/// (`"n0000000000000003"`) — an opaque-handle wire class the codec does NOT
/// speak. Everything else mirrors [`fake_api_server`].
async fn fake_api_server_string_handle(
    mut reader: tokio::io::ReadHalf<tokio::io::DuplexStream>,
    mut writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
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
            let Some(id) = msg.get("id").cloned() else {
                continue;
            };
            if id.is_null() {
                continue;
            }
            let result = serde_json::json!({
                "snapshot": "n0000000000000003",
                "projects": []
            });
            let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = writer.write_all(&encode_message(&reply)).await;
            let _ = writer.flush().await;
        }
    }
}

/// DISCRIMINATING: the OWNED attach path's first-snapshot integer-handle rail.
/// An engine whose first snapshot handle is a STRING must be refused with a
/// typed `UnsupportedTsgoWire` naming the observed version — NOT the generic
/// `Json` decode error the raw `deserialize` would produce.
#[tokio::test]
async fn first_snapshot_string_handle_fails_closed() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(fake_api_server_string_handle(sr, sw));
    let client = ApiAttachClient::new(JsonRpcConnection::connect(cr, cw));

    let err = client
        .current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
        .await
        .expect_err("a string first snapshot handle must be refused");
    assert!(
        matches!(err, crate::error::TsgoApiError::UnsupportedTsgoWire(ref m)
            if m.contains(TYPESCRIPT_PIN) && m.contains("not a bare i64 integer")),
        "the refusal must be the typed UnsupportedTsgoWire naming the observed \
         engine version, not a generic Json decode error; got {err:?}"
    );
}

/// CONCURRENCY-SAFETY: the double-checked async init serializes the cold-start
/// rail so N concurrent FIRST snapshot calls all observe a consistent outcome —
/// no fail-open under contention, no panic, no deadlock. Runs on a
/// multi-threaded runtime for genuine parallel contention.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_first_snapshots_are_serialized_and_consistent() {
    const N: usize = 8;

    // (a) STRING-handle engine: EVERY concurrent first call is refused
    //     (fail-closed) — the rail never leaks a product result under a race.
    {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (cr, cw) = tokio::io::split(client);
        let (sr, sw) = tokio::io::split(server);
        tokio::spawn(fake_api_server_string_handle(sr, sw));
        let client = ApiAttachClient::new(JsonRpcConnection::connect(cr, cw));

        let mut handles = Vec::new();
        for _ in 0..N {
            let c = client.clone();
            handles.push(tokio::spawn(async move {
                c.current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
                    .await
            }));
        }
        for h in handles {
            let res = h.await.expect("task did not panic/deadlock");
            assert!(
                matches!(res, Err(crate::error::TsgoApiError::UnsupportedTsgoWire(_))),
                "every concurrent first call to a string-handle engine must be \
                 refused (no fail-open under contention); got {res:?}"
            );
        }
    }

    // (b) INTEGER-handle engine: EVERY concurrent first call succeeds and the
    //     client stays usable — the serialized rail admits the valid wire once
    //     and the waiters proceed on the fast path.
    {
        let (client, _recorder) = setup();
        let mut handles = Vec::new();
        for _ in 0..N {
            let c = client.clone();
            handles.push(tokio::spawn(async move {
                c.current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
                    .await
            }));
        }
        for h in handles {
            let snap = h
                .await
                .expect("task did not panic/deadlock")
                .expect("an integer-handle engine admits every concurrent first call");
            assert_eq!(snap.snapshot, OpaqueHandle(1));
        }
        let snap = client
            .current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
            .await
            .expect("post-race call still works");
        assert_eq!(snap.snapshot, OpaqueHandle(1));
    }
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

    // DISCRIMINATING: the request carried snapshot + project + the one-element
    // `files` list (an unknown scalar `file` would be ignored by the engine,
    // which then answers for the whole program).
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
    assert_eq!(
        params["files"],
        serde_json::json!(["/ws/src/Widget.vue.tsx"])
    );
    assert!(params.get("file").is_none(), "no scalar `file`: {params}");
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
        .current_snapshot_for_project("/ws/tsconfig.json", TYPESCRIPT_PIN)
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
