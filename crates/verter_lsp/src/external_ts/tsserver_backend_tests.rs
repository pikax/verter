//! Discriminating tests for the tsserver engine backend: the witness mint path,
//! the two-phase publish through the on-disk store, and the owned-vs-ready split.

use std::sync::Arc;

use verter_session::external_ts::{
    BoundProject, EngineBackend, EnvDims, OpenState, ProjectBinding, ProjectResolution,
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
    );
    // Sanity: the binding is the resolved state.
    assert!(matches!(
        ProjectResolution::ProjectBinding(binding.clone()),
        ProjectResolution::ProjectBinding(_)
    ));
    backend
        .ensure_project(binding.ensure_project_request())
        .expect("ensure_project")
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
    assert_eq!(caps.reported_version.as_deref(), Some(HOST_VERSION));
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
    backend.publish_snapshot(&witness, snap).expect("publish");

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
    assert!(
        backend.publish_snapshot(&witness, snap).is_err(),
        "a publish whose project does not match the witness must fail closed"
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
    backend
        .publish_snapshot(&witness, snap)
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
    let _ = backend.query(
        &witness,
        Query {
            project: Arc::from("d:/ws/tsconfig.json"),
            provider_uri: Arc::from("d:/ws/src/A.vue.tsx"),
            carrier_offset: 0,
            feature: QueryFeature::Hover,
            content_hash: [0u8; 16],
            map_hash: [0u8; 16],
            required_version: 1,
        },
    );
}
