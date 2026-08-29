//! Discriminating tests for the typed compiler capability catalog.
//!
//! Covers compile-time presence (frontend-only, projection-only, runtime-
//! capable), immutable identity (duplicate construction + deterministic
//! iteration), generic catalog independence from framework-private types,
//! and that no production request route consults the catalog yet.

use verter_compiler::framework_common::{
    CarrierFrontend, CatalogCapability, CatalogRow, DuplicateCatalogIdentity, FrameworkEpochId,
    FrameworkHostIntegrationBackend, FrameworkSemanticAuthority, HostEpochId,
    ImmutableCapabilityCatalog, Present, ProjectionBackend, RuntimeCompilerBackend,
    TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ToolingFrontend;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProjectionOnly;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeCapable;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SemanticCapable;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HostCapable;

impl CarrierFrontend for ToolingFrontend {
    type ParseArtifact = ();
    type SyntaxReject = ();
    type ParseAdmission = ();

    fn parse(&self, _source: &str, _opts: &verter_language::ParseOptions) -> Result<(), ()> {
        Ok(())
    }
}

impl ProjectionBackend for ProjectionOnly {
    type IdeCompanion = ();
    type PublicApi = ();
    type Declarations = ();
}

impl FrameworkSemanticAuthority<FrameworkEpochId> for SemanticCapable {
    type EvalSource = ();
    type TemplateFacts = ();
    type StyleMeaning = ();
    type SemanticAdmission = ();
    type ParseArtifact = ();

    fn eval_source(&self, _source: &str, _artifact: &()) {}
    fn template_facts(&self, _source: &str, _artifact: &()) {}
}

impl RuntimeCompilerBackend<FrameworkEpochId> for RuntimeCapable {
    type RuntimeClient = ();
    type RuntimeServer = ();
}

impl FrameworkHostIntegrationBackend<FrameworkEpochId, HostEpochId> for HostCapable {
    type CompileAdmission = ();
}

type TestCatalogRow =
    CatalogRow<ToolingFrontend, ProjectionOnly, SemanticCapable, RuntimeCapable, HostCapable>;

fn frontend_row(adapter: &str, language: &str, epoch: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_frontend(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        FrameworkEpochId::new(epoch),
        Present(ToolingFrontend),
    )
    .into()
}

fn projection_row(adapter: &str, language: &str, epoch: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_projection(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        FrameworkEpochId::new(epoch),
        Present(ProjectionOnly),
    )
    .into()
}

fn runtime_row(adapter: &str, language: &str, epoch: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_runtime(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        FrameworkEpochId::new(epoch),
        Present(RuntimeCapable),
    )
    .into()
}

fn host_row(adapter: &str, language: &str, epoch: &str, host: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_host_integration(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        FrameworkEpochId::new(epoch),
        HostEpochId::new(host),
        Present(HostCapable),
    )
    .into()
}

#[test]
fn frontend_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_frontend(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        FrameworkEpochId::new("html-v1"),
        Present(ToolingFrontend),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Frontend);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.frontend();
}

#[test]
fn projection_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_projection(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        FrameworkEpochId::new("dts-v1"),
        Present(ProjectionOnly),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Projection);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.projection();
}

#[test]
fn runtime_capable_registration_binds_a_real_runtime_backend() {
    let row = TypedCapabilityRegistration::register_runtime(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(RuntimeCapable),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Runtime);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.runtime();
}

