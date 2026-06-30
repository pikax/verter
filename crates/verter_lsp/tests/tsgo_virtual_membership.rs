//! REAL-engine proof of the one previously-unproven carrier-discovery case: a `.vue`
//! carrier's companion surface becomes a member of the configured tsgo Program
//! under a `.vue`-SPECIFIC `include` (`src/**/*.vue`) — which does NOT enumerate
//! the `.vue.tsx` companion — VIA tsconfig virtualization.
//!
//! This drives the PRODUCTION materialization path
//! (`verter_workspace::tsgo_virtual_config`) against a live tsgo `--api`
//! engine through the production `verter_tsgo_api` transport + actor.
//!
//! DISCRIMINATING. The test has two legs over the SAME fixture:
//!   * NEGATIVE — the real (un-virtualized) `*.vue`-only config: the companion
//!     is NOT a Program root file (proves virtualization is genuinely needed —
//!     the merged directory enumeration alone does not make a `.vue.tsx` a
//!     member when no include matches `.tsx`).
//!   * POSITIVE — the virtualized config (companion injected into `files` by
//!     `augment_tsconfig_bytes`, served through the overlay): the companion IS a
//!     root file, type-checks clean (no TS2307 → the import resolved through the
//!     REAL tsconfig), and a deliberate type error fires TS2345 (a genuine
//!     member under the real compiler options).
//!
//! Gating: runs NON-VACUOUSLY whenever the tsgo binary is present. Under
//! `VERTER_REQUIRE_TSGO` a missing engine is a hard failure (a skip would be a
//! vacuous pass); without it, a genuinely-absent engine hermetic-skips.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use verter_tsgo_api::actor::spawn_actor;
use verter_tsgo_api::proto::types::{
    method, Diagnostic, InitializeResponse, UpdateSnapshotResponse,
};
use verter_tsgo_api::snapshot::{AccessibleEntries, OverlaySnapshot, RealDirSource};
use verter_tsgo_api::transport::pipe::StdioPipeTransport;
use verter_tsgo_api::transport::spawn::discover_tsgo;
use verter_tsgo_api::{ClientHandle, RequestOptions};
use verter_workspace::tsgo_virtual_config::{
    augment_tsconfig_bytes, build_virtual_overlay_snapshot,
};

/// A real-dir source backed by `std::fs`, scoped to the fixture. This is the
/// role Verter's VFS fills in production; the test uses `std::fs` directly so
/// `getAccessibleEntries` returns the real on-disk files merged with the
/// overlay-injected companion.
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

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/")
        .to_path_buf()
}

/// Discover the engine, honoring `VERTER_REQUIRE_TSGO` (a skip under that env is
/// a vacuous-pass failure).
fn engine_or_skip() -> Option<PathBuf> {
    match discover_tsgo(&workspace_root()) {
        Ok(p) => Some(p),
        Err(e) => {
            if std::env::var("VERTER_REQUIRE_TSGO").is_ok() {
                panic!("VERTER_REQUIRE_TSGO is set but tsgo was not found: {e}. A skip would be a vacuous pass.");
            }
            eprintln!("[skip] tsgo engine not found ({e}); set VERTER_REQUIRE_TSGO to require it");
            None
        }
    }
}

