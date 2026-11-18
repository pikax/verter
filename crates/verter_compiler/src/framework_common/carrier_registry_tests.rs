//! The carrier registries over a catalog snapshot: snapshot construction,
//! typed backend access, native-host selection, absence of a compiler
//! backend as a capability status, and registry dispatch counts.
//!
//! Stub carriers are test-only rows in a test-built snapshot; production
//! registers only the Vue and Svelte providers.

use std::sync::Arc;

use verter_language::carrier_grammar::AcceptedRegisteredCarrierSource;
use verter_language::{
    CarrierBlockInventory, CarrierParse, FileLanguage, FrameworkAdapterId,
    InventoryValidationError, LanguageId, ParseOptions, SyntaxReject,
    UnregisteredFrameworkParseArtifact,
};

use super::capability::{
    CarrierFrontend, FrameworkEpoch, FrameworkHostIntegrationBackend, HostEpoch, NativeHostEpoch,
    Present, ProductExecutionGrant, ProjectionBackend, RuntimeCompilerBackend,
};
use super::carrier_compiler::CompileUnsupported;
use super::carrier_registry::{
    take_registry_dispatches, CarrierCompilerBackendRegistry, HostIntegrationMiss,
    RegisteredBackend, RegisteredCarrierFrontend, RegistryDispatches,
};
use super::catalog::{
    CatalogCapability, CatalogRows, CatalogSnapshot, CatalogSnapshotError, ProductCapability,
    TypedCapabilityRegistration,
};
use super::profile::{carrier_profile, framework_profile, FrameworkProfileManifest};
use super::registered_carrier_projection::{
    parse_registered_source_for_tests, template_facts_from_catalog, InstalledIdeCompanion,
    InstalledProjectionBackend, ProjectionCatalogInputs, TemplateFactsBasis,
};
use super::svelte_host_integration::{
    svelte_host_integration_registration, SvelteHostIntegrationBackend,
};
use super::vue_carrier_frontend::{
    vue_carrier_frontend_registration, VueCarrierFrontend, VueSfcV3,
};
use super::vue_host_integration::{
    vue_catalog_rows, vue_host_integration_registration, VueHostIntegrationBackend,
};
use super::vue_runtime_backend::{vue_runtime_backend_registration, VueRuntimeBackend};
use super::FrameworkParseArtifact;
use crate::compile_request::{CompileRequest, ProductKind};
use crate::svelte::{svelte_carrier_frontend_registration, SvelteRuntimeBackend, SvelteSfc5};
use verter_identity::successor::{CarrierProfileId, FrameworkProfileId, ReleaseDescriptorError};

// ── Test-only carriers ──────────────────────────────────────────────

struct HtmlEpoch;
impl FrameworkEpoch for HtmlEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "html",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct AstroEpoch;
impl FrameworkEpoch for AstroEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "astro",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

struct VuePreviewEpoch;
impl FrameworkEpoch for VuePreviewEpoch {
    const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
        family: "vue",
        release: "3.6",
        admitted_majors: &[3],
        claimed_carriers: &["vue"],
    };
}

struct OtherHostEpoch;
impl HostEpoch for OtherHostEpoch {
    const ID: &'static str = "other-host";
}

/// A frontend the snapshot registers but no test ever parses through.
struct StubFrontend;

impl CarrierFrontend for StubFrontend {
    type ParseArtifact = ();
    type SyntaxReject = ();
    type ParseAdmission = ();

    fn parse(&self, _source: &str, _opts: &ParseOptions) -> Result<(), ()> {
        unreachable!("the stub frontend is never parsed")
    }
}

impl RegisteredCarrierFrontend for StubFrontend {
    fn parse(
        &self,
        _source: &str,
        _opts: &ParseOptions,
    ) -> Result<Arc<UnregisteredFrameworkParseArtifact>, SyntaxReject> {
        unreachable!("the stub frontend is never parsed")
    }

    fn project_geometry(
        &self,
        _accepted: &AcceptedRegisteredCarrierSource,
        _artifact: &Arc<UnregisteredFrameworkParseArtifact>,
    ) -> Result<(CarrierBlockInventory, Arc<dyn CarrierParse>), InventoryValidationError> {
        unreachable!("the stub frontend is never projected")
    }
}

