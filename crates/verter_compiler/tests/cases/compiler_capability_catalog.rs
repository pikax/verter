//! Discriminating tests for the typed compiler capability catalog.
//!
//! Covers compile-time presence (frontend-only, projection-only, runtime-
//! capable), immutable identity (duplicate construction + deterministic
//! iteration), and generic catalog independence from framework-private types.

use verter_compiler::framework_common::profile::FrameworkProfileManifest;
use verter_compiler::framework_common::{
    CarrierFrontend, CatalogCapability, CatalogTable, DuplicateCatalogIdentity, FrameworkEpoch,
    FrameworkHostIntegrationBackend, FrameworkSemanticAuthority, FrontendCap, HostCap, HostEpoch,
    HostEpochId, Present, ProjectionBackend, RuntimeCompilerBackend, TypedCapabilityRegistration,
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

    fn project_ide(
        &self,
        _: verter_compiler::framework_common::ProductExecutionGrant,
        _: &str,
        _: &(),
        _: &(),
        _: &(),
    ) -> Result<(), ()> {
        Ok(())
    }
}

struct TestEpoch;
impl FrameworkEpoch for TestEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "vue-sfc-v3",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct HtmlEpoch;
impl FrameworkEpoch for HtmlEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "html-v1",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct DtsEpoch;
impl FrameworkEpoch for DtsEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "dts-v1",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct EpochOne;
impl FrameworkEpoch for EpochOne {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "e1",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct EpochTwo;
impl FrameworkEpoch for EpochTwo {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "e2",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
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
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();
    type Output = ();

    fn compile_runtime(
        &self,
        _: verter_compiler::framework_common::ProductExecutionGrant,
        _: &str,
        _: &(),
        _: &(),
        _: &(),
    ) -> Result<(), ()> {
        Ok(())
    }
}

impl FrameworkHostIntegrationBackend<TestEpoch, SessionHostEpoch> for HostCapable {
    type CompileAdmission = ();
    type ParseArtifact = ();
    type MultiProductDemand = ();
    type RuntimeRenderDemand = ();
    type AdmissionRefusal = ();

    fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_canonical_request(
        &self,
        _artifact: &(),
        _request: verter_compiler::compile_request::CompileRequest,
    ) -> Result<(), ()> {
        Err(())
    }
}

impl FrameworkHostIntegrationBackend<TestEpoch, HostEpochA> for HostCapable {
    type CompileAdmission = ();
    type ParseArtifact = ();
    type MultiProductDemand = ();
    type RuntimeRenderDemand = ();
    type AdmissionRefusal = ();

    fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_canonical_request(
        &self,
        _artifact: &(),
        _request: verter_compiler::compile_request::CompileRequest,
    ) -> Result<(), ()> {
        Err(())
    }
}

impl FrameworkHostIntegrationBackend<TestEpoch, HostEpochB> for HostCapable {
    type CompileAdmission = ();
    type ParseArtifact = ();
    type MultiProductDemand = ();
    type RuntimeRenderDemand = ();
    type AdmissionRefusal = ();

    fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_canonical_request(
        &self,
        _artifact: &(),
        _request: verter_compiler::compile_request::CompileRequest,
    ) -> Result<(), ()> {
        Err(())
    }
}

fn frontend_row<E: FrameworkEpoch>(
    adapter: &str,
    language: &str,
) -> TypedCapabilityRegistration<FrontendCap<ToolingFrontend>> {
    TypedCapabilityRegistration::register_frontend::<E, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted")
}

fn host_row<HostE: HostEpoch>(
    adapter: &str,
    language: &str,
) -> TypedCapabilityRegistration<HostCap<HostCapable>>
where
    HostCapable: FrameworkHostIntegrationBackend<TestEpoch, HostE>,
{
    TypedCapabilityRegistration::register_host_integration::<TestEpoch, HostE, _>(
        FrameworkAdapterId::new(adapter),
        LanguageId::new(language),
        Present(HostCapable),
    )
    .expect("the test manifest is admitted")
}

#[test]
fn frontend_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted");
    assert_eq!(row.identity().capability(), CatalogCapability::Frontend);
    assert_eq!(
        row.identity().framework_family(),
        HtmlEpoch::MANIFEST.family
    );
    assert!(row.identity().host_epoch().is_none());
    let _ = row.frontend();
}

#[test]
fn projection_only_registration_does_not_require_runtime_backend() {
    let row = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    )
    .expect("the test manifest is admitted");
    assert_eq!(row.identity().capability(), CatalogCapability::Projection);
    assert_eq!(row.identity().framework_family(), DtsEpoch::MANIFEST.family);
    assert!(row.identity().host_epoch().is_none());
    let _ = row.projection();
}