fn norm(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn tempdir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("verter_lsp_vcfg_{}_{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

async fn req_json<T: serde::de::DeserializeOwned>(
    handle: &ClientHandle,
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
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("`{method}` decode failed: {e}"))
}

/// Write a hermetic fixture: a real `util.ts`, a `.vue`-SPECIFIC tsconfig (so
/// the companion is NOT enumerated), and the real carrier source on disk.
/// Returns `(tsconfig_path, src_dir, carrier_source, companion_path)`.
fn write_vue_specific_fixture(dir: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("create src");

    std::fs::write(
        src.join("util.ts"),
        "export function double(n: number): number {\n  return n * 2;\n}\n",
    )
    .expect("write util.ts");

    // The carrier source on disk (a `.vue` file). The include matches THIS, not
    // the generated `.vue.tsx` companion.
    let carrier = src.join("Widget.vue");
    std::fs::write(&carrier, "<template>{{ x }}</template>\n").expect("write carrier");

    // A `.vue`-SPECIFIC include — the non-enumerated discovery case.
    let tsconfig = dir.join("tsconfig.json");
    std::fs::write(
        &tsconfig,
        r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*.vue"]
}
"#,
    )
    .expect("write tsconfig");

    let companion = src.join("Widget.vue.tsx");
    (tsconfig, src, carrier, companion)
}

fn find_project<'a>(
    snap: &'a UpdateSnapshotResponse,
    tsconfig: &Path,
) -> &'a verter_tsgo_api::proto::types::ProjectResponse {
    snap.projects
        .iter()
        .find(|p| norm(Path::new(&p.config_file_name)) == norm(tsconfig))
        .expect("the opened project is in the snapshot")
}