/// The projection registration type of the Astro stub.
struct StubProjection;

impl ProjectionBackend for StubProjection {
    type IdeCompanion = ();
    type PublicApi = ();
    type Declarations = ();
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();

    fn project_ide(
        &self,
        _grant: ProductExecutionGrant,
        _source: &str,
        _artifact: &(),
        _request: &(),
        _inputs: &(),
    ) -> Result<(), ()> {
        unreachable!("the stub projection is never executed")
    }
}

fn astro_project_ide(
    _grant: ProductExecutionGrant,
    _source: &str,
    _artifact: &FrameworkParseArtifact,
    _request: &CompileRequest,
    _inputs: &ProjectionCatalogInputs,
) -> Result<InstalledIdeCompanion, CompileUnsupported> {
    unreachable!("the stub projection is never executed")
}

/// HTML: a frontend and nothing else.
fn html_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    rows.carrier(TypedCapabilityRegistration::register_frontend::<
        HtmlEpoch,
        _,
    >(
        FrameworkAdapterId::new("html"),
        LanguageId::new("html"),
        Present(StubFrontend),
    )?);
    Ok(())
}

/// Astro: a frontend and a projection backend, no compiler backend.
fn astro_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    rows.carrier(TypedCapabilityRegistration::register_frontend::<
        AstroEpoch,
        _,
    >(
        FrameworkAdapterId::new("astro"),
        LanguageId::new("astro"),
        Present(StubFrontend),
    )?);
    rows.projection(
        TypedCapabilityRegistration::register_projection::<AstroEpoch, _>(
            FrameworkAdapterId::new("astro"),
            LanguageId::new("astro"),
            Present(StubProjection),
        )?
        .map_projection(|_| InstalledProjectionBackend::new(astro_project_ide)),
    );
    Ok(())
}

/// The runtime backend of a second Vue framework epoch.
struct PreviewRuntime;

impl RegisteredBackend for PreviewRuntime {
    fn registered() -> &'static Self {
        &PreviewRuntime
    }
}

impl RuntimeCompilerBackend<VuePreviewEpoch> for PreviewRuntime {
    type RuntimeClient = ();
    type RuntimeServer = ();
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();
    type Output = ();

    fn compile_runtime(
        &self,
        _grant: ProductExecutionGrant,
        _source: &str,
        _artifact: &(),
        _request: &(),
        _inputs: &(),
    ) -> Result<(), ()> {
        unreachable!("the preview runtime is never executed")
    }
}

/// The native-host backend of a second Vue framework epoch.
struct PreviewHost;

impl RegisteredBackend for PreviewHost {
    fn registered() -> &'static Self {
        &PreviewHost
    }
}

impl FrameworkHostIntegrationBackend<VuePreviewEpoch, NativeHostEpoch> for PreviewHost {
    type CompileAdmission = ();
    type ParseArtifact = ();
    type MultiProductDemand = ();
    type RuntimeRenderDemand = ();
    type AdmissionRefusal = ();

    fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        unreachable!("the preview host is never admitted")
    }

    fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        unreachable!("the preview host is never admitted")
    }

    fn admit_canonical_request(&self, _artifact: &(), _request: CompileRequest) -> Result<(), ()> {
        unreachable!("the preview host is never admitted")
    }
}

/// A second Vue framework epoch: its own frontend row and compiler backend.
fn vue_preview_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    let vue = FrameworkAdapterId::vue();
    let language = VueCarrierFrontend.carrier_language_id();
    rows.carrier(TypedCapabilityRegistration::register_frontend::<
        VuePreviewEpoch,
        _,
    >(
        vue.clone(),
        language.clone(),
        Present(VueCarrierFrontend),
    )?);
    rows.compiler_backend(
        TypedCapabilityRegistration::register_runtime::<VuePreviewEpoch, _>(
            vue.clone(),
            language.clone(),
            Present(PreviewRuntime),
        )?,
        TypedCapabilityRegistration::register_host_integration::<
            VuePreviewEpoch,
            NativeHostEpoch,
            _,
        >(vue, language, Present(PreviewHost))?,
    );
    Ok(())
}