#[test]
fn runtime_capable_registration_binds_a_real_runtime_backend() {
    let row = TypedCapabilityRegistration::register_runtime::<TestEpoch, _>(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        Present(RuntimeCapable),
    )
    .expect("the test manifest is admitted");
    assert_eq!(row.identity().capability(), CatalogCapability::Runtime);
    assert_eq!(
        row.identity().framework_family(),
        TestEpoch::MANIFEST.family
    );
    assert!(row.identity().host_epoch().is_none());
    let _ = row.runtime();
}

#[test]
fn semantic_and_host_registrations_are_typed_without_stubs() {
    let semantic = TypedCapabilityRegistration::register_semantic::<TestEpoch, _>(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        Present(SemanticCapable),
    )
    .expect("the test manifest is admitted");
    let host =
        TypedCapabilityRegistration::register_host_integration::<TestEpoch, SessionHostEpoch, _>(
            FrameworkAdapterId::new("host"),
            LanguageId::new("vue"),
            Present(HostCapable),
        )
        .expect("the test manifest is admitted");
    assert_eq!(
        semantic.identity().capability(),
        CatalogCapability::Semantic
    );
    assert_eq!(
        semantic.identity().framework_family(),
        TestEpoch::MANIFEST.family
    );
    assert!(semantic.identity().host_epoch().is_none());
    assert_eq!(
        host.identity().capability(),
        CatalogCapability::HostIntegration
    );
    assert_eq!(
        host.identity().framework_family(),
        TestEpoch::MANIFEST.family
    );
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
    )
    .expect("the test manifest is admitted");
    let projection = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    )
    .expect("the test manifest is admitted");
    let semantic = TypedCapabilityRegistration::register_semantic::<TestEpoch, _>(
        FrameworkAdapterId::new("sem"),
        LanguageId::new("vue"),
        Present(SemanticCapable),
    )
    .expect("the test manifest is admitted");
    let runtime = TypedCapabilityRegistration::register_runtime::<TestEpoch, _>(
        FrameworkAdapterId::new("rt"),
        LanguageId::new("vue"),
        Present(RuntimeCapable),
    )
    .expect("the test manifest is admitted");
    let host =
        TypedCapabilityRegistration::register_host_integration::<TestEpoch, SessionHostEpoch, _>(
            FrameworkAdapterId::new("host"),
            LanguageId::new("vue"),
            Present(HostCapable),
        )
        .expect("the test manifest is admitted");
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
        const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
            family: "local-frontend-epoch",
            release: "1",
            admitted_majors: &[1],
            claimed_carriers: &[],
        };
    }
    let row = TypedCapabilityRegistration::register_frontend::<LocalEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted");
    assert_eq!(
        row.identity().framework_family(),
        LocalEpoch::MANIFEST.family
    );
    let _ = row.frontend();
}

#[test]
fn register_projection_catalog_epoch_is_derived_from_the_epoch_type() {
    struct LocalEpoch;
    impl FrameworkEpoch for LocalEpoch {
        const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
            family: "local-projection-epoch",
            release: "1",
            admitted_majors: &[1],
            claimed_carriers: &[],
        };
    }
    let row = TypedCapabilityRegistration::register_projection::<LocalEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    )
    .expect("the test manifest is admitted");
    assert_eq!(
        row.identity().framework_family(),
        LocalEpoch::MANIFEST.family
    );
    let _ = row.projection();
}

#[test]
fn register_semantic_catalog_epoch_is_derived_from_the_epoch_type() {
    struct LocalEpoch;
    impl FrameworkEpoch for LocalEpoch {
        const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
            family: "local-epoch",
            release: "1",
            admitted_majors: &[1],
            claimed_carriers: &[],
        };
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
    )
    .expect("the test manifest is admitted");
    assert_eq!(
        row.identity().framework_family(),
        LocalEpoch::MANIFEST.family
    );
    let _ = row.semantic();
}

#[test]
fn duplicate_identities_fail_construction() {
    let err = CatalogTable::try_from_rows([
        frontend_row::<TestEpoch>("vue", "vue"),
        frontend_row::<TestEpoch>("vue", "vue"),
    ])
    .expect_err("duplicate adapter/epoch/capability must fail");
    assert!(matches!(err, DuplicateCatalogIdentity { .. }));
}

#[test]
fn duplicate_frontend_is_detected_across_languages_with_an_intervening_row() {
    let err = CatalogTable::try_from_rows([
        frontend_row::<TestEpoch>("vue", "a"),
        frontend_row::<TestEpoch>("other", "m"),
        frontend_row::<TestEpoch>("vue", "z"),
    ])
    .expect_err("same adapter/epoch/Frontend is duplicate regardless of language");
    assert_eq!(err.identity.capability(), CatalogCapability::Frontend);
    assert_eq!(err.identity.adapter_id().as_str(), "vue");
    assert_eq!(err.identity.framework_family(), "vue-sfc-v3");
}

