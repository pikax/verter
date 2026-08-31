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
use std::time::Duration;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;
use verter_lsp::tsgo::composite::TsgoCompositeProvider;
use verter_lsp::tsgo::ipc::{TsgoOwnedProvider, TsgoTypeProvider};
use verter_lsp::type_provider::traits::TypeProvider;
use verter_semantic::resolver_core::ConfiguredMembership;
use verter_session::{HostConfig, VerterHost};
use verter_tsgo_api::actor::spawn_actor;
use verter_tsgo_api::proto::types::{
    method, Diagnostic, InitializeResponse, UpdateSnapshotResponse,
};
use verter_tsgo_api::snapshot::{AccessibleEntries, OverlaySnapshot, RealDirSource};
use verter_tsgo_api::transport::pipe::StdioPipeTransport;
use verter_tsgo_api::{ClientHandle, RequestOptions};
use verter_workspace::tsgo_virtual_config::{
    augment_tsconfig_bytes, build_virtual_overlay_snapshot,
};
use verter_workspace::{
    canonical_path::CanonicalPath,
    config::{load_compiler_options, load_project_membership, load_project_references},
    published_state::PublishedRoot,
    snapshot_builder::{
        build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
    },
    workspace_snapshot::{OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration},
    FilesystemOptions, FilesystemWorkspace, WorkspaceAccess,
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
async fn engine_or_skip() -> Option<PathBuf> {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Lsp,
        Some(workspace_root()),
    );
    match verter_tsgo_api::toolchain::discovery::resolve(&request).await {
        Ok(resolution) => Some(resolution.path),
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
    let dir = verter_test_support::unique_temp_dir("verter_lsp_vcfg");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn file_uri(path: &str) -> String {
    if path.starts_with('/') {
        format!("file://{path}")
    } else {
        format!("file:///{path}")
    }
}

fn host_for_vue_specific_fixture(root: &Path, tsconfig: &Path) -> Arc<VerterHost> {
    let root = norm(root);
    let tsconfig = norm(tsconfig);
    let reader = FilesystemWorkspace::new(FilesystemOptions::default());
    let membership = load_project_membership(&reader, &tsconfig);
    let compiler_options = load_compiler_options(&reader, &tsconfig);
    let spec = membership_to_spec(
        &CanonicalPath::new(&root),
        &membership,
        &supported_extensions_for(&compiler_options),
    );
    let references = load_project_references(&reader, &tsconfig)
        .into_iter()
        .map(|path| CanonicalPath::new(&path))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: CanonicalPath::new(&root),
        workspace_root: CanonicalPath::new(&root),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(&tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    let snapshot = build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1));
    let workspace = Arc::new(FilesystemWorkspace::new(FilesystemOptions::default()));
    workspace.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(snapshot)));
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    host.set_workspace(Arc::clone(&workspace) as Arc<dyn WorkspaceAccess>);
    host
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
    let Some(exe) = engine_or_skip().await else {
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
            serde_json::json!({ "openProjects": [tsconfig_norm] }),
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
        serde_json::json!({ "openProjects": [tsconfig_norm] }),
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
        serde_json::json!({ "openProjects": [tsconfig_norm] }),
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

/// A real `*.vue` owner proves the composite admission without pretending the
/// generated `.vue.tsx` is already an attached-API configured root. User-facing
/// diagnostics remain on the established rich managed LSP route, which owns the
/// didOpen/didChange overlay. This is the regression for the API-only promotion
/// that returned an empty set and erased the real editor diagnostic.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn vue_only_owner_preserves_managed_lsp_mutation_diagnostics() {
    let Some(exe) = engine_or_skip().await else {
        return;
    };
    let tmp = tempdir();
    let (tsconfig, _src, source, companion) = write_vue_specific_fixture(&tmp);
    assert!(source.exists());
    assert!(
        !companion.exists(),
        "the generated companion stays off disk"
    );

    let root = norm(&tmp);
    let companion = norm(&companion);
    let root_uri = file_uri(&root);
    let host = host_for_vue_specific_fixture(&tmp, &tsconfig);
    let crash_notify = Arc::new(Notify::new());
    let tsgo_bin = exe.to_string_lossy().into_owned();
    let lsp = TsgoTypeProvider::spawn_with_crash_signal(
        &tsgo_bin,
        &root_uri,
        Some(Arc::clone(&crash_notify)),
    )
    .await
    .expect("spawn real tsgo --lsp");
    let owned = TsgoOwnedProvider::attach(Arc::new(lsp), &tsgo_bin)
        .await
        .expect("attach production owned checker");
    let resilient = verter_lsp::tsgo::resilient::new_owned(
        owned,
        crash_notify,
        tsgo_bin,
        root_uri,
        Arc::new(OnceCell::<Client>::new()),
        3,
    );
    let composite = TsgoCompositeProvider::new(Arc::new(resilient), host, None);

    let clean = "export const value: string = \"ok\";\n";
    let broken = "export const value: number = \"bad\";\n";
    composite
        .open_file(&companion, clean)
        .await
        .expect("didOpen");
    assert!(composite
        .get_diagnostics(&companion)
        .await
        .expect("clean diagnostics")
        .is_empty());

    composite
        .update_file(&companion, broken)
        .await
        .expect("didChange error");
    let diagnostics = composite
        .get_diagnostics(&companion)
        .await
        .expect("mutated diagnostics");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.as_deref() == Some("2322")),
        "the proven owner must admit the managed LSP mutation diagnostic: {diagnostics:?}"
    );

    composite
        .update_file(&companion, clean)
        .await
        .expect("didChange restore");
    assert!(composite
        .get_diagnostics(&companion)
        .await
        .expect("restored diagnostics")
        .is_empty());
    composite.shutdown().await.expect("shutdown");
    std::fs::remove_dir_all(&tmp).expect("remove isolated fixture");
}
