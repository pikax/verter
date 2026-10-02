//! Discriminating tests for the tsserver engine backend: the witness mint path,
//! the two-phase publish through the on-disk store, and the owned-vs-ready split.

use std::sync::Arc;

use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineBackend, EnvDims, OpenState, ProjectBinding,
    PublishSnapshot, ScriptKind, SnapshotFile, SnapshotRole,
};
use verter_session::file_artifact_store::ProjectIdentity;

use super::*;
use crate::external_ts::carrier_publish_store::{ManifestRole, ManifestScriptKind};

const HOST_VERSION: &str = "test-host-9.9.9";

fn env_dims() -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: ProjectIdentity([4u8; 16]),
    }
}

/// Mint a `BoundProject` THROUGH the contract witness chain: a resolved
/// `ProjectBinding` (via the test-util constructor) → `EnsureProject` →
/// `ensure_project`. This is the SAME chain production uses; the test never
/// fabricates a witness off-contract.
fn ensure(
    backend: &TsserverEngineBackend,
    workspace_root: &str,
    tsconfig_uri: &str,
) -> BoundProject {
    let binding = ProjectBinding::new_for_test(
        workspace_root,
        tsconfig_uri,
        "7.0.1",
        env_dims(),
        Vec::new(),
        verter_workspace::ProjectId(0),
        verter_workspace::SnapshotGeneration(1),
    );
    // Sanity: the binding is the resolved state.
    assert!(matches!(
        CarrierOwnershipResolution::Bound(binding.clone()),
        CarrierOwnershipResolution::Bound(_)
    ));
    backend
        .ensure_project(binding.ensure_project_request())
        .expect("ensure_project")
}

/// Certify the way the publish coordinator does: over the backend's OWN observed
/// serving identity and the snapshot's own basis. The snapshot is taken by value
/// so the caller certifies the exact snapshot it then publishes — a
/// certify-here/publish-there pair could otherwise drift apart unnoticed.
fn certified_for(
    backend: &TsserverEngineBackend,
    bound: &BoundProject,
    snap: &PublishSnapshot,
) -> CertifiedTypeEngineBinding {
    CertifiedTypeEngineBinding::certify(bound, &backend.serving_identity(), snap.input_basis())
        .expect("an observed handshake certifies")
}

fn h16(s: &str) -> [u8; 16] {
    let d = blake3::hash(s.as_bytes());
    let mut out = [0u8; 16];
    out.copy_from_slice(&d.as_bytes()[..16]);
    out
}

fn file(provider: &str, source: &str, content: &str, v: u64) -> SnapshotFile {
    SnapshotFile {
        source_uri: Arc::from(source),
        provider_uri: Arc::from(provider),
        role: SnapshotRole::CarrierIde,
        script_kind: ScriptKind::Tsx,
        content: Arc::from(content),
        content_hash: h16(content),
        map_hash: [0u8; 16],
        map_json: None,
        structure: None,
        version: v,
        open_state: OpenState::Closed,
    }
}

#[test]
fn ensure_project_mints_a_witness_bound_to_the_request() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");
    assert_eq!(witness.project(), "d:/ws/tsconfig.json");
    assert_eq!(witness.env_dims(), &env_dims());
}

#[test]
fn capabilities_report_tsserver_shape() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let caps = backend.capabilities();
    // The shipped tsserver plugin model: synchronous, no static resolution map.
    assert!(!caps.static_module_resolution_map);
    assert!(!caps.async_cancellable_queries);
    // The publish backend handshakes no engine: the host version is this
    // publisher's own carrier-store segment, recorded as a DECLARATION so it can
    // never be read as an engine report.
    assert_eq!(caps.version.as_str(), Some(HOST_VERSION));
    assert_eq!(
        caps.version,
        EngineVersion::Declared(Arc::from(HOST_VERSION))
    );
}

