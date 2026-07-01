//! End-to-end integration test against a REAL tsgo `--api` engine.
//!
//! Spawns the actual tsgo process through the production stdio-pipe transport
//! and drives the single-writer actor through the real wire: `initialize`,
//! `updateSnapshot(openProject)`, and `getSemanticDiagnostics` on both a clean
//! and a deliberately-broken OVERLAY file (served only through the snapshot's
//! FS callbacks, never written to disk).
//!
//! Gating: this test runs NON-VACUOUSLY whenever the tsgo binary is present
//! (it is, in this worktree). Under `VERTER_REQUIRE_TSGO` a missing engine is a
//! hard failure (a skip would be a vacuous pass). Without that env var, a
//! genuinely-absent engine hermetic-skips.

mod common;

use std::sync::Arc;
use std::time::Duration;

use verter_tsgo_api::actor::spawn_actor;
use verter_tsgo_api::proto::types::{
    method, Diagnostic, InitializeResponse, UpdateSnapshotParams, UpdateSnapshotResponse,
};
use verter_tsgo_api::snapshot::{AccessibleEntries, OverlaySnapshot, RealDirSource};
use verter_tsgo_api::transport::pipe::StdioPipeTransport;
use verter_tsgo_api::RequestOptions;

/// A real-dir source backed by the actual filesystem, scoped to the fixture.
/// This is the role S3 fills with Verter's VFS; for the integration test we use
/// `std::fs` directly so `getAccessibleEntries` returns the real on-disk files
/// merged with the overlay carrier.
#[derive(Debug)]
struct StdFsDirSource;

impl RealDirSource for StdFsDirSource {
    fn real_entries(&self, dir: &str) -> Option<AccessibleEntries> {
        let rd = std::fs::read_dir(dir).ok()?;
        let mut files = Vec::new();
        let mut directories = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            match entry.file_type() {
                Ok(t) if t.is_dir() => directories.push(name),
                Ok(_) => files.push(name),
                Err(_) => {}
            }
        }
        Some(AccessibleEntries { files, directories })
    }
}

