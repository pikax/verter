//! Discriminating tests for the typed compiler capability catalog.
//!
//! Covers compile-time presence (frontend-only, projection-only, runtime-
//! capable), immutable identity (duplicate construction + deterministic
//! iteration), and generic catalog independence from framework-private types.

use verter_compiler::framework_common::{
    CarrierFrontend, CatalogCapability, CatalogRow, DuplicateCatalogIdentity, FrameworkEpoch,
    FrameworkHostIntegrationBackend, FrameworkSemanticAuthority, HostEpoch, HostEpochId,
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
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();

    fn project_ide(&self, _: &str, _: &(), _: &(), _: &()) -> Result<(), ()> {
        Ok(())
    }
}

struct TestEpoch;
impl FrameworkEpoch for TestEpoch {
    const ID: &'static str = "vue-sfc-v3";
}

struct HtmlEpoch;
impl FrameworkEpoch for HtmlEpoch {
    const ID: &'static str = "html-v1";
}

struct DtsEpoch;
impl FrameworkEpoch for DtsEpoch {
    const ID: &'static str = "dts-v1";
}

struct EpochOne;
impl FrameworkEpoch for EpochOne {
    const ID: &'static str = "e1";
}

struct EpochTwo;
impl FrameworkEpoch for EpochTwo {
    const ID: &'static str = "e2";
}

struct SessionHostEpoch;
impl HostEpoch for SessionHostEpoch {
    const ID: &'static str = "session-v1";
}

struct HostEpochA;
impl HostEpoch for HostEpochA {
    const ID: &'static str = "host-a";
}

struct HostEpochB;
impl HostEpoch for HostEpochB {
    const ID: &'static str = "host-b";
}

impl FrameworkSemanticAuthority<TestEpoch> for SemanticCapable {
    type EvalSource = ();
    type TemplateFacts = ();
    type StyleMeaning = ();
    type SemanticAdmission = ();
    type ParseArtifact = ();

    fn eval_source(&self, _source: &str, _artifact: &()) {}
    fn template_facts(&self, _source: &str, _artifact: &()) {}
}

impl RuntimeCompilerBackend<TestEpoch> for RuntimeCapable {
    type RuntimeClient = ();
    type RuntimeServer = ();
}

impl FrameworkHostIntegrationBackend<TestEpoch, SessionHostEpoch> for HostCapable {
    type CompileAdmission = ();
}

impl FrameworkHostIntegrationBackend<TestEpoch, HostEpochA> for HostCapable {
    type CompileAdmission = ();
}

impl FrameworkHostIntegrationBackend<TestEpoch, HostEpochB> for HostCapable {
    type CompileAdmission = ();
}

type TestCatalogRow =
    CatalogRow<ToolingFrontend, ProjectionOnly, SemanticCapable, RuntimeCapable, HostCapable>;

fn frontend_row<E: FrameworkEpoch>(adapter: &str, language: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_frontend::<E, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(ToolingFrontend),
    )
    .into()
}

fn projection_row<E: FrameworkEpoch>(adapter: &str, language: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_projection::<E, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(ProjectionOnly),
    )
    .into()
}

fn runtime_row(adapter: &str, language: &str) -> TestCatalogRow {
    TypedCapabilityRegistration::register_runtime::<TestEpoch, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(RuntimeCapable),
    )
    .into()
}

fn host_row<HostE: HostEpoch>(adapter: &str, language: &str) -> TestCatalogRow
where
    HostCapable: FrameworkHostIntegrationBackend<TestEpoch, HostE>,
{
    TypedCapabilityRegistration::register_host_integration::<TestEpoch, HostE, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(HostCapable),
    )
    .into()
}

#[test]
fn frontend_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Frontend);
    assert_eq!(row.identity().epoch().as_str(), HtmlEpoch::ID);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.frontend();
}

#[test]
fn projection_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Projection);
    assert_eq!(row.identity().epoch().as_str(), DtsEpoch::ID);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.projection();
}

#[test]
fn runtime_capable_registration_binds_a_real_runtime_backend() {
    let row = TypedCapabilityRegistration::register_runtime::<TestEpoch, _>(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        Present(RuntimeCapable),
    );
    assert_eq!(row.identity().capability(), CatalogCapability::Runtime);
    assert_eq!(row.identity().epoch().as_str(), TestEpoch::ID);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.runtime();
}

#[test]
fn semantic_and_host_registrations_are_typed_without_stubs() {
    let semantic = TypedCapabilityRegistration::register_semantic::<TestEpoch, _>(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        Present(SemanticCapable),
    );
    let host =
        TypedCapabilityRegistration::register_host_integration::<TestEpoch, SessionHostEpoch, _>(
            FrameworkAdapterId::new("host"),
            LanguageId::new("vue"),
            Present(HostCapable),
        );
    assert_eq!(
        semantic.identity().capability(),
        CatalogCapability::Semantic
    );
    assert_eq!(semantic.identity().epoch().as_str(), TestEpoch::ID);
    assert!(semantic.identity().host_epoch().is_none());
    assert_eq!(
        host.identity().capability(),
        CatalogCapability::HostIntegration
    );
    assert_eq!(host.identity().epoch().as_str(), TestEpoch::ID);
    assert_eq!(
        host.identity().host_epoch().map(HostEpochId::as_str),
        Some(SessionHostEpoch::ID)
    );
    let _ = semantic.semantic();
    let _ = host.host_integration();
}

