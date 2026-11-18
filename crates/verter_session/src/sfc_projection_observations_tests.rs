//! Publication fences exercised with resolver-produced engine certifications.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use verter_session_query::resolution::ProjectId;
use verter_span::CanonicalPath;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::snapshot_builder::{build_workspace_snapshot_simple, configured_project};
use verter_workspace::workspace_snapshot::SnapshotGeneration;

use crate::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineCapabilities, EngineIdentity, EngineVersion,
    EnvDims, ExternalTsProjectResolver, PublishSnapshot, ServeMode, WorkspaceProjectResolver,
};
use crate::file_artifact_store::ProjectIdentity;
use crate::semantic_capability::{CertifiedTypeEngineBinding, ServingLease};
use crate::sfc_projection_cache::{ProjectionRevisionSet, PublicSignatureRevision, Revision};
use crate::sfc_projection_mapper::MapperProjectHandle;
use crate::sfc_projection_observations::{
    projection_input_basis, CertifiedProjectionBinding, ProjectionPublicationAdmission,
    PublicationRefusal, SnapshotQualifiedObservation,
};

#[test]
fn the_input_basis_moves_with_the_config_chain_and_not_with_carrier_text() {
    use crate::sfc_projection_mapper::MapperProjectSettings;
    use verter_workspace::virtual_config::compute_virtual_config_identity;

    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec!["/workspace".into()],
        default_resolve_extensions: None,
    });
    let config = "/workspace/tsconfig.json";
    let base = "/workspace/base.json";
    ws.inject_file(
        config.into(),
        Arc::from(r#"{"extends":"./base.json","files":["Panel.vue"]}"#),
    );
    ws.inject_file(
        base.into(),
        Arc::from(r#"{"compilerOptions":{"strict":true}}"#),
    );
    ws.inject_file("/workspace/Panel.vue".into(), Arc::from("<template/>"));
    let paths = vec!["/workspace/Panel.vue.ts".to_string()];
    let settings =
        MapperProjectSettings::new(Arc::from(config), Vec::new(), Vec::new(), Vec::new());
    let basis = |config, paths: &[String], settings: &[u8; 32]| {
        projection_input_basis(
            config,
            compute_virtual_config_identity(&ws, config, paths).to_hash16(),
            settings,
        )
    };
    let initial = basis(config, &paths, &settings.identity);
    ws.inject_file(
        "/workspace/Panel.vue".into(),
        Arc::from("<template>{{changed}}</template>"),
    );
    assert_eq!(initial, basis(config, &paths, &settings.identity));
    ws.inject_file(
        base.into(),
        Arc::from(r#"{"compilerOptions":{"strict":false}}"#),
    );
    let changed_base = basis(config, &paths, &settings.identity);
    assert_ne!(initial, changed_base);
    ws.inject_file(
        config.into(),
        Arc::from(
            r#"{"extends":"./base.json","files":["Panel.vue"],"compilerOptions":{"noEmit":true}}"#,
        ),
    );
    assert_ne!(changed_base, basis(config, &paths, &settings.identity));
    let current = basis(config, &paths, &settings.identity);
    assert_ne!(current, basis(config, &[], &settings.identity));
    let changed_settings = MapperProjectSettings::new(
        Arc::from(config),
        vec!["custom-".into()],
        Vec::new(),
        Vec::new(),
    );
    assert_ne!(current, basis(config, &paths, &changed_settings.identity));
    let virtual_config = compute_virtual_config_identity(&ws, config, &paths).to_hash16();
    assert_ne!(
        current,
        projection_input_basis("/workspace/other.json", virtual_config, &settings.identity)
    );
}

fn binding(config: &str, generation: u64, lease: u64) -> CertifiedProjectionBinding {
    binding_on_basis(config, generation, lease, 1)
}

fn binding_on_basis(
    config: &str,
    generation: u64,
    lease: u64,
    fs_generation: u64,
) -> CertifiedProjectionBinding {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec!["/workspace".into()],
        default_resolve_extensions: None,
    });
    ws.inject_file(config.into(), Arc::from(r#"{"files":["Panel.vue"]}"#));
    ws.inject_file("/workspace/Panel.vue".into(), Arc::from("<template/>"));
    let snapshot = build_workspace_snapshot_simple(
        vec![configured_project(
            &ws,
            config,
            "/workspace",
            &CanonicalPath::new("/workspace"),
            ProjectId(0),
        )],
        SnapshotGeneration(lease),
    );
    // Test-only substitute for the host's orthogonal environment dimensions.
    let env_dims = |_: &str| EnvDims {
        parse_env_hash: [1; 16],
        resolve_env_hash: [2; 16],
        lib_env_hash: [3; 16],
        project_identity: ProjectIdentity([4; 16]),
    };
    let resolver = WorkspaceProjectResolver::new(&snapshot, &ws, "7.1.1", &env_dims, true);
    let CarrierOwnershipResolution::Bound(resolved) =
        resolver.resolve("/workspace/Panel.vue", None)
    else {
        panic!("configured carrier must resolve to a project");
    };
    let ensured = resolved.ensure_project_request();
    let bound = BoundProject::from_ensured(
        &ensured,
        EngineCapabilities {
            version: EngineVersion::Declared(Arc::from("7.1.1")),
            ..EngineCapabilities::default()
        },
    );
    let serving = EngineIdentity {
        mode: ServeMode::Owned,
        observed_version: Arc::from("7.1.1"),
        wire_pin: 1,
        editor_session_generation: 1,
    };
    let basis = PublishSnapshot {
        project: Arc::from(resolved.tsconfig_uri()),
        resolution_map_version: 1,
        fs_generation,
        files: Vec::new(),
    }
    .input_basis();
    CertifiedProjectionBinding {
        project: MapperProjectHandle::of_config(config),
        config_file: Arc::from(config),
        engine: CertifiedTypeEngineBinding::certify(
            &bound,
            &serving,
            ServingLease::new(lease),
            basis,
        )
        .expect("declared capabilities certify"),
        generation,
    }
}

fn revisions() -> ProjectionRevisionSet {
    ProjectionRevisionSet {
        authored_text: Revision([1; 32]),
        checking_text: Revision([2; 32]),
        correspondence: Revision([3; 32]),
        public_signature: PublicSignatureRevision([4; 32]),
        dependencies: Revision([5; 32]),
    }
}

fn observation(binding: &CertifiedProjectionBinding) -> SnapshotQualifiedObservation<&'static str> {
    SnapshotQualifiedObservation {
        project: binding.project.clone(),
        generation: binding.generation,
        carrier: Arc::from("/workspace/Panel.vue"),
        revisions: revisions(),
        value: "diagnostics",
    }
}

fn open(admission: &ProjectionPublicationAdmission, binding: &CertifiedProjectionBinding) {
    admission.bind(binding);
    admission.record_current(&binding.project, "/workspace/Panel.vue", revisions());
}

#[test]
fn project_bindings_isolate_the_same_carrier_across_configs() {
    let a = binding("/workspace/tsconfig.app.json", 1, 1);
    let b = binding("/workspace/tsconfig.server.json", 1, 1);
    assert_ne!(a.engine, b.engine);
    assert_ne!(a.project, b.project);
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &a);
    assert_eq!(admission.admit(observation(&a), false), Ok("diagnostics"));
    assert_eq!(
        admission.admit(observation(&b), false),
        Err(PublicationRefusal::UnknownProject)
    );
    admission.bind(&b);
    assert_eq!(
        admission.admit(observation(&b), false),
        Err(PublicationRefusal::StaleRevisions)
    );
    open(&admission, &b);
    admission.close(&a.project);
    assert_eq!(admission.admit(observation(&b), false), Ok("diagnostics"));
}