async fn req_json<T: serde::de::DeserializeOwned>(
    handle: &verter_tsgo_api::ClientHandle,
    method: &str,
    params: serde_json::Value,
) -> T {
    let payload = serde_json::to_vec(&params).unwrap();
    let bytes = tokio::time::timeout(
        Duration::from_secs(30),
        handle.request(method, payload, RequestOptions::default()),
    )
    .await
    .unwrap_or_else(|_| panic!("`{method}` timed out"))
    .unwrap_or_else(|e| panic!("`{method}` failed: {e}"));
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("`{method}` response decode failed: {e}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_engine_initialize_update_snapshot_and_diagnostics() {
    let Some(exe) = common::engine_or_skip() else {
        return;
    };

    // Hermetic fixture project in a temp dir.
    let tmp = tempdir();
    let tsconfig = common::write_fixture_project(&tmp);
    let src_dir = tmp.join("src");

    // The OVERLAY carrier: an off-disk .tsx that imports the real util.ts and
    // contains ONE deliberate type error (passing a string where a number is
    // required → TS2345). It is served ONLY via the snapshot.
    let carrier_path = src_dir.join("Carrier.tsx");
    let carrier_ok = "import { double } from \"./util\";\nexport const x: number = double(21);\n";
    let carrier_err =
        "import { double } from \"./util\";\nexport const x: number = double(\"nope\");\n";

    let snapshot = OverlaySnapshot::builder()
        .file(common::norm(&carrier_path), carrier_ok)
        .directory(common::norm(&src_dir))
        .real_dir_source(Arc::new(StdFsDirSource))
        .build();

    let transport = StdioPipeTransport::spawn(&exe, &tmp).expect("spawn real tsgo");
    let handle = spawn_actor(transport, snapshot, 16);

    // 1. initialize — the real engine reports its cwd + case sensitivity.
    let init: InitializeResponse =
        req_json(&handle, method::INITIALIZE, serde_json::Value::Null).await;
    assert!(
        !init.current_directory.is_empty(),
        "engine reported a current directory"
    );

    // 2. updateSnapshot(openProject) — opens the configured project; the carrier
    //    must appear in the project's root files (discovered via the merged
    //    getAccessibleEntries).
    let snap: UpdateSnapshotResponse = req_json(
        &handle,
        method::UPDATE_SNAPSHOT,
        serde_json::json!({ "openProject": common::norm(&tsconfig) }),
    )
    .await;
    // Got a snapshot handle faithful to the rc engine's wire shape — a bare
    // integer (NOT a string).
    let snap_wire = serde_json::to_value(snap.snapshot).expect("snapshot handle serializes");
    assert!(
        snap_wire.is_number() && !snap_wire.is_string(),
        "the rc snapshot handle is a JSON integer, not a string: {snap_wire}"
    );
    let project = snap
        .projects
        .iter()
        .find(|p| {
            common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
        })
        .expect("the opened project is in the snapshot");
    let carrier_norm = common::norm(&carrier_path);
    assert!(
        project
            .root_files
            .iter()
            .any(|f| common::norm(std::path::Path::new(f)) == carrier_norm),
        "the off-disk carrier is a project root file: {:?}",
        project.root_files
    );

    // 3. getSemanticDiagnostics on the CLEAN carrier → no TS2307 (module not
    //    found), proving `./util` resolved through the overlay+real merge.
    let clean_diags: Vec<Diagnostic> = req_json(
        &handle,
        method::GET_SEMANTIC_DIAGNOSTICS,
        serde_json::json!({
            "snapshot": snap.snapshot,
            "project": project.id,
            "file": carrier_norm,
        }),
    )
    .await;
    assert!(
        !clean_diags.iter().any(|d| d.code == 2307),
        "clean carrier has no TS2307 (module resolved): {clean_diags:?}"
    );
    assert!(
        clean_diags.is_empty(),
        "clean carrier type-checks with zero diagnostics: {clean_diags:?}"
    );

    // 4. Flip the carrier to the error variant via a fresh snapshot + a
    //    fileChanges delta, then re-check: TS2345 must fire on the off-disk file.
    let err_snapshot = OverlaySnapshot::builder()
        .file(&carrier_norm, carrier_err)
        .directory(common::norm(&src_dir))
        .real_dir_source(Arc::new(StdFsDirSource))
        .build();
    handle.publish_snapshot(err_snapshot);

    let params = UpdateSnapshotParams {
        open_project: Some(common::norm(&tsconfig)),
        file_changes: Some(verter_tsgo_api::proto::types::FileChanges::Summary(
            verter_tsgo_api::proto::types::FileChangeSummary {
                changed: Some(vec![
                    verter_tsgo_api::proto::types::DocumentIdentifier::file_name(
                        carrier_norm.clone(),
                    ),
                ]),
                ..Default::default()
            },
        )),
    };
    let snap2: UpdateSnapshotResponse = req_json(
        &handle,
        method::UPDATE_SNAPSHOT,
        serde_json::to_value(&params).unwrap(),
    )
    .await;
    let project2 = snap2
        .projects
        .iter()
        .find(|p| {
            common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
        })
        .expect("project present after delta");

    let err_diags: Vec<Diagnostic> = req_json(
        &handle,
        method::GET_SEMANTIC_DIAGNOSTICS,
        serde_json::json!({
            "snapshot": snap2.snapshot,
            "project": project2.id,
            "file": carrier_norm,
        }),
    )
    .await;
    assert!(
        err_diags.iter().any(|d| d.code == 2345),
        "the deliberate error fires TS2345 on the off-disk carrier: {err_diags:?}"
    );
    // NEGATIVE: it is NOT a module-resolution error (the import still resolved).
    assert!(
        !err_diags.iter().any(|d| d.code == 2307),
        "the error is a type error, not a false module-not-found: {err_diags:?}"
    );

    handle.close().await.expect("clean shutdown");
}

/// The typed [`TsgoClient`] connects through the fail-closed gate and serves
/// typed ops against the live engine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_client_connects_through_gate_and_serves_typed_ops() {
    use verter_tsgo_api::TsgoClient;

    let Some(exe) = common::engine_or_skip() else {
        return;
    };

    let tmp = tempdir();
    let tsconfig = common::write_fixture_project(&tmp);
    let carrier = tmp.join("src").join("Carrier.tsx");
    let carrier_norm = common::norm(&carrier);
    let content = "import { double } from \"./util\";\nexport const x: number = double(21);\n";

    let snapshot = OverlaySnapshot::builder()
        .file(&carrier_norm, content)
        .directory(common::norm(&tmp.join("src")))
        .real_dir_source(Arc::new(StdFsDirSource))
        .build();

    // connect() runs the wire gate first; a matching engine clears it.
    let client = TsgoClient::connect(&exe, &tmp, snapshot, 16)
        .expect("typed client connects through the gate");
    assert!(
        !client.clearance().capabilities.is_empty(),
        "gate confirmed at least one capability"
    );

    let init = client.initialize().await.expect("initialize");
    assert!(!init.current_directory.is_empty());

    let params = UpdateSnapshotParams {
        open_project: Some(common::norm(&tsconfig)),
        file_changes: None,
    };
    let snap = client
        .update_snapshot(&params)
        .await
        .expect("updateSnapshot");
    let project = snap
        .projects
        .iter()
        .find(|p| {
            common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
        })
        .expect("project opened");

    // Typed diagnostics: clean carrier → empty.
    let diags = client
        .get_semantic_diagnostics(&snap.snapshot, &project.id, &carrier_norm)
        .await
        .expect("semantic diagnostics");
    assert!(diags.is_empty(), "clean carrier: {diags:?}");

    // Typed type-at-position + typeToString → "number".
    let x_off = content.find("x:").unwrap() as u32;
    let ty = client
        .get_type_at_position(&snap.snapshot, &project.id, &carrier_norm, x_off)
        .await
        .expect("type at position")
        .expect("a type is present at `x`");
    let s = client
        .type_to_string(&snap.snapshot, &project.id, &ty.id)
        .await
        .expect("typeToString");
    assert_eq!(s, "number");

    client.close().await.expect("close");
}