#[test]
fn host_epoch_is_present_only_on_host_integration_rows() {
    let frontend = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    );
    let projection = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    );
    let semantic = TypedCapabilityRegistration::register_semantic::<TestEpoch, _>(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        Present(SemanticCapable),
    );
    let runtime = TypedCapabilityRegistration::register_runtime::<TestEpoch, _>(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        Present(RuntimeCapable),
    );
    let host =
        TypedCapabilityRegistration::register_host_integration::<TestEpoch, SessionHostEpoch, _>(
            FrameworkAdapterId::new("host"),
            LanguageId::new("vue"),
            Present(HostCapable),
        );
    assert!(frontend.identity().host_epoch().is_none());
    assert!(projection.identity().host_epoch().is_none());
    assert!(semantic.identity().host_epoch().is_none());
    assert!(runtime.identity().host_epoch().is_none());
    assert!(host.identity().host_epoch().is_some());
}

#[test]
fn register_frontend_catalog_epoch_is_derived_from_the_epoch_type() {
    struct LocalEpoch;
    impl FrameworkEpoch for LocalEpoch {
        const ID: &'static str = "local-frontend-epoch";
    }
    let row = TypedCapabilityRegistration::register_frontend::<LocalEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    );
    assert_eq!(row.identity().epoch().as_str(), LocalEpoch::ID);
    let _ = row.frontend();
}

#[test]
fn register_projection_catalog_epoch_is_derived_from_the_epoch_type() {
    struct LocalEpoch;
    impl FrameworkEpoch for LocalEpoch {
        const ID: &'static str = "local-projection-epoch";
    }
    let row = TypedCapabilityRegistration::register_projection::<LocalEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    );
    assert_eq!(row.identity().epoch().as_str(), LocalEpoch::ID);
    let _ = row.projection();
}

#[test]
fn register_semantic_catalog_epoch_is_derived_from_the_epoch_type() {
    struct LocalEpoch;
    impl FrameworkEpoch for LocalEpoch {
        const ID: &'static str = "local-epoch";
    }
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
    let row = TypedCapabilityRegistration::register_semantic::<LocalEpoch, _>(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        Present(LocalSemantic),
    );
    assert_eq!(row.identity().epoch().as_str(), LocalEpoch::ID);
    let _ = row.semantic();
}

#[test]
fn duplicate_identities_fail_construction() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row::<TestEpoch>("vue", "vue"),
        frontend_row::<TestEpoch>("vue", "vue"),
    ])
    .expect_err("duplicate adapter/epoch/capability must fail");
    assert!(matches!(err, DuplicateCatalogIdentity { .. }));
}

#[test]
fn duplicate_frontend_is_detected_across_languages_with_intervening_projection() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row::<TestEpoch>("vue", "a"),
        projection_row::<TestEpoch>("vue", "m"),
        frontend_row::<TestEpoch>("vue", "z"),
    ])
    .expect_err("same adapter/epoch/Frontend is duplicate regardless of language");
    assert_eq!(err.identity.capability(), CatalogCapability::Frontend);
    assert_eq!(err.identity.adapter_id().as_str(), "vue");
    assert_eq!(err.identity.epoch().as_str(), "vue-sfc-v3");
}

#[test]
fn host_epoch_distinguishes_otherwise_equal_rows() {
    ImmutableCapabilityCatalog::try_from_rows([
        host_row::<HostEpochA>("vue", "vue"),
        host_row::<HostEpochB>("vue", "vue"),
    ])
    .expect("distinct host epochs must coexist");
}

#[test]
fn same_host_epoch_duplicate_fails_construction() {
    let err = ImmutableCapabilityCatalog::try_from_rows([
        host_row::<HostEpochA>("vue", "vue"),
        host_row::<HostEpochA>("vue", "vue"),
    ])
    .expect_err("same adapter/epoch/host-epoch/capability must fail");
    assert_eq!(
        err.identity.capability(),
        CatalogCapability::HostIntegration
    );
    assert_eq!(
        err.identity.host_epoch().map(HostEpochId::as_str),
        Some(HostEpochA::ID)
    );
}

#[test]
fn frozen_catalog_retains_typed_capability_payloads() {
    let catalog = ImmutableCapabilityCatalog::try_from_rows([
        frontend_row::<HtmlEpoch>("tooling", "html"),
        projection_row::<DtsEpoch>("api", "dts"),
        runtime_row("rt", "vue"),
        TypedCapabilityRegistration::register_semantic::<TestEpoch, _>(
            FrameworkAdapterId::new("sem"),
            LanguageId::new("vue"),
            Present(SemanticCapable),
        )
        .into(),
        host_row::<SessionHostEpoch>("host", "vue"),
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
        projection_row::<EpochTwo>("b", "b"),
        frontend_row::<EpochOne>("a", "a"),
        runtime_row("a", "a"),
    ];
    let rows_b = [
        runtime_row("a", "a"),
        frontend_row::<EpochOne>("a", "a"),
        projection_row::<EpochTwo>("b", "b"),
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