/// The Vue compiler backend with no Vue frontend registered.
fn vue_backend_without_frontend_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    rows.compiler_backend(
        vue_runtime_backend_registration(),
        vue_host_integration_registration(),
    );
    Ok(())
}

/// Both frontends, and one compiler backend whose halves name Vue and
/// Svelte.
fn crossed_backend_halves_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    rows.carrier(vue_carrier_frontend_registration());
    rows.carrier(svelte_carrier_frontend_registration());
    rows.compiler_backend(
        vue_runtime_backend_registration(),
        svelte_host_integration_registration(),
    );
    Ok(())
}

/// The carrier and framework profile an epoch type registers a carrier
/// language under.
fn keys<E: FrameworkEpoch>(carrier_language: &str) -> (CarrierProfileId, FrameworkProfileId) {
    (
        carrier_profile(&LanguageId::new(carrier_language)),
        framework_profile(&E::MANIFEST).expect("the manifest is admitted"),
    )
}

/// A provider declaring a ranged release, which the release law rejects.
fn rejected_release_rows(rows: &mut CatalogRows) -> Result<(), ReleaseDescriptorError> {
    struct RangedEpoch;
    impl FrameworkEpoch for RangedEpoch {
        const MANIFEST: FrameworkProfileManifest = FrameworkProfileManifest {
            family: "ranged",
            release: "^1",
            admitted_majors: &[1],
            claimed_carriers: &["ranged"],
        };
    }
    rows.carrier(TypedCapabilityRegistration::register_frontend::<
        RangedEpoch,
        _,
    >(
        FrameworkAdapterId::new("ranged"),
        LanguageId::new("ranged"),
        Present(StubFrontend),
    )?);
    Ok(())
}

// ── Snapshot construction ───────────────────────────────────────────

#[test]
fn a_compiler_backend_without_its_carrier_frontend_fails_snapshot_construction() {
    let err = CatalogSnapshot::compose(&[vue_backend_without_frontend_rows])
        .expect_err("a backend whose carrier has no frontend must not compose");
    let CatalogSnapshotError::BackendWithoutFrontend(identity) = err else {
        panic!("expected the missing-frontend refusal, got {err:?}");
    };
    assert_eq!(*identity, *vue_runtime_backend_registration().identity());

    // A frontend registered under another framework epoch is not this
    // carrier's frontend: nothing would ever parse an artifact the backend's
    // epoch names.
    let err = CatalogSnapshot::compose(&[vue_backend_without_frontend_rows, html_rows])
        .expect_err("another carrier's frontend does not satisfy the backend");
    assert!(matches!(
        err,
        CatalogSnapshotError::BackendWithoutFrontend(_)
    ));

    CatalogSnapshot::compose(&[vue_catalog_rows]).expect("the Vue provider alone composes");
}

#[test]
fn backend_halves_naming_different_carriers_fail_snapshot_construction() {
    let err = CatalogSnapshot::compose(&[crossed_backend_halves_rows])
        .expect_err("a backend whose halves name two carriers must not compose");
    assert_eq!(
        err,
        CatalogSnapshotError::BackendHalvesDisagree {
            runtime: Box::new(vue_runtime_backend_registration().identity().clone()),
            native_host: Box::new(svelte_host_integration_registration().identity().clone()),
        }
    );
}

#[test]
fn a_second_provider_claiming_a_registered_identity_fails_snapshot_construction() {
    let err = CatalogSnapshot::compose(&[vue_catalog_rows, vue_catalog_rows])
        .expect_err("two providers registering one identity must not compose");
    assert!(
        matches!(err, CatalogSnapshotError::Duplicate(_)),
        "a duplicate identity is a typed composition refusal, got {err:?}"
    );
}

// ── Typed access and selection ──────────────────────────────────────

