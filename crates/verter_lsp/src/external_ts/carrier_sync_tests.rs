//! Foundation tests for the sealed carrier-sync gateway.
//!
//! The full owned-publish / owner-loss-retract behaviour is covered through the real
//! production-path harness in the workspace-scanner and server tests; these unit
//! tests pin the gateway's local primitives: the receipt-gated commit and the
//! close-only target helper.

use super::*;
use crate::provider_sync::{ProviderOwnerBinding, ProviderSyncState};
use dashmap::DashMap;
use std::sync::Arc;

use verter_session::{HostConfig, VerterHost};
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

use crate::external_ts::tsserver_backend::TsserverEngineBackend;
use crate::external_ts::{
    default_carrier_store_host_version, CarrierCompanion, CarrierPublishCoordinator,
    CarrierPublishStore, ReconcileOutcome, ReconcileReason,
};
use crate::project_resolver::{IdeProjectConfig, NativeProjectResolver};
use crate::provider_surface_store::ProviderSurfaceStore;
use crate::type_provider::mock::MockTypeProvider;

fn owned_carrier_state() -> ProviderSyncState {
    ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
        decl_path: None,
        shadow_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_background_loaded: false,
    }
}

#[test]
fn commit_carrier_provider_state_requires_a_receipt_and_commits() {
    // The receipt-gated commit writes the carrier state into the shared map. The
    // receipt is the capability token; without it this call would not compile (the
    // structural half of the fusion — the gateway is the only production producer).
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let receipt = CarrierProviderCommit::for_test();
    let state = owned_carrier_state();

    commit_carrier_provider_state(&states, "/workspace/src/App.vue", state.clone(), &receipt);

    let committed = states
        .get("/workspace/src/App.vue")
        .expect("the receipt-gated commit must write the carrier state");
    assert_eq!(
        committed.owner_binding,
        ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
    );
    assert_eq!(
        committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert_eq!(
        committed.api_path.as_deref(),
        Some("/workspace/src/App.vue.verter.ts")
    );
}

#[test]
fn carrier_close_target_returns_owner_resolved_paths() {
    // The close-only path computes the carrier provider paths to close WITHOUT a
    // receipt (it is not a commit). An owner-resolved carrier yields its IDE + API
    // companion paths.
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let target = carrier_close_target(&resolver, "/workspace/src/App.vue", false, None)
        .expect("an owner-resolved carrier has provider paths to close");
    assert_eq!(
        target.owner_binding,
        ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        "the close target carries the resolved owner binding"
    );
    assert!(
        target.ide_path.is_some() && target.api_path.is_some(),
        "the close target carries both companion paths: {target:?}"
    );
}

/// A unique, already-canonical (lowercase drive, forward slashes) workspace root, so
/// the on-disk carrier store dir is isolated per run.
fn unique_ws_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("d:/verter_carrier_sync_compilefail_{nanos}_{n}/ws")
}

/// A `WorkspaceSnapshot` with ONE configured project owning `src/**/*` (so a `.vue`
/// under `src/` resolves to a `ProjectBinding`), built through the production
/// membership/expansion chain over an in-memory workspace.
fn project_binding_snapshot(ws_root: &str, tsconfig: &str) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        tsconfig.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    ws.inject_file(
        format!("{ws_root}/src/Comp.vue"),
        Arc::<str>::from("<template></template>"),
    );

    let root = CanonicalPath::new(ws_root);
    let raw_membership = load_project_membership(&ws, tsconfig);
    let compiler_options = load_compiler_options(&ws, tsconfig);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, tsconfig)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(ws_root),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// Whether `provider` is still in the project's `ready_files` set (the cross-process
/// advertised surface the plugin's `getExternalFiles` serves).
fn carrier_ready_in_store(ws_root: &str, tsconfig: &str, provider: &str) -> bool {
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), ws_root);
    let manifest = store.current_manifest();
    manifest
        .projects
        .get(tsconfig)
        .is_some_and(|project| project.ready_files.contains_key(provider))
}

/// An owned carrier that PREVIOUSLY published, then compiles to an EMPTY companion
/// set (neither an IDE surface nor a public-API artifact), must be RETRACTED from the
/// on-disk store — its stale `ready_files` row must DISAPPEAR so the plugin stops
/// advertising it. This drives the production gateway entry
/// (`reconcile_carrier_source`) for the genuinely-empty owned case (the
/// `ReconcileReason::CompileFailed` production constructor). RED before the fix: the
/// empty-companions branch returned `Pending` WITHOUT retracting, so the prior
/// advertisement lingered indefinitely.
#[tokio::test]
async fn owned_carrier_compiling_to_empty_companions_retracts_stale_advertisement() {
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord =
        CarrierPublishCoordinator::new(Arc::clone(&backend), Arc::new(mock.clone()), "5.9.0");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = VerterHost::new(HostConfig::default(), vfs);
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));

    // 1. Publish the carrier under its configured owner (a non-empty companion set)
    //    through the single membership entry — it enters the store's `ready_files`.
    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    };
    let published = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("the initial publish under a configured owner succeeds");
    assert!(
        matches!(published, ReconcileOutcome::Advertised { .. }),
        "the initial publish resolves to a configured owner ⇒ advertised, got {published:?}"
    );
    assert!(
        carrier_ready_in_store(&ws_root, &tsconfig, &provider),
        "the carrier must be advertised in the store's ready_files after the initial publish"
    );

    // 2. The source now compiles to NOTHING (no IDE surface, no public-API artifact)
    //    while it STILL has an authoritative owner. The resolver resolves the owner,
    //    but the host yields no compiled artifacts (it was never compiled), so the
    //    gateway builds an EMPTY companion set under authoritative ownership.
    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
        ws_root.clone(),
        ws_root.clone(),
        Some(tsconfig.clone()),
    )]);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let surfaces = ProviderSurfaceStore::new();
    let decision = reconcile_carrier_source(CarrierSyncRequest {
        host: &host,
        resolver: &resolver,
        provider_sync_states: &states,
        provider_surfaces: &surfaces,
        documents: None,
        canonical_id: &source,
        is_jsx: false,
        ide: None,
        membership: Some(CarrierMembershipCtx {
            coordinator: &coord,
            vfs: &fs,
            ownership_ready: true,
        }),
        reason: ReconcileReason::SourceSynced,
    })
    .await;
    assert!(
        matches!(decision, CarrierSyncDecision::Pending),
        "an owned compile-to-empty pass advertises nothing this pass (Pending), got a \
         different decision"
    );

    // 3. The stale advertisement MUST be gone from the store — the compile-to-empty
    //    owned case retracts the previously-published carrier.
    assert!(
        !carrier_ready_in_store(&ws_root, &tsconfig, &provider),
        "an owned carrier that compiled to EMPTY companions MUST be retracted from the \
         store's ready_files (so the plugin stops advertising it); the stale row lingered"
    );
}
