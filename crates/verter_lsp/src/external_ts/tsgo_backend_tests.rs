//! Discriminating tests for the OWNED tsgo engine backend: the witness mint path
//! and the dual-surface capability handshake.

use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineBackend, EnvDims, ProjectBinding,
};
use verter_session::file_artifact_store::ProjectIdentity;

use super::*;

const ENGINE_VERSION: &str = "7.0.2";

fn env_dims() -> EnvDims {
    EnvDims {
        parse_env_hash: [11u8; 16],
        resolve_env_hash: [22u8; 16],
        lib_env_hash: [33u8; 16],
        project_identity: ProjectIdentity([44u8; 16]),
    }
}

/// Mint a `BoundProject` THROUGH the contract witness chain: a resolved
/// `ProjectBinding` (via the test-util constructor) → `EnsureProject` →
/// `ensure_project`. The SAME chain production uses; the test never fabricates a
/// witness off-contract.
fn ensure(backend: &TsgoEngineBackend, workspace_root: &str, tsconfig_uri: &str) -> BoundProject {
    let binding = ProjectBinding::new_for_test(
        workspace_root,
        tsconfig_uri,
        ENGINE_VERSION,
        env_dims(),
        Vec::new(),
        verter_workspace::ProjectId(0),
        verter_workspace::SnapshotGeneration(1),
    );
    assert!(matches!(
        CarrierOwnershipResolution::Bound(binding.clone()),
        CarrierOwnershipResolution::Bound(_)
    ));
    backend
        .ensure_project(binding.ensure_project_request())
        .expect("ensure_project")
}

#[test]
fn ensure_project_mints_witness_bound_to_the_configured_project() {
    let backend = TsgoEngineBackend::new(ENGINE_VERSION);
    let witness = ensure(&backend, "file:///ws", "file:///ws/tsconfig.json");
    // The witness is bound to the requested configured project (the backend cannot
    // substitute a mismatched project — the URI is read from the request).
    assert_eq!(witness.project(), "file:///ws/tsconfig.json");
    // The witness carries the request's env dims.
    assert_eq!(witness.env_dims(), &env_dims());
}

#[test]
fn capability_handshake_records_api_wire_cancel_false_and_no_static_map() {
    let backend = TsgoEngineBackend::new(ENGINE_VERSION);
    let caps = backend.capabilities();
    // §2.8: the shipped --api exposes NO wire cancellation — `false` is the
    // EXPECTED recorded value (not a failure), and NO static module-resolution-map
    // endpoint.
    assert!(
        !caps.async_cancellable_queries,
        "the OWNED tsgo handshake must record api_wire_cancel = false (expected)"
    );
    assert!(
        !caps.static_module_resolution_map,
        "the shipped tsgo --api exposes no static module-resolution-map endpoint"
    );
    assert_eq!(
        caps.version.as_str(),
        Some(ENGINE_VERSION),
        "the record names the negotiated engine version"
    );
    assert_eq!(
        caps.version,
        verter_session::external_ts::EngineVersion::Declared(Arc::from(ENGINE_VERSION)),
        "a version this session negotiated is a DECLARATION here, not a handshake the \
         engine never made"
    );
}

#[test]
fn witness_capabilities_match_the_backend_handshake() {
    let backend = TsgoEngineBackend::new(ENGINE_VERSION);
    let witness = ensure(&backend, "file:///ws", "file:///ws/tsconfig.json");
    // The witness carries the SAME negotiated capabilities the backend reports.
    assert_eq!(
        witness.capabilities().version.as_str(),
        Some(ENGINE_VERSION)
    );
    assert!(!witness.capabilities().async_cancellable_queries);
}

#[test]
#[should_panic(expected = "answered by the live TsgoOwnedProvider")]
fn query_is_not_a_silent_stub() {
    let backend = TsgoEngineBackend::new(ENGINE_VERSION);
    let witness = ensure(&backend, "file:///ws", "file:///ws/tsconfig.json");
    // query() must fail LOUDLY (the live transport is the provider, wired
    // separately) — never a silent always-NoResult stub. Reaching it needs the
    // certified binding: the op no longer answers for a bare bound-project
    // witness.
    let serving = verter_session::external_ts::EngineIdentity::for_mode(
        verter_session::external_ts::ServeMode::Owned,
        &verter_session::external_ts::EngineSessionFacts {
            observed_version: std::sync::Arc::from(ENGINE_VERSION),
            wire_pin: 7,
            editor_session_generation: 3,
        },
    );
    let certified = verter_session::semantic_capability::CertifiedTypeEngineBinding::certify(
        &witness,
        &serving,
        verter_session::semantic_capability::ServingLease::new(1),
        verter_session::external_ts::PublishSnapshot {
            project: std::sync::Arc::from("file:///ws/tsconfig.json"),
            files: Vec::new(),
            resolution_map_version: 0,
            fs_generation: 0,
        }
        .input_basis(),
    )
    .expect("an observed handshake certifies");
    let _ = backend.query(
        &certified,
        verter_session::external_ts::Query {
            project: std::sync::Arc::from("file:///ws/tsconfig.json"),
            provider_uri: std::sync::Arc::from("file:///ws/src/A.vue.tsx"),
            carrier_offset: 0,
            feature: verter_session::external_ts::QueryFeature::Hover,
            content_hash: [0u8; 16],
            map_hash: [0u8; 16],
            required_version: 0,
        },
    );
}