#[test]
fn publish_snapshot_runs_two_phase_publish_through_the_store() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");

    let snap = PublishSnapshot {
        project: Arc::from("d:/ws/tsconfig.json"),
        files: vec![
            file(
                "d:/ws/src/A.vue.tsx",
                "d:/ws/src/A.vue",
                "export const A = 1;",
                3,
            ),
            file(
                "d:/ws/src/B.vue.tsx",
                "d:/ws/src/B.vue",
                "export const B = 2;",
                3,
            ),
        ],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let certified = certified_for(&backend, &witness, &snap);
    backend.publish_snapshot(&certified, snap).expect("publish");

    // The store the backend opened for this workspace knows the project + ready set.
    let store = CarrierPublishStore::open(HOST_VERSION, &ws);
    let manifest = store.current_manifest();
    let project = manifest
        .projects
        .get("d:/ws/tsconfig.json")
        .expect("project entry");
    assert_eq!(project.ready_files.len(), 2);
    // Two-phase: every ready entry's blob exists.
    for ready in project.ready_files.values() {
        assert!(store.workspace_dir().join(&ready.blob_rel).exists());
    }
}

/// The refusal has to be proven where the WARM is: the store write. A
/// certify-here/publish-there pair over a superseded basis, asserting both the
/// error and the manifest the store did NOT change.
#[test]
fn a_superseded_snapshot_never_reaches_the_store_write() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");

    // Certified over the FIRST publish, published with the SECOND: same project,
    // same companion, new carrier bytes — the basis the certification named is
    // no longer the one being published.
    let certified_over = PublishSnapshot {
        project: Arc::from("d:/ws/tsconfig.json"),
        files: vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            "export const A = 1;",
            3,
        )],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let superseded = PublishSnapshot {
        files: vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            "export const A = 2;",
            3,
        )],
        ..certified_over.clone()
    };
    let certified = certified_for(&backend, &witness, &certified_over);

    let error = backend
        .publish_snapshot(&certified, superseded)
        .expect_err("a superseded basis is refused before the store write");
    assert!(
        format!("{error:?}").contains("does not admit"),
        "the refusal names the basis, not an unrelated failure: {error:?}"
    );

    let store = CarrierPublishStore::open(HOST_VERSION, &ws);
    let manifest = store.current_manifest();
    let project = manifest.projects.get("d:/ws/tsconfig.json");
    assert!(
        project.is_none_or(|entry| entry.ready_files.is_empty()),
        "a refused publish advertises nothing: {project:?}"
    );
    assert!(
        !store.workspace_dir().join("blobs").exists()
            || store
                .workspace_dir()
                .join("blobs")
                .read_dir()
                .is_ok_and(|mut d| d.next().is_none()),
        "a refused publish writes no blob"
    );
}

/// The serving session rotates independently of any snapshot change, so an
/// unchanged snapshot under a rotated session is a different publish — refused
/// at the same place, and equally absent from the manifest.
#[test]
fn a_rotated_serving_session_never_reaches_the_store_write() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");

    let snap = PublishSnapshot {
        project: Arc::from("d:/ws/tsconfig.json"),
        files: vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            "export const A = 1;",
            3,
        )],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let certified = certified_for(&backend, &witness, &snap);
    // The membership ledger rotates its session generation (the transition every
    // ownership move drives); the snapshot itself does not move with it.
    backend.membership_ledger().advance_session();

    let error = backend
        .publish_snapshot(&certified, snap.clone())
        .expect_err("a rotated serving session is refused before the store write");
    assert!(
        format!("{error:?}").contains("serving session rotated"),
        "the refusal names the rotated session: {error:?}"
    );

    let store = CarrierPublishStore::open(HOST_VERSION, &ws);
    let manifest = store.current_manifest();
    let project = manifest.projects.get("d:/ws/tsconfig.json");
    assert!(
        project.is_none_or(|entry| entry.ready_files.is_empty()),
        "a refused publish advertises nothing: {project:?}"
    );

    // Re-certifying under the NEW session publishes normally: the rule refuses
    // the stale witness, not the route.
    let recertified = certified_for(&backend, &witness, &snap);
    backend
        .publish_snapshot(&recertified, snap)
        .expect("the same snapshot publishes under the session it was re-certified for");
    let after = store.current_manifest();
    assert_eq!(
        after
            .projects
            .get("d:/ws/tsconfig.json")
            .expect("project entry")
            .ready_files
            .len(),
        1,
        "the re-certified publish warms"
    );
}