#[test]
fn a_typed_backend_lookup_refuses_a_row_registered_for_another_backend_type() {
    let registry = CarrierCompilerBackendRegistry::built_in();
    let (carrier, profile) = keys::<VueSfcV3>("vue");
    let vue = registry
        .row_for(&carrier, &profile)
        .expect("the Vue carrier registers a compiler backend");
    assert!(vue
        .runtime::<VueRuntimeBackend>()
        .is_some_and(|backend| std::ptr::eq(backend, VueRuntimeBackend::registered())));
    assert!(vue
        .native_host::<VueHostIntegrationBackend>()
        .is_some_and(|backend| std::ptr::eq(
            backend,
            <VueHostIntegrationBackend as RegisteredBackend>::registered()
        )));
    assert!(
        vue.runtime::<SvelteRuntimeBackend>().is_none(),
        "the Vue row never hands out the Svelte runtime backend"
    );
    assert!(
        vue.native_host::<SvelteHostIntegrationBackend>().is_none(),
        "the Vue row never hands out the Svelte host backend"
    );
    assert!(
        registry
            .runtime::<SvelteRuntimeBackend>(&carrier, &profile)
            .is_none(),
        "a typed lookup through the registry refuses the foreign type too"
    );
}

#[test]
fn host_integration_selection_reports_the_first_missing_axis() {
    let registry = CarrierCompilerBackendRegistry::built_in();
    let (vue_carrier, vue_profile) = keys::<VueSfcV3>("vue");
    let (_, svelte_profile) = keys::<SvelteSfc5>("svelte");
    let unknown_carrier = carrier_profile(&LanguageId::new("angular"));
    let miss = |result: Result<_, HostIntegrationMiss>| result.map(|_| ()).unwrap_err();
    assert_eq!(
        miss(registry.host_integration_for::<NativeHostEpoch>(&unknown_carrier, &vue_profile)),
        HostIntegrationMiss::UnregisteredAdapter
    );
    assert_eq!(
        miss(registry.host_integration_for::<OtherHostEpoch>(&vue_carrier, &vue_profile)),
        HostIntegrationMiss::HostEpoch
    );
    // Host epoch AND framework profile both miss: the host epoch is reported.
    assert_eq!(
        miss(registry.host_integration_for::<OtherHostEpoch>(&vue_carrier, &svelte_profile)),
        HostIntegrationMiss::HostEpoch
    );
    assert_eq!(
        miss(registry.host_integration_for::<NativeHostEpoch>(&vue_carrier, &svelte_profile)),
        HostIntegrationMiss::FrameworkEpoch
    );
    let row = registry
        .host_integration_for::<NativeHostEpoch>(&vue_carrier, &vue_profile)
        .expect("the exact native Vue row is selected");
    assert_eq!(row.host_identity().framework_profile(), &vue_profile);
}

#[test]
fn two_installed_releases_select_the_row_the_artifact_profile_names() {
    let snapshot = CatalogSnapshot::compose(&[vue_catalog_rows, vue_preview_rows])
        .expect("two Vue releases compose");
    let registry = snapshot.compiler_backends();
    let (carrier, stable_profile) = keys::<VueSfcV3>("vue");
    let (_, preview_profile) = keys::<VuePreviewEpoch>("vue");
    assert_ne!(stable_profile, preview_profile);
    for profile in [&stable_profile, &preview_profile] {
        let row = registry
            .host_integration_for::<NativeHostEpoch>(&carrier, profile)
            .expect("an exact-profile match selects its row");
        assert_eq!(
            row.host_identity().framework_profile(),
            profile,
            "the selected row is the exact requested framework profile"
        );
    }
    let stable = registry
        .host_integration_for::<NativeHostEpoch>(&carrier, &stable_profile)
        .expect("the stable Vue row");
    assert!(stable.native_host::<VueHostIntegrationBackend>().is_some());
    assert!(stable.native_host::<PreviewHost>().is_none());
    let preview = registry
        .host_integration_for::<NativeHostEpoch>(&carrier, &preview_profile)
        .expect("the preview Vue row");
    assert!(preview.native_host::<PreviewHost>().is_some());
    assert!(preview.runtime::<PreviewRuntime>().is_some());
    assert!(
        preview.native_host::<VueHostIntegrationBackend>().is_none(),
        "the preview release's row hands out only its own backend"
    );
}