#[test]
fn semantic_and_host_registrations_are_typed_without_stubs() {
    let semantic = TypedCapabilityRegistration::register_semantic(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(SemanticCapable),
    );
    let host = TypedCapabilityRegistration::register_host_integration(
        FrameworkAdapterId::new("host"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        HostEpochId::new("session-v1"),
        Present(HostCapable),
    );
    assert_eq!(
        semantic.identity().capability(),
        CatalogCapability::Semantic
    );
    assert!(semantic.identity().host_epoch().is_none());
    assert_eq!(
        host.identity().capability(),
        CatalogCapability::HostIntegration
    );
    assert_eq!(
        host.identity().host_epoch().map(HostEpochId::as_str),
        Some("session-v1")
    );
    let _ = semantic.semantic();
    let _ = host.host_integration();
}

#[test]
fn host_epoch_is_present_only_on_host_integration_rows() {
    let frontend = TypedCapabilityRegistration::register_frontend(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        FrameworkEpochId::new("html-v1"),
        Present(ToolingFrontend),
    );
    let projection = TypedCapabilityRegistration::register_projection(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        FrameworkEpochId::new("dts-v1"),
        Present(ProjectionOnly),
    );
    let semantic = TypedCapabilityRegistration::register_semantic(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(SemanticCapable),
    );
    let runtime = TypedCapabilityRegistration::register_runtime(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(RuntimeCapable),
    );
    let host = TypedCapabilityRegistration::register_host_integration(
        FrameworkAdapterId::new("host"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        HostEpochId::new("session-v1"),
        Present(HostCapable),
    );
    assert!(frontend.identity().host_epoch().is_none());
    assert!(projection.identity().host_epoch().is_none());
    assert!(semantic.identity().host_epoch().is_none());
    assert!(runtime.identity().host_epoch().is_none());
    assert!(host.identity().host_epoch().is_some());
}

#[test]
fn register_semantic_accepts_authority_generic_over_a_local_epoch_type() {
    struct LocalEpoch;
    struct LocalSemantic;
    impl FrameworkSemanticAuthority<LocalEpoch> for LocalSemantic {
        type EvalSource = ();
        type TemplateFacts = ();
        type StyleMeaning = ();
        type SemanticAdmission = ();
        type ParseArtifact = ();

        fn eval_source(&self, _source: &str, _artifact: &()) {}
        fn template_facts(&self, _source: &str, _artifact: &()) {}
    }
    let row = TypedCapabilityRegistration::register_semantic(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(LocalSemantic),
    );
    assert_eq!(row.identity().epoch().as_str(), "vue-sfc-v3");
    let _ = row.semantic();
}

#[test]
fn duplicate_identities_fail_construction() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row("vue", "vue", "vue-sfc-v3"),
        frontend_row("vue", "vue", "vue-sfc-v3"),
    ])
    .expect_err("duplicate adapter/epoch/capability must fail");
    assert!(matches!(err, DuplicateCatalogIdentity { .. }));
}

#[test]
fn duplicate_frontend_is_detected_across_languages_with_intervening_projection() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row("vue", "a", "vue-sfc-v3"),
        projection_row("vue", "m", "vue-sfc-v3"),
        frontend_row("vue", "z", "vue-sfc-v3"),
    ])
    .expect_err("same adapter/epoch/Frontend is duplicate regardless of language");
    assert_eq!(err.identity.capability(), CatalogCapability::Frontend);
    assert_eq!(err.identity.adapter_id().as_str(), "vue");
    assert_eq!(err.identity.epoch().as_str(), "vue-sfc-v3");
}

#[test]
fn host_epoch_distinguishes_otherwise_equal_rows() {
    ImmutableCapabilityCatalog::try_from_rows([
        host_row("vue", "vue", "vue-sfc-v3", "host-a"),
        host_row("vue", "vue", "vue-sfc-v3", "host-b"),
    ])
    .expect("distinct host epochs must coexist");
}

#[test]
fn same_host_epoch_duplicate_fails_construction() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        host_row("vue", "vue", "vue-sfc-v3", "host-a"),
        host_row("vue", "vue", "vue-sfc-v3", "host-a"),
    ])
    .expect_err("same adapter/epoch/host-epoch/capability must fail");
    assert_eq!(
        err.identity.capability(),
        CatalogCapability::HostIntegration
    );
    assert_eq!(
        err.identity.host_epoch().map(HostEpochId::as_str),
        Some("host-a")
    );
}