#[test]
fn publish_for_an_unensured_project_fails_closed() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");

    // A snapshot whose project URI does NOT match the witness is refused.
    let snap = PublishSnapshot {
        project: Arc::from("d:/other/tsconfig.json"),
        files: vec![file("x.vue.tsx", "x.vue", "x", 1)],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let certified = certified_for(&backend, &witness, &snap);
    assert!(
        backend.publish_snapshot(&certified, snap).is_err(),
        "a publish whose project does not match the certified project must fail closed"
    );
}

#[test]
fn register_owned_then_publish_content_is_the_owned_vs_ready_split() {
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");

    // Register the owned set (no content) first.
    backend
        .register_owned(
            &witness,
            vec![OwnedSource {
                source_uri: "d:/ws/src/A.vue".to_string(),
                provider_uri: "d:/ws/src/A.vue.tsx".to_string(),
                role: ManifestRole::CarrierIde,
                script_kind: ManifestScriptKind::Tsx,
            }],
        )
        .expect("register owned");

    let store = CarrierPublishStore::open(HOST_VERSION, &ws);
    let m1 = store.current_manifest();
    let p1 = m1.projects.get("d:/ws/tsconfig.json").expect("project");
    assert_eq!(p1.owned_sources.len(), 1, "owned set registered");
    assert!(p1.ready_files.is_empty(), "NOT ready before content");

    // Publish the content → ready.
    let snap = PublishSnapshot {
        project: Arc::from("d:/ws/tsconfig.json"),
        files: vec![file(
            "d:/ws/src/A.vue.tsx",
            "d:/ws/src/A.vue",
            "export const A = 1;",
            5,
        )],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let certified = certified_for(&backend, &witness, &snap);
    backend
        .publish_snapshot(&certified, snap)
        .expect("publish content");
    let m2 = store.current_manifest();
    let p2 = m2.projects.get("d:/ws/tsconfig.json").expect("project");
    assert!(
        p2.ready_files.contains_key("d:/ws/src/A.vue.tsx"),
        "now ready"
    );
}

#[test]
#[should_panic(expected = "live tsserver transport")]
fn query_is_unimplemented_until_live_transport_wired() {
    // The query path fails LOUDLY (unimplemented!) rather than returning a forbidden
    // always-empty nop — the Stub Prevention rule. The live transport is wired
    // separately from this on-disk publish authority.
    let backend = TsserverEngineBackend::new(HOST_VERSION);
    let user_tree = tempfile::tempdir().expect("tempdir");
    let ws = user_tree.path().to_string_lossy().to_string();
    let witness = ensure(&backend, &ws, "d:/ws/tsconfig.json");
    use verter_session::external_ts::{Query, QueryFeature};
    let snap = PublishSnapshot {
        project: Arc::from("d:/ws/tsconfig.json"),
        files: vec![file("d:/ws/src/A.vue.tsx", "d:/ws/src/A.vue", "x", 1)],
        resolution_map_version: 1,
        fs_generation: 1,
    };
    let query = Query {
        project: Arc::from("d:/ws/tsconfig.json"),
        provider_uri: Arc::from("d:/ws/src/A.vue.tsx"),
        carrier_offset: 0,
        feature: QueryFeature::Hover,
        content_hash: [0u8; 16],
        map_hash: [0u8; 16],
        required_version: 1,
    };
    let certified = certified_for(&backend, &witness, &snap);
    let _ = backend.query(&certified, query);
}