fn carrier_is_root(
    project: &verter_tsgo_api::proto::types::ProjectResponse,
    companion: &str,
) -> bool {
    project
        .root_files
        .iter()
        .any(|f| norm(Path::new(f)) == companion)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vue_specific_include_companion_becomes_member_via_virtualization() {
    let Some(exe) = engine_or_skip() else {
        return;
    };

    let tmp = tempdir();
    let (tsconfig, src_dir, _carrier, companion) = write_vue_specific_fixture(&tmp);
    let companion_norm = norm(&companion);
    let tsconfig_norm = norm(&tsconfig);

    let companion_ok = "import { double } from \"./util\";\nexport const x: number = double(21);\n";

    // ── NEGATIVE LEG: the REAL `*.vue`-only config, NO virtualization. The
    //    companion is served as an overlay file so the engine CAN read it, but
    //    the un-augmented config's `include` never matches `.tsx`, so the
    //    companion is NOT a Program root file. ─────────────────────────────────
    {
        let snapshot = OverlaySnapshot::builder()
            .file(&companion_norm, companion_ok)
            .directory(norm(&src_dir))
            .real_dir_source(Arc::new(StdFsDirSource))
            .build();
        let transport = StdioPipeTransport::spawn(&exe, &tmp).expect("spawn tsgo (negative leg)");
        let handle = spawn_actor(transport, snapshot, 16);

        let _init: InitializeResponse =
            req_json(&handle, method::INITIALIZE, serde_json::Value::Null).await;
        let snap: UpdateSnapshotResponse = req_json(
            &handle,
            method::UPDATE_SNAPSHOT,
            serde_json::json!({ "openProject": tsconfig_norm }),
        )
        .await;
        let project = find_project(&snap, &tsconfig);
        assert!(
            !carrier_is_root(project, &companion_norm),
            "NEGATIVE: under the real `*.vue`-only config the `.vue.tsx` companion must NOT be a \
             Program root (virtualization is genuinely needed): {:?}",
            project.root_files
        );
        handle.close().await.expect("close negative leg");
    }

    // ── POSITIVE LEG: VIRTUALIZE through the production path. Inject the
    //    companion into `files` via `augment_tsconfig_bytes`, serve the
    //    augmented config + companion through the overlay built by
    //    `build_virtual_overlay_snapshot`. The companion becomes a member. ──────
    let user_tsconfig = std::fs::read_to_string(&tsconfig).expect("read user tsconfig");
    let augmented = augment_tsconfig_bytes(&user_tsconfig, std::slice::from_ref(&companion_norm));
    // SANITY: the augmentation actually injected the companion.
    assert!(
        augmented.contains(&companion_norm),
        "augmented config injects the companion path: {augmented}"
    );

    let snapshot = build_virtual_overlay_snapshot(
        &tsconfig_norm,
        &augmented,
        &[(companion_norm.clone(), companion_ok.to_string())],
        Arc::new(StdFsDirSource),
    );
    let transport = StdioPipeTransport::spawn(&exe, &tmp).expect("spawn tsgo (positive leg)");
    let handle = spawn_actor(transport, snapshot, 16);

    let init: InitializeResponse =
        req_json(&handle, method::INITIALIZE, serde_json::Value::Null).await;
    assert!(!init.current_directory.is_empty());

    let snap: UpdateSnapshotResponse = req_json(
        &handle,
        method::UPDATE_SNAPSHOT,
        serde_json::json!({ "openProject": tsconfig_norm }),
    )
    .await;
    let project = find_project(&snap, &tsconfig);

    // THE PROOF: the off-disk `.vue.tsx` companion is a Program ROOT FILE under
    // a `.vue`-specific include, achieved purely through virtualization.
    assert!(
        carrier_is_root(project, &companion_norm),
        "POSITIVE: the virtualized config makes the `.vue.tsx` companion a Program root file: {:?}",
        project.root_files
    );

    // It type-checks clean (no TS2307 → `./util` resolved through the REAL
    // tsconfig's compiler options, so the real config genuinely applies).
    let clean: Vec<Diagnostic> = req_json(
        &handle,
        method::GET_SEMANTIC_DIAGNOSTICS,
        serde_json::json!({ "snapshot": snap.snapshot, "project": project.id, "file": companion_norm }),
    )
    .await;
    assert!(
        !clean.iter().any(|d| d.code == 2307),
        "the companion's import resolved (no TS2307) ⇒ the real tsconfig applies: {clean:?}"
    );
    assert!(
        clean.is_empty(),
        "the clean companion type-checks with zero diagnostics: {clean:?}"
    );
    handle.close().await.expect("close positive-clean leg");

    // ── ERROR VARIANT: a deliberately broken companion fires TS2345 — proving
    //    the companion is a GENUINE member under the real compiler options, not
    //    a silently-ignored file. ────────────────────────────────────────────
    let companion_err =
        "import { double } from \"./util\";\nexport const x: number = double(\"nope\");\n";
    let augmented2 = augment_tsconfig_bytes(&user_tsconfig, std::slice::from_ref(&companion_norm));
    let err_snapshot = build_virtual_overlay_snapshot(
        &tsconfig_norm,
        &augmented2,
        &[(companion_norm.clone(), companion_err.to_string())],
        Arc::new(StdFsDirSource),
    );
    let transport2 = StdioPipeTransport::spawn(&exe, &tmp).expect("spawn tsgo (error leg)");
    let handle2 = spawn_actor(transport2, err_snapshot, 16);
    let _init2: InitializeResponse =
        req_json(&handle2, method::INITIALIZE, serde_json::Value::Null).await;
    let snap2: UpdateSnapshotResponse = req_json(
        &handle2,
        method::UPDATE_SNAPSHOT,
        serde_json::json!({ "openProject": tsconfig_norm }),
    )
    .await;
    let project2 = find_project(&snap2, &tsconfig);
    assert!(
        carrier_is_root(project2, &companion_norm),
        "the broken companion is still a member: {:?}",
        project2.root_files
    );
    let err: Vec<Diagnostic> = req_json(
        &handle2,
        method::GET_SEMANTIC_DIAGNOSTICS,
        serde_json::json!({ "snapshot": snap2.snapshot, "project": project2.id, "file": companion_norm }),
    )
    .await;
    assert!(
        err.iter().any(|d| d.code == 2345),
        "the deliberate type error fires TS2345 on the virtualized member: {err:?}"
    );
    // NEGATIVE: it is a TYPE error, not a false module-not-found.
    assert!(
        !err.iter().any(|d| d.code == 2307),
        "the error is a type error, not a false module-not-found: {err:?}"
    );
    handle2.close().await.expect("close error leg");
}