#[test]
fn every_projection_revision_channel_fences_publication() {
    let binding = binding("/workspace/tsconfig.json", 7, 1);
    let admission = ProjectionPublicationAdmission::default();
    open(&admission, &binding);
    for changed in [
        ProjectionRevisionSet {
            authored_text: Revision([9; 32]),
            ..revisions()
        },
        ProjectionRevisionSet {
            checking_text: Revision([9; 32]),
            ..revisions()
        },
        ProjectionRevisionSet {
            correspondence: Revision([9; 32]),
            ..revisions()
        },
        ProjectionRevisionSet {
            public_signature: PublicSignatureRevision([9; 32]),
            ..revisions()
        },
        ProjectionRevisionSet {
            dependencies: Revision([9; 32]),
            ..revisions()
        },
    ] {
        admission.record_current(&binding.project, "/workspace/Panel.vue", changed);
        assert_eq!(
            admission.admit(observation(&binding), false),
            Err(PublicationRefusal::StaleRevisions)
        );
        let mut current = observation(&binding);
        current.revisions = changed;
        assert_eq!(admission.admit(current, false), Ok("diagnostics"));
    }
}

#[test]
fn admission_restart_and_close_need_no_native_engine_or_host() {
    // The real certification above owns facts only. No native engine, host,
    // backend or callback is constructed or passed to this admission API.
    let binding = binding("/workspace/tsconfig.json", 7, 1);
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &binding);
    let mut current = observation(&binding);
    assert_eq!(admission.admit(current.clone(), false), Ok("diagnostics"));
    current.generation = admission.restart(&binding.project);
    assert!(current.generation > binding.generation);
    assert_eq!(
        admission.admit(observation(&binding), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    assert_eq!(
        admission.admit(current.clone(), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    let replacement = binding_on_basis("/workspace/tsconfig.json", current.generation, 1, 2);
    assert_ne!(binding.engine, replacement.engine);
    admission.bind(&replacement);
    // Rebinding at the returned generation preserves source revisions;
    // no record_current or second generation advance is necessary.
    assert_eq!(admission.admit(current.clone(), false), Ok("diagnostics"));
    admission.bind(&binding);
    assert_eq!(admission.admit(current.clone(), false), Ok("diagnostics"));
    admission.close(&binding.project);
    assert_eq!(
        admission.admit(current, false),
        Err(PublicationRefusal::UnknownProject)
    );
}

#[test]
fn cancellation_drops_the_payload_and_does_not_record_source_state() {
    let binding = binding("/workspace/tsconfig.json", 1, 1);
    let admission = ProjectionPublicationAdmission::new();
    admission.bind(&binding);
    let payload = Arc::new("cancelled diagnostics");
    let observed = observation(&binding);
    let cancelled = SnapshotQualifiedObservation {
        project: observed.project,
        generation: observed.generation,
        carrier: observed.carrier,
        revisions: observed.revisions,
        value: payload.clone(),
    };
    assert_eq!(
        admission.admit(cancelled, true),
        Err(PublicationRefusal::Cancelled)
    );
    assert_eq!(Arc::strong_count(&payload), 1);
    assert_eq!(
        admission.admit(observation(&binding), false),
        Err(PublicationRefusal::StaleRevisions)
    );
    open(&admission, &binding);
    assert_eq!(
        admission.admit(observation(&binding), false),
        Ok("diagnostics")
    );
}

#[test]
fn graph_reload_refuses_old_observations_and_stale_membership_leases() {
    let old = binding("/workspace/tsconfig.json", 1, 10);
    let current = binding("/workspace/tsconfig.json", 2, 11);
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &old);
    open(&admission, &current);
    assert_eq!(
        admission.admit(observation(&old), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    let mut stale_lease = old.clone();
    stale_lease.generation = 100;
    admission.bind(&stale_lease);
    assert_eq!(
        admission.admit(observation(&current), false),
        Ok("diagnostics")
    );
    assert_eq!(
        admission.admit(observation(&stale_lease), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    admission.close(&current.project);
    admission.bind(&stale_lease);
    assert_eq!(
        admission.admit(observation(&stale_lease), false),
        Err(PublicationRefusal::UnknownProject)
    );
}

#[test]
fn closing_and_reopening_never_revives_an_old_generation() {
    let old = binding("/workspace/tsconfig.json", 1, 1);
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &old);
    admission.close(&old.project);
    let mut reopened = old.clone();
    reopened.generation = 2;
    admission.bind(&reopened);
    assert_eq!(
        admission.admit(observation(&reopened), false),
        Err(PublicationRefusal::StaleRevisions)
    );
    open(&admission, &reopened);
    assert_eq!(
        admission.admit(observation(&old), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    assert_eq!(
        admission.admit(observation(&reopened), false),
        Ok("diagnostics")
    );
}

#[test]
fn a_certificate_cannot_be_relabelled_as_another_config() {
    let mut binding = binding("/workspace/tsconfig.json", 1, 1);
    binding.project = MapperProjectHandle::of_config("/workspace/other.json");
    binding.config_file = Arc::from("/workspace/other.json");
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &binding);
    assert_eq!(
        admission.admit(observation(&binding), false),
        Err(PublicationRefusal::UnknownProject)
    );
}

#[test]
fn changed_certification_cannot_reuse_a_generation() {
    let old = binding_on_basis("/workspace/tsconfig.json", 1, 1, 1);
    let mut replacement = binding_on_basis("/workspace/tsconfig.json", 1, 1, 2);
    assert_ne!(old.engine.input_basis(), replacement.engine.input_basis());
    let admission = ProjectionPublicationAdmission::new();
    open(&admission, &old);
    open(&admission, &replacement);
    assert_eq!(
        admission.admit(observation(&old), false),
        Err(PublicationRefusal::StaleGeneration)
    );
    replacement.generation = 2;
    open(&admission, &replacement);
    assert_eq!(
        admission.admit(observation(&replacement), false),
        Ok("diagnostics")
    );
}

#[test]
fn carrier_revisions_share_separator_normalization_and_require_an_open_project() {
    let binding = binding("/workspace/tsconfig.json", 1, 1);
    let admission = ProjectionPublicationAdmission::new();
    admission.record_current(&binding.project, "/workspace/Panel.vue", revisions());
    admission.bind(&binding);
    assert_eq!(
        admission.admit(observation(&binding), false),
        Err(PublicationRefusal::StaleRevisions)
    );
    admission.record_current(&binding.project, r"/workspace\Panel.vue", revisions());
    assert_eq!(
        admission.admit(observation(&binding), false),
        Ok("diagnostics")
    );
}

#[test]
fn project_results_need_a_live_certification_without_carrier_revisions() {
    let old = binding("/workspace/tsconfig.json", 1, 10);
    let admission = ProjectionPublicationAdmission::new();
    let admit = |generation| admission.admit_project(&old.project, generation, "options", false);
    assert_eq!(admit(1), Err(PublicationRefusal::UnknownProject));
    admission.bind(&old);
    assert_eq!(admit(1), Ok("options"));
    assert_eq!(
        admission.admit(observation(&old), false),
        Err(PublicationRefusal::StaleRevisions)
    );
    let payload = Arc::new("cancelled");
    assert_eq!(
        admission.admit_project(&old.project, 1, payload.clone(), true),
        Err(PublicationRefusal::Cancelled)
    );
    assert_eq!(Arc::strong_count(&payload), 1);
    let next = admission.restart(&old.project);
    assert_eq!(admit(1), Err(PublicationRefusal::StaleGeneration));
    assert_eq!(admit(next), Err(PublicationRefusal::StaleGeneration));
    let current = binding("/workspace/tsconfig.json", next, 11);
    admission.bind(&current);
    assert_eq!(admit(next), Ok("options"));
    let mut stale_lease = old.clone();
    stale_lease.generation = 100;
    admission.bind(&stale_lease);
    assert_eq!(admit(next), Ok("options"));
    assert_eq!(admit(100), Err(PublicationRefusal::StaleGeneration));
    admission.close(&old.project);
    admission.bind(&stale_lease);
    assert_eq!(admit(100), Err(PublicationRefusal::UnknownProject));
    admission.bind(&current);
    assert_eq!(admit(next), Err(PublicationRefusal::StaleGeneration));
    assert_eq!(admit(next + 1), Ok("options"));
    assert_eq!(
        admission.admit_project(
            &MapperProjectHandle::of_config("/workspace/other.json"),
            next + 1,
            "options",
            false,
        ),
        Err(PublicationRefusal::UnknownProject)
    );
}