#[test]
fn frozen_catalog_retains_typed_capability_payloads() {
    let catalog = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row("tooling", "html", "html-v1"),
        projection_row("api", "dts", "dts-v1"),
        runtime_row("rt", "vue", "vue-sfc-v3"),
        TypedCapabilityRegistration::register_semantic(
            FrameworkAdapterId::new("sem"),
            LanguageId::new("vue"),
            FrameworkEpochId::new("vue-sfc-v3"),
            Present(SemanticCapable),
        )
        .into(),
        host_row("host", "vue", "vue-sfc-v3", "session-v1"),
    ])
    .expect("distinct capabilities must coexist");
    let mut saw_frontend = false;
    let mut saw_projection = false;
    let mut saw_semantic = false;
    let mut saw_runtime = false;
    let mut saw_host = false;
    for row in catalog.iter() {
        match row {
            CatalogRow::Frontend(reg) => {
                saw_frontend = true;
                let _ = reg.frontend();
            }
            CatalogRow::Projection(reg) => {
                saw_projection = true;
                let _ = reg.projection();
            }
            CatalogRow::Semantic(reg) => {
                saw_semantic = true;
                let _ = reg.semantic();
            }
            CatalogRow::Runtime(reg) => {
                saw_runtime = true;
                let _ = reg.runtime();
            }
            CatalogRow::HostIntegration(reg) => {
                saw_host = true;
                let _ = reg.host_integration();
            }
        }
        assert!(
            row.frontend().is_some()
                || row.projection().is_some()
                || row.semantic().is_some()
                || row.runtime().is_some()
                || row.host_integration().is_some(),
            "frozen row must retain a typed payload, not identity-only"
        );
    }
    assert!(saw_frontend && saw_projection && saw_semantic && saw_runtime && saw_host);
}

#[test]
fn catalog_iteration_is_deterministic_regardless_of_insert_order() {
    let rows_a = [
        projection_row("b", "b", "e2"),
        frontend_row("a", "a", "e1"),
        runtime_row("a", "a", "e2"),
    ];
    let rows_b = [
        runtime_row("a", "a", "e2"),
        frontend_row("a", "a", "e1"),
        projection_row("b", "b", "e2"),
    ];
    let catalog_a = ImmutableCapabilityCatalog::try_from_rows(rows_a).unwrap();
    let catalog_b = ImmutableCapabilityCatalog::try_from_rows(rows_b).unwrap();
    let keys_a: Vec<_> = catalog_a
        .iter()
        .map(|row| {
            (
                row.identity().adapter_id().as_str().to_owned(),
                row.identity().epoch().as_str().to_owned(),
                row.identity().capability(),
            )
        })
        .collect();
    let keys_b: Vec<_> = catalog_b
        .iter()
        .map(|row| {
            (
                row.identity().adapter_id().as_str().to_owned(),
                row.identity().epoch().as_str().to_owned(),
                row.identity().capability(),
            )
        })
        .collect();
    assert_eq!(keys_a, keys_b);
    assert_eq!(keys_a[0].0, "a");
    assert_eq!(keys_a[0].2, CatalogCapability::Frontend);
    assert_eq!(keys_a[1].2, CatalogCapability::Runtime);
    assert_eq!(keys_a[2].0, "b");
}

#[test]
fn generic_catalog_module_source_does_not_name_framework_or_session_owners() {
    let source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/framework_common/catalog.rs"
    ));
    let traits = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/framework_common/capability.rs"
    ));
    for blob in [source, traits] {
        assert!(
            !blob.contains("vue_bridge"),
            "generic catalog must not import Vue bridge types"
        );
        assert!(
            !blob.contains("crate::svelte"),
            "generic catalog must not import Svelte private types"
        );
        assert!(
            !blob.contains("verter_session"),
            "generic catalog must not import host/session owners"
        );
        assert!(
            !blob.contains("CompileArtifactSet"),
            "generic catalog must not name compile-artifact-set types"
        );
    }
}

#[test]
fn production_request_routes_do_not_consult_the_catalog() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut hits = Vec::new();
    walk_rs(&src_root, &mut hits);
    assert!(
        hits.is_empty(),
        "production request routes must not consult the new catalog yet: {hits:?}"
    );
}

fn walk_rs(dir: &std::path::Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("src walk") {
        let entry = entry.expect("dirent");
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, hits);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let rel = path
            .strip_prefix(env!("CARGO_MANIFEST_DIR"))
            .unwrap_or(&path);
        let rel_str = rel.to_string_lossy();
        if rel_str.ends_with("framework_common/catalog.rs")
            || rel_str.ends_with("framework_common/capability.rs")
            || rel_str.ends_with("framework_common/mod.rs")
            || rel_str.ends_with("framework_common/vue_carrier_frontend.rs")
            || rel_str.ends_with("framework_common/vue_semantic_authority.rs")
            || rel_str.ends_with("framework_common/registered_carrier_projection.rs")
            || rel_str.ends_with("svelte/carrier_frontend.rs")
            || rel_str.ends_with("lib.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read rust");
        if text.contains("ImmutableCapabilityCatalog")
            || text.contains("TypedCapabilityRegistration")
        {
            hits.push(rel_str.into_owned());
        }
    }
}