/// DISCRIMINATING (whole-program vs per-file): a real, ON-DISK, NON-ROOT
/// imported `.ts` file carries a type error. The project's tsconfig lists ONLY
/// the carrier in `files` (no `include` glob), so the imported module enters the
/// program TRANSITIVELY, never as a root. The per-file `getSemanticDiagnostics`
/// on the carrier does NOT surface the imported file's error; the file-OMITTED
/// whole-program getter DOES. A clean control (imported file fixed) surfaces
/// none from either call.
///
/// This is the rootscope proof at the API-client layer: it FAILS to surface the
/// non-root error via the per-file call and SUCCEEDS via the program call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn program_getter_surfaces_non_root_imported_error_that_per_file_misses() {
    use verter_tsgo_api::TsgoClient;

    let Some(exe) = common::engine_or_skip() else {
        return;
    };

    let tmp = tempdir();
    let src = tmp.join("src");
    std::fs::create_dir_all(&src).expect("create src");

    // A real, ON-DISK, non-root imported module with a deliberate type error:
    // assigning a string to a `number`-typed export → TS2322. It is NOT written
    // to the overlay (it lives on disk) and is NOT a tsconfig root.
    std::fs::write(
        src.join("imported.ts"),
        "export const bad: number = \"not a number\";\nexport const ok: number = 1;\n",
    )
    .expect("write imported.ts");

    // The carrier (a tsconfig ROOT) imports the non-root module but is itself
    // clean — so a per-file check of the carrier finds nothing.
    let carrier = src.join("Carrier.tsx");
    let carrier_norm = common::norm(&carrier);
    let carrier_src = "import { ok } from \"./imported\";\nexport const x: number = ok;\n";

    // tsconfig lists ONLY the carrier in `files` (no `include`), so `imported.ts`
    // is a NON-ROOT transitive program member.
    let tsconfig = tmp.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        format!(
            r#"{{
  "compilerOptions": {{
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "skipLibCheck": true
  }},
  "files": ["{}"]
}}
"#,
            common::norm(&carrier)
        ),
    )
    .expect("write tsconfig");

    let snapshot = OverlaySnapshot::builder()
        .file(&carrier_norm, carrier_src)
        .directory(common::norm(&src))
        .real_dir_source(Arc::new(StdFsDirSource))
        .build();

    let client = TsgoClient::connect(&exe, &tmp, snapshot, 16).expect("connect");
    client.initialize().await.expect("initialize");
    let params = UpdateSnapshotParams {
        open_project: Some(common::norm(&tsconfig)),
        file_changes: None,
    };
    let snap = client
        .update_snapshot(&params)
        .await
        .expect("updateSnapshot");
    let project = snap
        .projects
        .iter()
        .find(|p| {
            common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
        })
        .expect("project opened");

    // (a) Per-file getter on the CARRIER: the imported file's error is NOT here.
    let per_file = client
        .get_semantic_diagnostics(&snap.snapshot, &project.id, &carrier_norm)
        .await
        .expect("per-file diagnostics");
    assert!(
        !per_file.iter().any(|d| d.code == 2322),
        "the per-file carrier check must NOT surface the non-root imported error \
         (that is exactly the rootscope gap): {per_file:?}"
    );

    // (b) Whole-program getter (file omitted): the non-root imported error IS here.
    let program = client
        .get_semantic_diagnostics_for_program(&snap.snapshot, &project.id)
        .await
        .expect("program diagnostics");
    let imported_error = program.iter().find(|d| d.code == 2322);
    assert!(
        imported_error.is_some(),
        "the whole-program getter MUST surface the non-root imported TS2322: {program:?}"
    );
    // The surfaced diagnostic is attributed to the imported file, not the carrier.
    let d = imported_error.unwrap();
    assert!(
        d.file_name
            .as_deref()
            .map(|f| f.replace('\\', "/").ends_with("src/imported.ts"))
            .unwrap_or(false),
        "the non-root diagnostic is homed on imported.ts: {:?}",
        d.file_name
    );

    // (c) CLEAN control: fix the imported file on disk, fresh snapshot, re-check.
    // The whole-program getter now surfaces NO TS2322.
    std::fs::write(
        src.join("imported.ts"),
        "export const bad: number = 0;\nexport const ok: number = 1;\n",
    )
    .expect("rewrite imported.ts clean");
    let clean_client = TsgoClient::connect(
        &exe,
        &tmp,
        OverlaySnapshot::builder()
            .file(&carrier_norm, carrier_src)
            .directory(common::norm(&src))
            .real_dir_source(Arc::new(StdFsDirSource))
            .build(),
        16,
    )
    .expect("connect clean");
    clean_client.initialize().await.expect("initialize clean");
    let snap_clean = clean_client
        .update_snapshot(&params)
        .await
        .expect("updateSnapshot clean");
    let project_clean = snap_clean
        .projects
        .iter()
        .find(|p| {
            common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
        })
        .expect("clean project opened");
    let clean_program = clean_client
        .get_semantic_diagnostics_for_program(&snap_clean.snapshot, &project_clean.id)
        .await
        .expect("clean program diagnostics");
    assert!(
        !clean_program.iter().any(|d| d.code == 2322),
        "a clean imported file yields no TS2322 from the whole-program getter: {clean_program:?}"
    );

    client.close().await.expect("close");
    clean_client.close().await.expect("close clean");
}