#[test]
fn a_rejected_release_declaration_fails_registration_typed() {
    let err = CatalogSnapshot::compose(&[vue_catalog_rows, rejected_release_rows])
        .expect_err("a ranged release declaration must not compose");
    assert_eq!(
        err,
        CatalogSnapshotError::RejectedManifest(ReleaseDescriptorError::Range)
    );
}

// ── Absence of a compiler backend (PC06) ────────────────────────────

/// A carrier with no compiler backend answers a runtime demand with the
/// UNSUPPORTED capability status for that product only: the registry never
/// falls back to another carrier's backend (Vue's is installed beside it),
/// and the Astro carrier's IDE companion is still served by its projection
/// row. Each product enters its own table once.
#[test]
fn a_backendless_carrier_reports_runtime_unsupported_and_still_projects_its_ide_companion() {
    let snapshot = CatalogSnapshot::compose(&[vue_catalog_rows, html_rows, astro_rows])
        .expect("the stub carriers compose beside Vue");
    let (html_carrier, html_profile) = keys::<HtmlEpoch>("html");
    let (astro_carrier, astro_profile) = keys::<AstroEpoch>("astro");

    // HTML runtime demand.
    let _ = take_registry_dispatches();
    assert_eq!(
        snapshot.product_capability(&html_carrier, &html_profile, ProductKind::RuntimeClient),
        ProductCapability::Unsupported(CatalogCapability::Runtime)
    );
    assert_eq!(
        take_registry_dispatches(),
        RegistryDispatches {
            backend: 1,
            ..RegistryDispatches::default()
        }
    );
    assert!(snapshot
        .compiler_backends()
        .runtime::<VueRuntimeBackend>(&html_carrier, &html_profile)
        .is_none());
    assert!(
        snapshot
            .frontends()
            .row_for(&FrameworkAdapterId::new("html"), &LanguageId::new("html"))
            .is_some(),
        "the frontend-only carrier stays registered"
    );

    // Astro mixed demand: IDE companion plus a runtime product.
    let _ = take_registry_dispatches();
    let statuses: Vec<_> = [ProductKind::IdeCompanion, ProductKind::RuntimeClient]
        .into_iter()
        .map(|kind| snapshot.product_capability(&astro_carrier, &astro_profile, kind))
        .collect();
    assert_eq!(
        statuses,
        vec![
            ProductCapability::Supported,
            ProductCapability::Unsupported(CatalogCapability::Runtime),
        ]
    );
    assert_eq!(
        take_registry_dispatches(),
        RegistryDispatches {
            projection: 1,
            backend: 1,
            ..RegistryDispatches::default()
        }
    );
    assert!(
        snapshot
            .projection_for(&astro_carrier, &astro_profile)
            .is_some(),
        "the Astro IDE companion's projection backend is selected"
    );
    assert!(snapshot
        .compiler_backends()
        .row_for(&astro_carrier, &astro_profile)
        .is_none());
}

// ── Template facts without a compile demand (PC07) ──────────────────

#[test]
fn template_facts_without_a_compile_demand_never_enter_the_backend_registry() {
    let cases = [
        (
            FileLanguage::vue(),
            crate::framework_common::vue_grammar_config("{{", "}}", std::iter::empty::<&str>())
                .expect("default Vue grammar"),
            "<script setup>const a = 1</script><template><p>{{ a }}</p></template>",
        ),
        (
            FileLanguage::svelte(),
            crate::svelte::svelte_grammar_config(),
            "<script>let a = 1;</script><p>{a}</p>",
        ),
    ];
    for (language, grammar, source) in cases {
        let artifact = parse_registered_source_for_tests(language, grammar, source);
        let _ = take_registry_dispatches();
        let facts =
            template_facts_from_catalog(&artifact, source, TemplateFactsBasis::AdmittedArtifact);
        assert!(facts.is_some(), "the semantic authority produces the facts");
        assert_eq!(
            take_registry_dispatches(),
            RegistryDispatches {
                semantic: 1,
                ..RegistryDispatches::default()
            },
            "template facts enter the semantic table once and no other table"
        );
    }
}