#[test]
fn host_epoch_distinguishes_otherwise_equal_rows() {
    CatalogTable::try_from_rows([
        host_row::<HostEpochA>("vue", "vue"),
        host_row::<HostEpochB>("vue", "vue"),
    ])
    .expect("distinct host epochs must coexist");
}

#[test]
fn same_host_epoch_duplicate_fails_construction() {
    let err = CatalogTable::try_from_rows([
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
fn catalog_iteration_is_deterministic_regardless_of_insert_order() {
    let rows_a = [
        frontend_row::<EpochTwo>("b", "b"),
        frontend_row::<EpochOne>("a", "a"),
        frontend_row::<EpochTwo>("a", "a"),
    ];
    let rows_b = [
        frontend_row::<EpochTwo>("a", "a"),
        frontend_row::<EpochOne>("a", "a"),
        frontend_row::<EpochTwo>("b", "b"),
    ];
    let catalog_a = CatalogTable::try_from_rows(rows_a).unwrap();
    let catalog_b = CatalogTable::try_from_rows(rows_b).unwrap();
    let keys_a: Vec<_> = catalog_a
        .iter()
        .map(|row| {
            (
                row.identity().adapter_id().as_str().to_owned(),
                row.identity().framework_family().to_owned(),
                row.identity().capability(),
            )
        })
        .collect();
    let keys_b: Vec<_> = catalog_b
        .iter()
        .map(|row| {
            (
                row.identity().adapter_id().as_str().to_owned(),
                row.identity().framework_family().to_owned(),
                row.identity().capability(),
            )
        })
        .collect();
    assert_eq!(keys_a, keys_b);
    assert_eq!(keys_a.len(), 3);
    assert!(
        catalog_a.iter().map(|row| row.identity()).is_sorted(),
        "rows iterate in identity order, whatever order they were inserted in"
    );
}

struct VueThree;
impl FrameworkEpoch for VueThree {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "vue",
        release: "3",
        admitted_majors: &[3],
        claimed_carriers: &["vue"],
    };
}

struct VueThreeSix;
impl FrameworkEpoch for VueThreeSix {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "vue",
        release: "3.6",
        admitted_majors: &[3],
        claimed_carriers: &["vue"],
    };
}

#[test]
fn two_releases_of_one_family_are_two_catalog_rows_and_one_release_twice_is_a_duplicate() {
    let table = CatalogTable::try_from_rows([
        frontend_row::<VueThree>("vue", "vue"),
        frontend_row::<VueThreeSix>("vue", "vue"),
    ])
    .expect("two exact releases of one family are two rows");
    assert_eq!(table.len(), 2);
    let profiles: Vec<_> = table
        .iter()
        .map(|row| row.identity().framework_profile().clone())
        .collect();
    assert_ne!(profiles[0], profiles[1]);
    let err = CatalogTable::try_from_rows([
        frontend_row::<VueThree>("vue", "vue"),
        frontend_row::<VueThree>("vue", "vue"),
    ])
    .expect_err("one release registered twice is a duplicate");
    assert_eq!(err.identity.framework_family(), "vue");
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
fn frontend_row_grammar_fact_is_explicit_registration_not_default() {
    let bare = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted");
    // A frontend row without a registered grammar fact publishes none —
    // the consumer must fail closed, never substitute a framework default.
    assert!(bare.registered_grammar().is_none());
    let with_grammar =
        bare.with_registered_grammar(verter_compiler::svelte::svelte_grammar_config());
    assert_eq!(
        with_grammar.registered_grammar(),
        Some(&verter_compiler::svelte::svelte_grammar_config())
    );
}

#[test]
fn built_in_frontend_rows_publish_registered_grammar_facts() {
    use verter_compiler::framework_common::registered_carrier_projection::registered_grammar_for;
    let vue = registered_grammar_for(&FrameworkAdapterId::vue(), &LanguageId::new("vue"))
        .expect("Vue frontend row carries its registered grammar");
    assert_eq!(
        vue,
        &verter_compiler::framework_common::vue_grammar_config(
            "{{",
            "}}",
            std::iter::empty::<&str>()
        )
        .expect("default Vue grammar")
    );
    let svelte = registered_grammar_for(&FrameworkAdapterId::svelte(), &LanguageId::new("svelte"))
        .expect("Svelte frontend row carries its registered grammar");
    assert_eq!(svelte, &verter_compiler::svelte::svelte_grammar_config());
}

#[test]
fn registered_grammar_lookup_fails_closed_for_unregistered_identity() {
    use verter_compiler::framework_common::registered_carrier_projection::registered_grammar_for;
    // An unregistered carrier identity yields no grammar fact — never
    // another framework's grammar.
    assert!(registered_grammar_for(
        &FrameworkAdapterId::new("tooling"),
        &LanguageId::new("html"),
    )
    .is_none());
    // A registered adapter paired with a language it does not carry is
    // likewise a miss, not a fallback.
    assert!(registered_grammar_for(&FrameworkAdapterId::vue(), &LanguageId::new("html")).is_none());
}