/// DISCRIMINATING (config/options): `getConfigFileParsingDiagnostics` surfaces a
/// compiler-options error (an invalid `target` → TS6046) that neither per-file
/// nor whole-program semantic/syntactic getters report; a clean config returns
/// none. This is the config-diagnostic proof at the API-client layer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_file_parsing_getter_surfaces_bad_target_and_none_on_clean() {
    use verter_tsgo_api::TsgoClient;

    let Some(exe) = common::engine_or_skip() else {
        return;
    };

    // Helper: open a project whose tsconfig has the given `target` value and
    // return its config-file-parsing diagnostics.
    async fn config_diags_for_target(exe: &std::path::Path, target: &str) -> Vec<Diagnostic> {
        let tmp = tempdir();
        let src = tmp.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        let carrier = src.join("Carrier.tsx");
        let carrier_norm = common::norm(&carrier);
        std::fs::write(&carrier, "export const x: number = 1;\n").expect("write carrier on disk");

        let tsconfig = tmp.join("tsconfig.json");
        std::fs::write(
            &tsconfig,
            format!(
                r#"{{
  "compilerOptions": {{
    "strict": true,
    "target": "{target}",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "skipLibCheck": true
  }},
  "files": ["{}"]
}}
"#,
                common::norm(&carrier)
            ),
        )
        .expect("write tsconfig");

        let snapshot = OverlaySnapshot::builder()
            .file(&carrier_norm, "export const x: number = 1;\n")
            .directory(common::norm(&src))
            .real_dir_source(Arc::new(StdFsDirSource))
            .build();

        let client = TsgoClient::connect(exe, &tmp, snapshot, 16).expect("connect");
        client.initialize().await.expect("initialize");
        let params = UpdateSnapshotParams {
            open_project: Some(common::norm(&tsconfig)),
            file_changes: None,
        };
        let snap = client
            .update_snapshot(&params)
            .await
            .expect("updateSnapshot");
        let project = snap
            .projects
            .iter()
            .find(|p| {
                common::norm(std::path::Path::new(&p.config_file_name)) == common::norm(&tsconfig)
            })
            .expect("project opened");
        let diags = client
            .get_config_file_parsing_diagnostics(&snap.snapshot, &project.id)
            .await
            .expect("config diagnostics");
        client.close().await.expect("close");
        diags
    }

    // (a) A bad `target` value → TS6046 (Argument for '--target' option must be ...).
    let bad = config_diags_for_target(&exe, "NotARealTarget").await;
    assert!(
        bad.iter().any(|d| d.code == 6046),
        "an invalid `target` must surface TS6046 via getConfigFileParsingDiagnostics: {bad:?}"
    );

    // (b) A valid `target` → NO TS6046 (the control that proves it is the bad
    // value, not the call, producing the diagnostic).
    let clean = config_diags_for_target(&exe, "ES2020").await;
    assert!(
        !clean.iter().any(|d| d.code == 6046),
        "a valid `target` yields no TS6046: {clean:?}"
    );
}

/// Create a unique temp directory (no external crate; uses pid + nanos).
fn tempdir() -> std::path::PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("verter_tsgo_api_it_{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}
