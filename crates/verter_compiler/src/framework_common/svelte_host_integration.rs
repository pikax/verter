//! Svelte [`FrameworkHostIntegrationBackend`] adapter for the native host.
//!
//! Composes the Svelte parse and semantic admissions into the ONE
//! [`SvelteCompileAdmission`] token, coordinates one canonical
//! multi-product [`CompileRequest`] per admitted demand, and drives the
//! shared Svelte bundle orchestration (ordered runtime /
//! IDE-projection / template-fact capability calls with one prerequisite
//! population — the self-contained `Main` module and its scoped-css
//! side-products come from that one population). Catalog lookup keys
//! adapter × epoch × host epoch × HostIntegration.
//!
//! Demand specificity lives in the admission's VALUE (the admitted demand
//! plus the requested product set): there is exactly one admission token
//! type for this host epoch, never product-scoped siblings. Capability
//! validation is demand-specific — a runtime-render demand never requires
//! projection capability — and a missing required capability is a typed
//! refusal, never a fallback onto another lane or framework.

use std::sync::Arc;

use verter_language::{FrameworkAdapterId, LanguageId, ParseKey};

use crate::compile_request::{
    unroutable_host_request_axis, CompileProduct, CompileRequest, CompileRequestError,
    FrameworkCompileRequest, ProductKind, RuntimeProductRequest, SvelteOptionAttempt,
    UnroutableHostRequestAxis,
};
use crate::standalone::registered_runtime_for;
use crate::svelte::carrier::{svelte_carrier_bundle, SvelteCarrierCompiler};
use crate::svelte::carrier_frontend::{SvelteCarrierFrontend, SvelteParseAdmission, SvelteSfc5};
use crate::svelte::semantic_authority::{SvelteSemanticAdmission, SvelteSemanticAuthority};

use super::capability::{
    FrameworkHostIntegrationBackend, NativeHostEpoch, ProductExecutionGrant, ProductExecutionGrants,
};
use super::carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeOutput, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeDiagnostic,
};
use super::catalog::{CatalogCapability, HostCap, TypedCapabilityRegistration};
use super::registered_carrier_projection::{
    registered_projection_for, registered_semantic_for, TemplateFactsProduct,
};
use super::{FrameworkParseArtifact, Present};

/// Svelte host-integration backend for the native host epoch.
///
/// Deliberately NOT `Clone`/`Copy`/`Default`, and not constructible
/// outside this crate (private field): every consumer holds the
/// `&'static` registered instance — from [`Self::registered`], the
/// immutable catalog, or a request-scoped binding — never a freshly
/// minted service value. The request-scoped session binding is the sole
/// production route to issuance on the native session lanes; the issued
/// admission's parse key pairs issuance with execution, and the
/// admission and its per-demand execution grants are consumed by value.
#[derive(Debug, PartialEq, Eq)]
pub struct SvelteHostIntegrationBackend {
    _registered: (),
}

impl SvelteHostIntegrationBackend {
    /// Crate-internal constructor; the only instances are the registered
    /// static and catalog registrations built here.
    pub(crate) const fn new() -> Self {
        Self { _registered: () }
    }

    /// The registered native-host instance. Holding this reference grants
    /// no execution: every execution entry additionally requires a
    /// host-issued consume-once admission (or a grant carved off one).
    #[must_use]
    pub fn registered() -> &'static Self {
        static REGISTERED: SvelteHostIntegrationBackend = SvelteHostIntegrationBackend::new();
        &REGISTERED
    }
}

// The backends are sealed identities, never duplicable service values.
static_assertions::assert_not_impl_any!(SvelteHostIntegrationBackend: Clone, Copy, Default);

/// Host-backed multi-product demand: every requested product plus the
/// typed Svelte option attempt the backend turns into the one canonical
/// request. Request construction is the backend's, not the caller's.
#[derive(Debug, Clone, Default)]
pub struct SvelteHostMultiProductDemand {
    /// The requested product set (runtime, IDE companion, analysis, ...).
    pub products: Vec<CompileProduct>,
    /// Typed Svelte option attempt; unsupported options refuse at issuance.
    pub svelte_options: SvelteOptionAttempt,
    /// Carrier file name for component-name + scope-hash + source-map
    /// identity.
    pub filename: Option<String>,
    /// Production mode.
    pub is_production: bool,
    /// Force JavaScript output.
    pub force_js: bool,
}

/// Runtime-render (render-only) demand: the backend constructs the
/// runtime-only product set itself; no other product can ride along.
///
/// Unlike Vue, the render demand carries no template-fact diagnostics
/// companion: the Svelte runtime lowering parses every template expression
/// itself and fails closed on a malformed one (a typed
/// [`SvelteHostCompileRefusal::RuntimeSurfaceRefused`]), so no separate
/// fact-producer pass is needed for the render lane to fail closed.
#[derive(Debug, Clone, Default)]
pub struct SvelteHostRuntimeRenderDemand {
    /// Runtime product options for the rendered main module.
    pub runtime: RuntimeProductRequest,
    /// Demand the SERVER runtime product instead of the client one.
    /// Svelte has no request-level ssr option flag (unlike Vue), so the
    /// render demand names its runtime kind directly — a flag/product-kind
    /// divergence is structurally unrepresentable.
    pub ssr: bool,
    /// Typed Svelte option attempt; unsupported options refuse at issuance.
    pub svelte_options: SvelteOptionAttempt,
    /// Carrier file name for component-name + scope-hash + source-map
    /// identity.
    pub filename: Option<String>,
    /// Production mode.
    pub is_production: bool,
    /// Force JavaScript output.
    pub force_js: bool,
}

/// Typed issuance refusal. Never a fallback to another lane, framework,
/// or compatibility compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SvelteHostAdmissionRefusal {
    /// The artifact is not a Svelte parse this frontend admitted.
    NotASvelteParse,
    /// No registered Svelte semantic authority for the artifact's epoch.
    SemanticUnavailable,
    /// A demanded product's required capability has no registered row for
    /// the artifact's epoch.
    CapabilityUnavailable {
        /// The demanded product whose capability is missing.
        product: ProductKind,
        /// The missing capability.
        capability: CatalogCapability,
    },
    /// The demanded product has no Svelte host production route.
    UnsupportedProduct(ProductKind),
    /// A demanded product/option shape is admissible by canonical request
    /// construction but has no production route through the host bundle
    /// execution — refused at issuance, never silently dropped or
    /// silently served by a different compile.
    UnproducibleDemand(SvelteHostUnproducibleDemand),
    /// Canonical request construction refused the demand.
    RequestConstructionRefused(CompileRequestError),
}

/// Why an admissible-looking demand cannot be PRODUCED by the host bundle
/// execution. Producibility validation, distinct from capability presence:
/// each variant names a demand shape whose compile would run with the
/// demanded axis dropped or substituted. (An axis the execution ROUTES and
/// then typed-refuses — `dev: true`, an svg/mathml namespace, ssr — is not
/// listed here: it fails closed downstream with its own precise surface,
/// never silently.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteHostUnproducibleDemand {
    /// Both runtime kinds in one demand: the host bundle orchestration
    /// executes exactly one ssr mode per pass and derives it from the
    /// product set, so a dual-kind demand would serve one kind a bundle
    /// whose compile never ran.
    DualRuntimeKind,
    /// `AnalysisProductRequest.want_script_bindings`: execution never
    /// produces script bindings on this route and no accessor publishes
    /// them.
    AnalysisScriptBindings,
    /// `css` (injected/external selection): the bundle execution derives
    /// its css mode from the source (`<svelte:options>` + style analysis)
    /// and carries no request-level css channel, so an explicit demand
    /// would be silently dropped.
    CssMode,
    /// A request-level custom-element descriptor: the bundle execution
    /// resolves the descriptor from the parsed `<svelte:options>` element
    /// only and carries no request-level descriptor channel.
    CustomElementDescriptor,
    /// The `compatibility` canonical object: no bundle routing channel.
    Compatibility,
    /// `namespace: "foreign"`: the bundle option derivation has no routing
    /// token for the foreign namespace, so honoring the demand would
    /// silently substitute the html namespace.
    ForeignNamespace,
    /// A framework-NEUTRAL request axis the bundle execution would drop or
    /// substitute. These rows live on the product requests and on the
    /// request itself, so both host integrations refuse them identically.
    UnroutableAxis(UnroutableHostRequestAxis),
}

/// Which demand an admission was issued for. Value-level demand
/// specificity — never a sibling token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteAdmittedDemand {
    /// Host-backed multi-product demand.
    HostMultiProduct,
    /// Runtime-render (render-only) demand.
    RuntimeRender,
}

// Consume-once by-value evidence must never be duplicable or
// round-trippable through a serialized form: one issuance drives at most
// one execution.
static_assertions::assert_not_impl_any!(
    SvelteCompileAdmission: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);

/// The sole Svelte compile-admission token for the native host epoch:
/// the admitted demand, the one canonical request, the exact parse
/// binding, and the composed parse + semantic admissions. It is not a
/// capability or service bag — execution re-selects capabilities from the
/// immutable catalog.
#[derive(Debug)]
pub struct SvelteCompileAdmission {
    demand: SvelteAdmittedDemand,
    request: CompileRequest,
    parse_key: Arc<ParseKey>,
    _parse: SvelteParseAdmission,
    _semantic: SvelteSemanticAdmission,
}

impl SvelteCompileAdmission {
    /// The admitted demand.
    #[must_use]
    pub fn demand(&self) -> SvelteAdmittedDemand {
        self.demand
    }

    /// The one canonical request this admission admits.
    #[must_use]
    pub fn request(&self) -> &CompileRequest {
        &self.request
    }

    /// The admitted product set, in request order.
    #[must_use]
    pub fn admitted_products(&self) -> Vec<ProductKind> {
        self.request
            .products()
            .iter()
            .map(CompileProduct::kind)
            .collect()
    }

    /// Carve the per-demand consume-once execution grants off this
    /// admission, by value: the admission is destroyed, and each admitted
    /// product-backend leg receives at most one grant. This is the sole
    /// out-of-crate path to a [`ProductExecutionGrant`] — a product
    /// backend cannot be driven without an issuance from this backend.
    #[must_use]
    pub fn into_execution_grants(self) -> ProductExecutionGrants {
        execution_grants_for_request(&self.request)
    }
}

/// One grant per admitted product-backend leg of an admitted request.
fn execution_grants_for_request(request: &CompileRequest) -> ProductExecutionGrants {
    let mut grants = ProductExecutionGrants::default();
    for product in request.products() {
        match product.kind() {
            kind @ (ProductKind::RuntimeClient | ProductKind::RuntimeServer) => {
                grants.runtime = Some(ProductExecutionGrant::mint(kind));
            }
            ProductKind::IdeCompanion => {
                grants.projection = Some(ProductExecutionGrant::mint(ProductKind::IdeCompanion));
            }
            _ => {}
        }
    }
    grants
}

/// Execution inputs excluded from admission identity (host-selected block
/// bytes and host-resolved Svelte facts).
#[derive(Debug, Clone, Default)]
pub struct SvelteHostExecutionInputs {
    /// Host-selected block bytes for supplied templates, scripts, styles.
    pub block_content: super::carrier_compiler::RuntimeBlockContentInputs,
    /// The RESOLVED Svelte `cssHash` scope-class override — the official
    /// user callback's already-computed result (the callback runs at the
    /// host/session boundary), preserved byte-exact.
    pub css_hash_override: Option<String>,
    /// Host-retained parsed style IRs in inventory order.
    pub prepared_styles: Vec<Option<crate::style_planner::PreparedStyleIr>>,
    /// Host-admitted completed external preprocessing results, one slot per
    /// style block in inventory order. Each present slot is bound to its
    /// block through the stage-qualified continuation boundary before
    /// execution; a result that cannot be bound refuses the compile.
    pub supplied_styles: Vec<Option<SvelteSuppliedStyle>>,
}

/// One host-admitted, completed external preprocessing result for a style
/// block, carrying exactly the facts only the host can state.
#[derive(Debug, Clone)]
pub struct SvelteSuppliedStyle {
    /// The tool that produced the bytes. A slot the host names no external
    /// producer for is `Anonymous`: its bytes are still the block's only
    /// body, so it continues the block like any other result rather than
    /// being dropped back onto the authored block.
    pub producer: verter_css_syntax::PreprocessorIdentity,
    /// The produced plain-CSS bytes.
    pub code: Arc<str>,
    /// Host-minted source space containing `code`.
    pub source_space_token: String,
    /// Host-minted identity of the exact produced byte artifact.
    pub content_artifact_token: String,
    /// The host-admitted map from `code` back to the authored bytes the tool
    /// consumed. It is the only way a position in the produced bytes reaches
    /// the authored block, so a continued block's published css map is
    /// chained through it, or published absent when there is none.
    pub source_map: Option<Arc<str>>,
    /// Diagnostics the producing tool reported, in production order. They
    /// ride the continuation's result and the published style, never
    /// dropped at this boundary.
    pub diagnostics: Vec<verter_css_syntax::StyleDiagnostic>,
    /// The host's retained parse of exactly these bytes, when admission
    /// produced one.
    pub parsed: Option<crate::style_planner::PreparedStyleIr>,
    /// Content identity of the authored bytes the tool consumed, observed by
    /// the host under the same fence that selected `code` — not recomputed
    /// from a later read of the carrier, which would bind a revision the host
    /// never validated. `None` when the host could not state it: such a
    /// result is refused, never compiled from the authored block in its
    /// place.
    pub consumed_basis: Option<crate::assembly::ContentId>,
}

/// The refusal code for a supplied style result that could not be bound to
/// its block as a stage-qualified continuation.
const STYLE_CONTINUATION_REFUSED: &str = "svelte-runtime-style-continuation-refused";

/// Typed execution refusal. All-or-none: a refusal publishes no product.
#[derive(Debug)]
pub enum SvelteHostCompileRefusal {
    /// The presented artifact is not the parse this admission was issued
    /// over.
    AdmissionParseMismatch,
    /// The admission was issued for the other demand kind.
    WrongDemand {
        /// The demand this entry point serves.
        expected: SvelteAdmittedDemand,
        /// The demand the admission actually admits.
        actual: SvelteAdmittedDemand,
    },
    /// The shared orchestration refused the admitted request.
    Unsupported(CompileUnsupported),
    /// A requested runtime surface was refused; no sibling product
    /// publishes after this refusal.
    RuntimeSurfaceRefused {
        /// Structural refusal code.
        diagnostic_code: String,
        /// Human-readable refusal reason.
        message: String,
        /// Carrier-absolute span of the refusing construct (whole-source
        /// for a whole-component oracle result).
        span: verter_span::Span,
        /// Non-fatal diagnostics collected before the refusal.
        diagnostics: Vec<RuntimeDiagnostic>,
    },
}

/// Per-product publication payloads of one admitted multi-product compile.
/// Every accessor gates on the admitted product set: a prerequisite that
/// was produced but not admitted is never published.
#[derive(Debug)]
pub struct SvelteHostCompiledProducts {
    admitted: Vec<ProductKind>,
    bundle: RuntimeCompileOutput,
}

impl SvelteHostCompiledProducts {
    fn admits(&self, kind: ProductKind) -> bool {
        self.admitted.contains(&kind)
    }

    /// The CLIENT runtime bundle publication payload (the self-contained
    /// `Main` module plus its scoped-css style side-products), when the
    /// client runtime product was admitted. Per-kind: a bundle whose
    /// compile ran in the other ssr mode can never satisfy this accessor
    /// (a dual-kind demand is already refused at issuance).
    #[must_use]
    pub fn runtime_client_bundle(&self) -> Option<&RuntimeCompileOutput> {
        self.admits(ProductKind::RuntimeClient)
            .then_some(&self.bundle)
    }

    /// The SERVER runtime bundle publication payload, when the server
    /// runtime product was admitted. Per-kind — see
    /// [`Self::runtime_client_bundle`].
    #[must_use]
    pub fn runtime_server_bundle(&self) -> Option<&RuntimeCompileOutput> {
        self.admits(ProductKind::RuntimeServer)
            .then_some(&self.bundle)
    }

    /// The IDE companion publication payload, when admitted.
    #[must_use]
    pub fn ide_companion(&self) -> Option<&IdeOutput> {
        self.admits(ProductKind::IdeCompanion)
            .then_some(self.bundle.tsx.as_ref())
            .flatten()
    }

    /// The admitted template facts, when the analysis product was admitted.
    #[must_use]
    pub fn template_facts(&self) -> Option<&TemplateFactsProduct> {
        self.admits(ProductKind::Analysis)
            .then_some(self.bundle.template_data.as_ref())
            .flatten()
    }

    /// Aggregated non-fatal diagnostics of the whole admitted compile.
    #[must_use]
    pub fn diagnostics(&self) -> &[RuntimeDiagnostic] {
        &self.bundle.diagnostics
    }

    /// Wrap an already-produced runtime bundle as an admitted product.
    /// Test/harness constructor for publication-refusal coverage.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_admitted_runtime_bundle(bundle: RuntimeCompileOutput, kind: ProductKind) -> Self {
        Self {
            admitted: vec![kind],
            bundle,
        }
    }
}

/// Render-only handoff of one admitted runtime-render compile: the
/// runtime bundle the host assembles into the `Main` virtual module (the
/// self-contained `svelte/internal/client` module plus its scoped-css
/// side-products). No IDE or analysis payload exists on this handoff.
#[derive(Debug)]
pub struct SvelteHostRenderedMain {
    bundle: RuntimeCompileOutput,
}

impl SvelteHostRenderedMain {
    /// The runtime bundle for host `Main` assembly.
    #[must_use]
    pub fn runtime_bundle(&self) -> &RuntimeCompileOutput {
        &self.bundle
    }
}

impl SvelteHostIntegrationBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        SvelteCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend integrates.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        SvelteCarrierCompiler.carrier_language_id()
    }

    /// Compose the parse + semantic admissions over the artifact — the
    /// shared first half of both issuance entry points.
    fn compose_admissions(
        &self,
        artifact: &FrameworkParseArtifact,
    ) -> Result<(SvelteParseAdmission, SvelteSemanticAdmission), SvelteHostAdmissionRefusal> {
        let parse = SvelteCarrierFrontend
            .admit_registered(artifact)
            .ok_or(SvelteHostAdmissionRefusal::NotASvelteParse)?;
        let semantic = SvelteSemanticAuthority
            .admit_over_parse(&parse, artifact)
            .ok_or(SvelteHostAdmissionRefusal::SemanticUnavailable)?;
        Ok((parse, semantic))
    }

    #[allow(clippy::too_many_arguments)]
    fn issue(
        &self,
        demand: SvelteAdmittedDemand,
        products: Vec<CompileProduct>,
        svelte_options: SvelteOptionAttempt,
        filename: Option<String>,
        is_production: bool,
        force_js: bool,
        parse: SvelteParseAdmission,
        semantic: SvelteSemanticAdmission,
    ) -> Result<SvelteCompileAdmission, SvelteHostAdmissionRefusal> {
        let svelte = svelte_options
            .into_request()
            .map_err(SvelteHostAdmissionRefusal::RequestConstructionRefused)?;
        let request = CompileRequest::new(
            products,
            FrameworkCompileRequest::Svelte(svelte),
            None,
            filename,
            None,
            is_production,
            force_js,
        )
        .map_err(SvelteHostAdmissionRefusal::RequestConstructionRefused)?;
        self.issue_admitted(demand, request, parse, semantic)
    }

    /// The one issuance tail every entry point shares: producibility
    /// validation over the CANONICAL request, then the admission token.
    ///
    /// Validating here rather than over each entry's own demand vocabulary
    /// is what lets a caller-supplied canonical request reach exactly the
    /// same refusals as a composed one — there is one producibility
    /// authority, reading the one document the execution actually consumes.
    fn issue_admitted(
        &self,
        demand: SvelteAdmittedDemand,
        request: CompileRequest,
        parse: SvelteParseAdmission,
        semantic: SvelteSemanticAdmission,
    ) -> Result<SvelteCompileAdmission, SvelteHostAdmissionRefusal> {
        let svelte =
            request
                .svelte()
                .ok_or(SvelteHostAdmissionRefusal::RequestConstructionRefused(
                    CompileRequestError::FrameworkMismatch {
                        expected: "svelte",
                        actual: "vue",
                    },
                ))?;
        refuse_unproducible_products(request.products())?;
        refuse_unroutable_axes(&request)?;
        refuse_unproducible_svelte_options(svelte)?;
        Ok(SvelteCompileAdmission {
            demand,
            request,
            // The binding is the parse admission's own witnessed key —
            // the exact identity `admit_registered` issued over.
            parse_key: Arc::clone(parse.parse_key()),
            _parse: parse,
            _semantic: semantic,
        })
    }

    /// Capability presence for every demanded product, in request order.
    /// Shared by the demand entries and the caller-supplied-request entry
    /// so the three cannot drift on which capability a product requires.
    fn refuse_unavailable_capabilities(
        artifact: &FrameworkParseArtifact,
        products: &[CompileProduct],
    ) -> Result<(), SvelteHostAdmissionRefusal> {
        for product in products {
            let required = match product.kind() {
                ProductKind::RuntimeClient | ProductKind::RuntimeServer => {
                    registered_runtime_for(artifact.adapter_id(), artifact.epoch())
                        .is_some()
                        .then_some(())
                        .ok_or(CatalogCapability::Runtime)
                }
                ProductKind::IdeCompanion => {
                    registered_projection_for(artifact.adapter_id(), artifact.epoch())
                        .is_some()
                        .then_some(())
                        .ok_or(CatalogCapability::Projection)
                }
                ProductKind::Analysis => {
                    registered_semantic_for(artifact.adapter_id(), artifact.epoch())
                        .is_some()
                        .then_some(())
                        .ok_or(CatalogCapability::Semantic)
                }
                kind @ (ProductKind::PublicApi | ProductKind::Declarations) => {
                    return Err(SvelteHostAdmissionRefusal::UnsupportedProduct(kind));
                }
            };
            if let Err(capability) = required {
                return Err(SvelteHostAdmissionRefusal::CapabilityUnavailable {
                    product: product.kind(),
                    capability,
                });
            }
        }
        Ok(())
    }

    /// Compile a host-backed multi-product admission through the shared
    /// orchestration, publishing per-product payloads gated on the
    /// admitted set. One admitted request populates its parse, semantic,
    /// projection, plan, and emit prerequisites once — the self-contained
    /// `Main` and its requested style side-products come from that one
    /// population, never a second compile. Consumes the admission by
    /// value: one issuance drives at most one execution.
    pub fn compile_host_products(
        &self,
        admission: SvelteCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &SvelteHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<SvelteHostCompiledProducts, SvelteHostCompileRefusal> {
        if admission.demand != SvelteAdmittedDemand::HostMultiProduct {
            return Err(SvelteHostCompileRefusal::WrongDemand {
                expected: SvelteAdmittedDemand::HostMultiProduct,
                actual: admission.demand,
            });
        }
        let admitted = admission.admitted_products();
        let bundle = self.execute(admission, artifact, inputs, alloc)?;
        Ok(SvelteHostCompiledProducts { admitted, bundle })
    }

    /// Compile a runtime-render admission: the render-only handoff for
    /// host `Main` assembly. Never plans or publishes an IDE companion or
    /// template facts. Consumes the admission by value: one issuance
    /// drives at most one execution.
    pub fn compile_runtime_render(
        &self,
        admission: SvelteCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &SvelteHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<SvelteHostRenderedMain, SvelteHostCompileRefusal> {
        if admission.demand != SvelteAdmittedDemand::RuntimeRender {
            return Err(SvelteHostCompileRefusal::WrongDemand {
                expected: SvelteAdmittedDemand::RuntimeRender,
                actual: admission.demand,
            });
        }
        let mut bundle = self.execute(admission, artifact, inputs, alloc)?;
        // Render-only handoff: no analysis product was admitted, so no
        // fact payload was produced; keep the invariant structural even if
        // the orchestration ever grows a fact side channel.
        bundle.template_data = None;
        Ok(SvelteHostRenderedMain { bundle })
    }

    /// The one execution path both entry points share: verify the exact
    /// parse binding, derive the neutral options off the admitted request,
    /// and drive the shared Svelte bundle orchestration over the artifact's
    /// OWN registered source bytes — the admitted artifact is the single
    /// authority for both geometry and bytes, so a byte payload diverging
    /// from the admitted parse is unrepresentable at this seam. Caller owns
    /// the allocator scratch lifecycle; a dropped admission simply never
    /// executes — there is no partial publication channel.
    ///
    /// Consumes the admission by value and carves the per-demand execution
    /// grants off it: one issuance drives one execution of each admitted
    /// demand, and each product-backend leg consumes its own grant.
    fn execute(
        &self,
        admission: SvelteCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &SvelteHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, SvelteHostCompileRefusal> {
        record_host_backend_execution();
        if artifact.parse_key() != admission.parse_key.as_ref() {
            return Err(SvelteHostCompileRefusal::AdmissionParseMismatch);
        }
        let source = artifact.carrier_source();
        // Bound before any product runs: a supplied result that does not
        // describe its block refuses the whole compile, so nothing publishes
        // or warms from it.
        let continuations = crate::svelte::carrier::bind_svelte_style_continuations(
            artifact,
            &admission.request,
            &inputs.supplied_styles,
        )
        .map_err(|refusal| SvelteHostCompileRefusal::RuntimeSurfaceRefused {
            diagnostic_code: STYLE_CONTINUATION_REFUSED.to_string(),
            message: format!("supplied style result cannot continue its block: {refusal:?}"),
            span: refusal.span(source),
            diagnostics: Vec::new(),
        })?;
        let opts = derive_admitted_runtime_options(&admission.request, inputs, continuations);
        let grants = execution_grants_for_request(&admission.request);
        drop(admission);
        match svelte_carrier_bundle(source, artifact, &opts, alloc, grants) {
            Ok(CarrierCompileOutcome::Produced(bundle)) => Ok(bundle),
            // All-or-none: a refused runtime surface publishes nothing;
            // sibling projection/analysis products never warm or publish
            // after the refusal.
            Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(refusal)) => {
                Err(SvelteHostCompileRefusal::RuntimeSurfaceRefused {
                    diagnostic_code: refusal.diagnostic_code,
                    message: refusal.message,
                    span: refusal.span,
                    diagnostics: refusal.diagnostics,
                })
            }
            Err(unsupported) => Err(SvelteHostCompileRefusal::Unsupported(unsupported)),
        }
    }
}

impl FrameworkHostIntegrationBackend<SvelteSfc5, NativeHostEpoch> for SvelteHostIntegrationBackend {
    type CompileAdmission = SvelteCompileAdmission;
    type ParseArtifact = FrameworkParseArtifact;
    type MultiProductDemand = SvelteHostMultiProductDemand;
    type RuntimeRenderDemand = SvelteHostRuntimeRenderDemand;
    type AdmissionRefusal = SvelteHostAdmissionRefusal;

    fn admit_host_products(
        &self,
        artifact: &FrameworkParseArtifact,
        demand: SvelteHostMultiProductDemand,
    ) -> Result<SvelteCompileAdmission, SvelteHostAdmissionRefusal> {
        let (parse, semantic) = self.compose_admissions(artifact)?;
        // The shared issuance tail re-checks this over the composed
        // request. Checking the DEMAND first keeps the refusal a caller
        // sees when a demand is both unproducible and carries an
        // unsupported option: the product-set refusal wins, as it always
        // has, rather than the option refusal request construction raises.
        refuse_unproducible_products(&demand.products)?;
        Self::refuse_unavailable_capabilities(artifact, &demand.products)?;
        self.issue(
            SvelteAdmittedDemand::HostMultiProduct,
            demand.products,
            demand.svelte_options,
            demand.filename,
            demand.is_production,
            demand.force_js,
            parse,
            semantic,
        )
    }

    fn admit_runtime_render(
        &self,
        artifact: &FrameworkParseArtifact,
        demand: SvelteHostRuntimeRenderDemand,
    ) -> Result<SvelteCompileAdmission, SvelteHostAdmissionRefusal> {
        let (parse, semantic) = self.compose_admissions(artifact)?;
        // Demand-specific validation: the render lane requires ONLY the
        // runtime capability — projection is never consulted.
        if registered_runtime_for(artifact.adapter_id(), artifact.epoch()).is_none() {
            return Err(SvelteHostAdmissionRefusal::CapabilityUnavailable {
                product: if demand.ssr {
                    ProductKind::RuntimeServer
                } else {
                    ProductKind::RuntimeClient
                },
                capability: CatalogCapability::Runtime,
            });
        }
        let products = vec![if demand.ssr {
            CompileProduct::RuntimeServer(demand.runtime)
        } else {
            CompileProduct::RuntimeClient(demand.runtime)
        }];
        self.issue(
            SvelteAdmittedDemand::RuntimeRender,
            products,
            demand.svelte_options,
            demand.filename,
            demand.is_production,
            demand.force_js,
            parse,
            semantic,
        )
    }

    fn admit_canonical_request(
        &self,
        artifact: &FrameworkParseArtifact,
        request: CompileRequest,
    ) -> Result<SvelteCompileAdmission, SvelteHostAdmissionRefusal> {
        let (parse, semantic) = self.compose_admissions(artifact)?;
        // Same order as the demand entries: the product-set refusal wins
        // over a capability refusal when a request is both unproducible and
        // demands an unregistered capability. The shared issuance tail
        // re-checks both over the same document; a request whose framework
        // arm is not this backend's skips straight to the tail's typed
        // mismatch.
        if request.svelte().is_some() {
            refuse_unproducible_products(request.products())?;
            refuse_unroutable_axes(&request)?;
        }
        Self::refuse_unavailable_capabilities(artifact, request.products())?;
        self.issue_admitted(
            SvelteAdmittedDemand::HostMultiProduct,
            request,
            parse,
            semantic,
        )
    }
}

/// Reads the framework-neutral options off the ADMITTED request — the
/// compiler-side half of the construct-then-derive pattern: the request is
/// the identity authority, the execution inputs ride beside it.
fn derive_admitted_runtime_options(
    request: &CompileRequest,
    inputs: &SvelteHostExecutionInputs,
    style_continuations: Vec<Option<super::carrier_compiler::BoundStyleContinuation>>,
) -> RuntimeCompileOptions {
    use crate::compile_request::svelte::{
        SvelteFragmentsRequest, SvelteNamespaceRequest, SvelteRunesRequest,
    };

    let runtime = request.products().iter().find_map(|p| match p {
        CompileProduct::RuntimeClient(r) | CompileProduct::RuntimeServer(r) => Some(r),
        _ => None,
    });
    let ide = request.products().iter().find_map(|p| match p {
        CompileProduct::IdeCompanion(i) => Some(i),
        _ => None,
    });
    let want_template_data = request
        .products()
        .iter()
        .any(|p| matches!(p, CompileProduct::Analysis(a) if a.want_template_data));
    let ssr = request
        .products()
        .iter()
        .any(|p| matches!(p, CompileProduct::RuntimeServer(_)));
    let svelte = request.svelte();

    // A continued block's body is its continuation's produced bytes, so the
    // only parse that can stand in for it is the host's parse of exactly
    // those bytes; the runtime still verifies it against them before reuse.
    let mut prepared_styles = inputs.prepared_styles.clone();
    for (index, continuation) in style_continuations.iter().enumerate() {
        if continuation.is_none() {
            continue;
        }
        if prepared_styles.len() <= index {
            prepared_styles.resize(index + 1, None);
        }
        prepared_styles[index] = inputs
            .supplied_styles
            .get(index)
            .and_then(Option::as_ref)
            .and_then(|supplied| supplied.parsed.clone());
    }

    RuntimeCompileOptions {
        filename: request.filename().map(str::to_string),
        is_production: request.is_production(),
        custom_element: svelte.and_then(|s| s.custom_element).unwrap_or(false),
        // Per-leg, read faithfully off each admitted product's OWN
        // request: the runtime flag drives only the runtime leg and the
        // IDE flag only the IDE leg — demanding a map on one never
        // switches it on for the other.
        source_map: runtime.is_some_and(|r| r.runtime_source_map),
        ide_source_map: Some(ide.is_some_and(|i| i.want_source_map)),
        ssr,
        // The style stages this render owns, read off the admitted request.
        // A bundler that owns its own style-module lane demands
        // `AuthoredOnly`; dropping it here would run the full cascade and
        // fail closed on a preprocessor dialect the bundler was going to
        // handle.
        style_processing: request.runtime_style_processing(),
        force_js: request.force_js(),
        svelte_css_hash_override: inputs.css_hash_override.clone(),
        svelte_dev: svelte.and_then(|s| s.dev),
        svelte_runes: svelte.and_then(|s| s.runes).and_then(|r| match r {
            SvelteRunesRequest::True => Some(true),
            SvelteRunesRequest::False => Some(false),
            SvelteRunesRequest::Infer => None,
        }),
        // `Foreign` is refused at issuance (no routing token), so the
        // admitted values map totally.
        svelte_namespace: svelte.and_then(|s| s.namespace).and_then(|n| match n {
            SvelteNamespaceRequest::Html => Some("html".to_string()),
            SvelteNamespaceRequest::Svg => Some("svg".to_string()),
            SvelteNamespaceRequest::MathMl => Some("mathml".to_string()),
            SvelteNamespaceRequest::Foreign => None,
        }),
        svelte_fragments: svelte.and_then(|s| s.fragments).map(|f| {
            match f {
                SvelteFragmentsRequest::Html => "html",
                SvelteFragmentsRequest::Tree => "tree",
            }
            .to_string()
        }),
        svelte_preserve_whitespace: svelte.and_then(|s| s.preserve_whitespace),
        svelte_preserve_comments: svelte.and_then(|s| s.preserve_comments),
        svelte_disclose_version: svelte.and_then(|s| s.disclose_version),
        inline: runtime.and_then(|r| r.inline),
        want_runtime: runtime.is_some(),
        want_ide: ide.is_some(),
        want_template_data,
        embed_ambient_types: ide.is_some_and(|i| i.embed_ambient_types),
        block_content: inputs.block_content.clone(),
        prepared_styles,
        style_continuations,
        ..RuntimeCompileOptions::default()
    }
}

/// Producibility validation over the demanded PRODUCT SET: the host
/// bundle execution runs exactly one ssr mode per pass and derives it
/// from the product set, and it never produces script bindings. A demand
/// the execution would serve with a dropped or substituted axis refuses
/// at issuance.
fn refuse_unproducible_products(
    products: &[CompileProduct],
) -> Result<(), SvelteHostAdmissionRefusal> {
    let wants_client = products
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeClient);
    let wants_server = products
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeServer);
    if wants_client && wants_server {
        return Err(SvelteHostAdmissionRefusal::UnproducibleDemand(
            SvelteHostUnproducibleDemand::DualRuntimeKind,
        ));
    }
    if products
        .iter()
        .any(|p| matches!(p, CompileProduct::Analysis(a) if a.want_script_bindings))
    {
        return Err(SvelteHostAdmissionRefusal::UnproducibleDemand(
            SvelteHostUnproducibleDemand::AnalysisScriptBindings,
        ));
    }
    Ok(())
}

/// Producibility validation over the request's FRAMEWORK-NEUTRAL axes:
/// every caller-settable product/request axis the bundle execution has no
/// routing channel for refuses at issuance rather than being silently
/// dropped or substituted. The rows and their order are the shared
/// [`unroutable_host_request_axis`] reader's, so both host integrations
/// refuse identically.
fn refuse_unroutable_axes(request: &CompileRequest) -> Result<(), SvelteHostAdmissionRefusal> {
    match unroutable_host_request_axis(request) {
        Some(axis) => Err(SvelteHostAdmissionRefusal::UnproducibleDemand(
            SvelteHostUnproducibleDemand::UnroutableAxis(axis),
        )),
        None => Ok(()),
    }
}

/// Producibility validation over the admitted request's SVELTE OPTIONS:
/// every axis [`derive_admitted_runtime_options`] cannot route into the
/// bundle execution refuses at issuance instead of being silently dropped
/// or substituted. (The six unconditionally-unsupported option rows plus
/// the `SVELTE-MODULE`-gated pair refuse separately, at canonical request
/// construction.) Deterministic declaration order.
///
/// Reads the CANONICAL request rather than an option attempt, so a
/// caller-supplied request and a composed one are held to the identical
/// field set by the identical code.
fn refuse_unproducible_svelte_options(
    options: &crate::compile_request::SvelteCompileRequest,
) -> Result<(), SvelteHostAdmissionRefusal> {
    use crate::compile_request::svelte::SvelteNamespaceRequest;
    let unroutable: [(bool, SvelteHostUnproducibleDemand); 4] = [
        (options.css.is_some(), SvelteHostUnproducibleDemand::CssMode),
        (
            options.custom_element_descriptor.is_some(),
            SvelteHostUnproducibleDemand::CustomElementDescriptor,
        ),
        (
            options.compatibility.is_some(),
            SvelteHostUnproducibleDemand::Compatibility,
        ),
        (
            options.namespace == Some(SvelteNamespaceRequest::Foreign),
            SvelteHostUnproducibleDemand::ForeignNamespace,
        ),
    ];
    for (present, demand) in unroutable {
        if present {
            return Err(SvelteHostAdmissionRefusal::UnproducibleDemand(demand));
        }
    }
    Ok(())
}

/// Typed Svelte host-integration catalog row for the native host epoch.
#[must_use]
pub fn svelte_host_integration_registration(
) -> TypedCapabilityRegistration<HostCap<SvelteHostIntegrationBackend>> {
    TypedCapabilityRegistration::register_host_integration::<SvelteSfc5, NativeHostEpoch, _>(
        SvelteHostIntegrationBackend::new().adapter_id(),
        SvelteHostIntegrationBackend::new().carrier_language_id(),
        Present(SvelteHostIntegrationBackend::new()),
    )
}

#[cfg(test)]
thread_local! {
    /// Per-thread count of host-backend executions, so tests can prove
    /// the generic production compile route never consults this backend.
    static HOST_BACKEND_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn host_backend_execution_count() -> usize {
    HOST_BACKEND_EXECUTIONS.with(std::cell::Cell::get)
}

fn record_host_backend_execution() {
    #[cfg(test)]
    HOST_BACKEND_EXECUTIONS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
mod tests {
    use super::super::registered_carrier_projection::{
        parse_registered_source_for_tests, projection_catalog_consult_count,
        take_projection_producer_invocations, take_template_facts_producer_invocations,
    };
    use super::super::vue_host_integration::{
        built_in_host_integration_catalog, registered_host_integration_for,
        InstalledHostIntegration,
    };
    use super::super::{CarrierCompileOutcome, HostEpochId, RuntimeCompileOptions};
    use super::*;
    use crate::compile_request::svelte::{SvelteCssRequest, SvelteCustomElementDescriptor};
    use crate::compile_request::{AnalysisProductRequest, IdeProductRequest, SvelteOption};
    use crate::framework_common::capability::HostEpoch;
    use crate::standalone::runtime_backend_delegation_count;
    use verter_language::carrier_grammar::CarrierGrammarConfig;
    use verter_language::FileLanguage;

    fn svelte_artifact(source: &str) -> Arc<FrameworkParseArtifact> {
        parse_registered_source_for_tests(
            FileLanguage::svelte(),
            CarrierGrammarConfig::Svelte,
            source,
        )
    }

    const COMPONENT: &str = "<script>let count = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => count++}>{count}</button>\n";

    fn multi_demand() -> SvelteHostMultiProductDemand {
        SvelteHostMultiProductDemand {
            products: vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::IdeCompanion(IdeProductRequest::default()),
                CompileProduct::Analysis(AnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
            filename: Some("App.svelte".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn host_integration_catalog_registers_the_svelte_native_row() {
        let catalog = built_in_host_integration_catalog();
        let identity = catalog
            .iter()
            .map(|row| row.identity())
            .find(|identity| identity.adapter_id() == &FrameworkAdapterId::svelte())
            .expect("the catalog holds the Svelte host-integration row");
        assert_eq!(identity.capability(), CatalogCapability::HostIntegration);
        assert_eq!(identity.epoch().as_str(), "svelte");
        assert_eq!(
            identity.host_epoch(),
            Some(&HostEpochId::new(NativeHostEpoch::ID))
        );
        let (row_identity, installed) = registered_host_integration_for::<NativeHostEpoch>(
            &FrameworkAdapterId::svelte(),
            identity.epoch(),
        )
        .expect("the native Svelte host row resolves");
        assert_eq!(
            row_identity, identity,
            "the lookup returns the matched row's own identity"
        );
        assert!(
            matches!(installed, InstalledHostIntegration::Svelte(_)),
            "the installed payload is the Svelte arm of the one host catalog"
        );
    }

    #[test]
    fn multi_product_admission_composes_one_canonical_request() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("a Svelte parse with registered capabilities admits");
        assert_eq!(admission.demand(), SvelteAdmittedDemand::HostMultiProduct);
        assert_eq!(
            admission.admitted_products(),
            vec![
                ProductKind::RuntimeClient,
                ProductKind::IdeCompanion,
                ProductKind::Analysis
            ]
        );
        assert_eq!(admission.request().filename(), Some("App.svelte"));
    }

    #[test]
    fn runtime_render_admission_is_runtime_only_and_never_consults_projection() {
        let artifact = svelte_artifact(COMPONENT);
        let before = projection_catalog_consult_count();
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("a Svelte parse with a registered runtime backend admits");
        assert_eq!(admission.demand(), SvelteAdmittedDemand::RuntimeRender);
        assert_eq!(
            admission.admitted_products(),
            vec![ProductKind::RuntimeClient],
            "the backend constructs the runtime-only product set itself"
        );
        assert_eq!(
            projection_catalog_consult_count(),
            before,
            "a runtime-render demand must not require projection capability"
        );

        // The multi-product demand naming the IDE companion DOES consult it
        // — the counter discriminates the two demand validations.
        let _ = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("multi-product admits");
        assert_eq!(projection_catalog_consult_count(), before + 1);
    }

    #[test]
    fn ssr_render_demand_admits_the_server_runtime_product() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(
                &artifact,
                SvelteHostRuntimeRenderDemand {
                    ssr: true,
                    ..Default::default()
                },
            )
            .expect("an SSR render demand admits the server runtime");
        assert_eq!(
            admission.admitted_products(),
            vec![ProductKind::RuntimeServer]
        );
    }

    #[test]
    fn ssr_render_execution_refuses_typed_never_a_client_fallback() {
        // The Svelte server backend has not landed: the admitted server
        // demand fails CLOSED at execution with the precise typed surface
        // — never a silent client-mode compile in its place.
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(
                &artifact,
                SvelteHostRuntimeRenderDemand {
                    ssr: true,
                    ..Default::default()
                },
            )
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("the server backend is fail-closed until it lands");
        match refusal {
            SvelteHostCompileRefusal::RuntimeSurfaceRefused {
                diagnostic_code, ..
            } => {
                assert_eq!(
                    diagnostic_code,
                    "svelte-runtime-unsupported-server-generate"
                );
            }
            other => panic!("expected the typed server-generate refusal, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_product_refuses_typed_and_issues_nothing() {
        let artifact = svelte_artifact(COMPONENT);
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::PublicApi(Default::default())],
                    ..Default::default()
                },
            )
            .expect_err("the Svelte host route has no public-api production path");
        assert_eq!(
            refusal,
            SvelteHostAdmissionRefusal::UnsupportedProduct(ProductKind::PublicApi)
        );
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::Declarations(Default::default())],
                    ..Default::default()
                },
            )
            .expect_err("the Svelte host route has no declarations production path");
        assert_eq!(
            refusal,
            SvelteHostAdmissionRefusal::UnsupportedProduct(ProductKind::Declarations)
        );
    }

    #[test]
    fn dual_runtime_kind_demand_refuses_at_issuance() {
        let artifact = svelte_artifact(COMPONENT);
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![
                        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                        CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
                    ],
                    ..Default::default()
                },
            )
            .expect_err("one bundle pass runs one ssr mode; a dual-kind demand cannot be served");
        assert_eq!(
            refusal,
            SvelteHostAdmissionRefusal::UnproducibleDemand(
                SvelteHostUnproducibleDemand::DualRuntimeKind
            )
        );
    }

    #[test]
    fn analysis_script_bindings_demand_refuses_at_issuance() {
        let artifact = svelte_artifact(COMPONENT);
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::Analysis(AnalysisProductRequest {
                        want_script_bindings: true,
                        want_template_data: true,
                    })],
                    ..Default::default()
                },
            )
            .expect_err("no accessor publishes script bindings on the host products payload");
        assert_eq!(
            refusal,
            SvelteHostAdmissionRefusal::UnproducibleDemand(
                SvelteHostUnproducibleDemand::AnalysisScriptBindings
            )
        );
    }

    #[test]
    fn every_unroutable_svelte_option_refuses_at_issuance_on_both_entry_points() {
        use crate::compile_request::svelte::SvelteNamespaceRequest;
        let artifact = svelte_artifact(COMPONENT);
        let variants: [(SvelteOptionAttempt, SvelteHostUnproducibleDemand); 4] = [
            (
                SvelteOptionAttempt {
                    css: Some(SvelteCssRequest::Injected),
                    ..Default::default()
                },
                SvelteHostUnproducibleDemand::CssMode,
            ),
            (
                SvelteOptionAttempt {
                    custom_element_descriptor: Some(SvelteCustomElementDescriptor::default()),
                    ..Default::default()
                },
                SvelteHostUnproducibleDemand::CustomElementDescriptor,
            ),
            (
                SvelteOptionAttempt {
                    compatibility: Some(Default::default()),
                    ..Default::default()
                },
                SvelteHostUnproducibleDemand::Compatibility,
            ),
            (
                SvelteOptionAttempt {
                    namespace: Some(SvelteNamespaceRequest::Foreign),
                    ..Default::default()
                },
                SvelteHostUnproducibleDemand::ForeignNamespace,
            ),
        ];
        for (options, expected) in variants {
            let refusal = SvelteHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    SvelteHostMultiProductDemand {
                        products: vec![CompileProduct::RuntimeClient(
                            RuntimeProductRequest::default(),
                        )],
                        svelte_options: options.clone(),
                        ..Default::default()
                    },
                )
                .expect_err("an admitted-but-unroutable option must refuse, not drop");
            assert_eq!(
                refusal,
                SvelteHostAdmissionRefusal::UnproducibleDemand(expected)
            );
            let refusal = SvelteHostIntegrationBackend::new()
                .admit_runtime_render(
                    &artifact,
                    SvelteHostRuntimeRenderDemand {
                        svelte_options: options,
                        ..Default::default()
                    },
                )
                .expect_err("the render demand validates the same producibility class");
            assert_eq!(
                refusal,
                SvelteHostAdmissionRefusal::UnproducibleDemand(expected)
            );
        }
    }

    #[test]
    fn unsupported_option_rows_refuse_as_request_construction_on_both_entry_points() {
        // The unconditionally-unsupported rows (here: hmr) and the
        // `SVELTE-MODULE`-gated pair (here: generate_module) refuse at
        // canonical request construction, not the producibility layer.
        let artifact = svelte_artifact(COMPONENT);
        for options in [
            SvelteOptionAttempt {
                hmr: Some(true),
                ..Default::default()
            },
            SvelteOptionAttempt {
                generate_module: Some(true),
                ..Default::default()
            },
        ] {
            let refusal = SvelteHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    SvelteHostMultiProductDemand {
                        products: vec![CompileProduct::RuntimeClient(
                            RuntimeProductRequest::default(),
                        )],
                        svelte_options: options.clone(),
                        ..Default::default()
                    },
                )
                .expect_err("an unsupported option row refuses construction");
            assert!(
                matches!(
                    refusal,
                    SvelteHostAdmissionRefusal::RequestConstructionRefused(
                        CompileRequestError::UnsupportedOption { .. }
                    )
                ),
                "got {refusal:?}"
            );
            let refusal = SvelteHostIntegrationBackend::new()
                .admit_runtime_render(
                    &artifact,
                    SvelteHostRuntimeRenderDemand {
                        svelte_options: options,
                        ..Default::default()
                    },
                )
                .expect_err("the render demand refuses the same class");
            assert!(matches!(
                refusal,
                SvelteHostAdmissionRefusal::RequestConstructionRefused(
                    CompileRequestError::UnsupportedOption { .. }
                )
            ));
        }
        // The refusal carries the exact option row.
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeClient(
                        RuntimeProductRequest::default(),
                    )],
                    svelte_options: SvelteOptionAttempt {
                        hmr: Some(true),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect_err("refuses");
        match refusal {
            SvelteHostAdmissionRefusal::RequestConstructionRefused(
                CompileRequestError::UnsupportedOption { option, .. },
            ) => {
                assert_eq!(
                    option,
                    crate::compile_request::FrameworkOption::Svelte(
                        SvelteOption::CompileOptionsHmr
                    )
                );
            }
            other => panic!("expected the exact option row, got {other:?}"),
        }
    }

    #[test]
    fn inline_runtime_demand_refuses_at_request_construction() {
        // `inline` is a Vue-only axis; Svelte's canonical request refuses
        // it at construction (never a silently-ignored field).
        let artifact = svelte_artifact(COMPONENT);
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                        inline: Some(true),
                        ..Default::default()
                    })],
                    ..Default::default()
                },
            )
            .expect_err("inline is a Vue-only axis");
        assert_eq!(
            refusal,
            SvelteHostAdmissionRefusal::RequestConstructionRefused(
                CompileRequestError::InlineSsrUnsupported
            )
        );
    }

    #[test]
    fn admission_is_issued_only_over_a_svelte_parse() {
        let foreign = parse_registered_source_for_tests(
            FileLanguage::vue(),
            CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap(),
            "<template><p>a</p></template>",
        );
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_host_products(&foreign, multi_demand())
            .expect_err("a Vue artifact composes no Svelte parse admission");
        assert_eq!(refusal, SvelteHostAdmissionRefusal::NotASvelteParse);
        let refusal = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&foreign, SvelteHostRuntimeRenderDemand::default())
            .expect_err("the render demand composes the same parse admission");
        assert_eq!(refusal, SvelteHostAdmissionRefusal::NotASvelteParse);
    }

    #[test]
    fn one_admitted_request_populates_prerequisites_once() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let runtime_before = runtime_backend_delegation_count();
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let products = SvelteHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the admitted multi-product compile produces");

        // Parse reuse is structural: execution consumes the admitted
        // artifact directly and the shared orchestration has no parse
        // entry, so only the three per-thread producer counters below can
        // move.
        assert_eq!(
            runtime_backend_delegation_count(),
            runtime_before + 1,
            "exactly one runtime-backend population for the whole request \
             — the Main module AND its style side-products come from it"
        );
        assert_eq!(
            take_projection_producer_invocations(),
            1,
            "exactly one projection population for the whole request"
        );
        assert_eq!(
            take_template_facts_producer_invocations(),
            1,
            "exactly one template-fact population for the whole request"
        );
        let bundle = products
            .runtime_client_bundle()
            .expect("the client runtime bundle publishes");
        let staged = bundle
            .main
            .as_ref()
            .expect("the self-contained Main module comes from the one population");
        assert_eq!(
            staged.root().name(),
            "main",
            "the one population stages the self-contained module as the root artifact"
        );
        assert!(
            !bundle.qualified_styles.is_empty(),
            "the scoped-css side-product rides the SAME population — no \
             second compile produced it"
        );
        assert!(
            products.runtime_server_bundle().is_none(),
            "the server accessor never serves a bundle whose ssr compile did not run"
        );
        assert!(products.ide_companion().is_some());
        assert!(products.template_facts().is_some());
    }

    #[test]
    fn render_execution_is_render_only() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let rendered = SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the render-only compile produces");

        assert!(
            rendered.runtime_bundle().has_runtime_surface(),
            "the render handoff carries the runtime main surface"
        );
        assert!(
            rendered
                .runtime_bundle()
                .main
                .as_ref()
                .is_some_and(|staged| staged.code().contains("svelte/internal/client")),
            "the rendered Main is the self-contained client module"
        );
        assert!(rendered.runtime_bundle().tsx.is_none());
        assert!(rendered.runtime_bundle().template_data.is_none());
        assert_eq!(
            take_projection_producer_invocations(),
            0,
            "the render lane never runs the projection producer"
        );
        assert_eq!(
            take_template_facts_producer_invocations(),
            0,
            "the render lane never runs the template-fact producer"
        );
    }

    #[test]
    fn entry_points_refuse_the_other_demands_admission() {
        let artifact = svelte_artifact(COMPONENT);
        let render = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend::new()
            .compile_host_products(
                render,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("a render admission does not admit the multi-product entry");
        assert!(matches!(
            refusal,
            SvelteHostCompileRefusal::WrongDemand {
                expected: SvelteAdmittedDemand::HostMultiProduct,
                actual: SvelteAdmittedDemand::RuntimeRender,
            }
        ));

        let multi = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let refusal = SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                multi,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("a multi-product admission does not admit the render entry");
        assert!(matches!(
            refusal,
            SvelteHostCompileRefusal::WrongDemand {
                expected: SvelteAdmittedDemand::RuntimeRender,
                actual: SvelteAdmittedDemand::HostMultiProduct,
            }
        ));
    }

    #[test]
    fn admission_binds_to_the_exact_admitted_parse() {
        let artifact = svelte_artifact(COMPONENT);
        let other = svelte_artifact("<p>other</p>\n");
        let admission = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &other,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("an admission never executes against a different parse");
        assert!(matches!(
            refusal,
            SvelteHostCompileRefusal::AdmissionParseMismatch
        ));
    }

    #[test]
    fn publication_payloads_gate_on_the_admitted_product_set() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                SvelteHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeClient(
                        RuntimeProductRequest::default(),
                    )],
                    ..Default::default()
                },
            )
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let products = SvelteHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect("produces");
        assert!(products.runtime_client_bundle().is_some());
        assert!(products.runtime_server_bundle().is_none());
        assert!(
            products.ide_companion().is_none(),
            "an unadmitted product never publishes"
        );
        assert!(products.template_facts().is_none());
    }

    const SNIPPET_COMPONENT: &str =
        "<script>let c = $state(true);</script>\n{#snippet foo()}<p>{c}</p>{/snippet}\n";

    #[test]
    fn runtime_surface_refusal_classifies_through_admission_and_is_atomic() {
        // A `{#snippet}` declaration is an unsupported runtime surface: the
        // admitted multi-product compile (runtime + IDE + analysis) fails
        // CLOSED with the precise typed surface, and NO sibling product
        // publishes after the refusal — the refusal carries no payload at
        // all, so the atomicity is structural.
        let artifact = svelte_artifact(SNIPPET_COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("issuance is producibility validation, not source support");
        let _ = take_projection_producer_invocations();
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("the transaction is all-or-none");
        match refusal {
            SvelteHostCompileRefusal::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
                ..
            } => {
                assert!(
                    diagnostic_code.starts_with("svelte-runtime-unsupported-"),
                    "the refusal names the precise unsupported surface, got {diagnostic_code:?}"
                );
                assert!(!message.is_empty(), "the refusal carries a reason");
            }
            other => panic!("expected a runtime-surface refusal, got {other:?}"),
        }
        assert_eq!(
            take_projection_producer_invocations(),
            0,
            "the refusal returned before the IDE projection leg ran — no \
             sibling product was produced, let alone published"
        );
    }

    #[test]
    fn generic_compile_route_never_consults_the_host_backend() {
        let artifact = svelte_artifact(COMPONENT);
        let alloc = oxc_allocator::Allocator::new();
        let before = host_backend_execution_count();
        let opts = RuntimeCompileOptions::default();
        let outcome = svelte_carrier_bundle(
            COMPONENT,
            &artifact,
            &opts,
            &alloc,
            crate::framework_common::carrier_compiler::registry_route_execution_grants(&opts),
        )
        .expect("the shared bundle orchestration still compiles directly");
        assert!(matches!(outcome, CarrierCompileOutcome::Produced(_)));
        assert_eq!(
            host_backend_execution_count(),
            before,
            "reaching the shared orchestration directly must not execute the host backend"
        );

        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = SvelteHostIntegrationBackend::new().compile_runtime_render(
            admission,
            &artifact,
            &SvelteHostExecutionInputs::default(),
            &alloc,
        );
        assert_eq!(
            host_backend_execution_count(),
            before + 1,
            "the backend entry is what increments the execution count"
        );
    }

    /// Per-leg source-map faithfulness: each admitted product's OWN map
    /// flag drives only its own leg.
    #[test]
    fn source_map_demands_stay_per_leg() {
        let artifact = svelte_artifact(COMPONENT);
        let alloc = oxc_allocator::Allocator::new();
        let compile = |runtime_map: bool, ide_map: bool| {
            let admission = SvelteHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    SvelteHostMultiProductDemand {
                        products: vec![
                            CompileProduct::RuntimeClient(RuntimeProductRequest {
                                runtime_source_map: runtime_map,
                                ..Default::default()
                            }),
                            CompileProduct::IdeCompanion(IdeProductRequest {
                                want_source_map: ide_map,
                                ..Default::default()
                            }),
                        ],
                        filename: Some("App.svelte".to_string()),
                        ..Default::default()
                    },
                )
                .expect("admits");
            SvelteHostIntegrationBackend::new()
                .compile_host_products(
                    admission,
                    &artifact,
                    &SvelteHostExecutionInputs::default(),
                    &alloc,
                )
                .expect("produces")
        };

        let products = compile(true, false);
        let main = &products
            .runtime_client_bundle()
            .expect("client bundle")
            .main;
        assert!(
            main.as_ref()
                .is_some_and(|staged| staged.source_map().is_some()),
            "runtime_source_map=true must populate the runtime leg's own map"
        );
        assert!(
            products.ide_companion().expect("ide").source_map.is_empty(),
            "want_source_map=false must keep the IDE leg's map OFF even \
             though the runtime leg demanded one"
        );

        let products = compile(false, true);
        let main = &products
            .runtime_client_bundle()
            .expect("client bundle")
            .main;
        assert!(
            main.as_ref()
                .is_some_and(|staged| staged.source_map().is_none()),
            "an IDE-only map demand must not switch the runtime map on"
        );
        assert!(
            !products.ide_companion().expect("ide").source_map.is_empty(),
            "want_source_map=true must populate the IDE leg's own map"
        );
    }

    #[test]
    fn css_hash_override_execution_input_reaches_the_scoped_style_plan() {
        // The host-resolved `cssHash` result threads verbatim into the
        // scope class — an execution input beside the admitted request,
        // never part of admission identity.
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let rendered = SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &SvelteHostExecutionInputs {
                    css_hash_override: Some("verter-override-1".to_string()),
                    ..Default::default()
                },
                &alloc,
            )
            .expect("produces");
        let style = rendered
            .runtime_bundle()
            .qualified_styles
            .first()
            .expect("the scoped style side-product");
        assert!(
            style.result.code().contains("verter-override-1"),
            "the resolved cssHash override is the scope class, got: {}",
            style.result.code()
        );
    }

    /// A block authored in a preprocessor dialect, and the plain CSS its
    /// preprocessor produced. The authored top-level `$tone: red;` is not
    /// CSS, so any stage that reads the authored block instead of the
    /// produced bytes fails the compile.
    const SCSS_COMPONENT: &str = "<div class=\"card\">x</div>\n<style lang=\"scss\">$tone: red;\n.card { color: $tone; }</style>\n";
    /// The same component with a plain-css block: it needs no continuation,
    /// so its output declares the carrier source itself.
    const CSS_COMPONENT: &str =
        "<div class=\"card\">x</div>\n<style>.card { color: red; }</style>\n";
    const PRODUCED: &str = ".card { color: red; }";
    /// The host-minted identity of the produced bytes. Opaque to the
    /// compiler, so the tests state a recognisable value and assert the
    /// published style declares exactly it.
    const HOST_PRODUCED_SPACE: &str = "host-space:produced-css";
    const HOST_PRODUCED_ARTIFACT: &str = "host-artifact:produced-css";

    fn authored_style_basis(source: &str) -> crate::assembly::ContentId {
        let parsed = crate::svelte::parse_svelte(source);
        let content = parsed.styles[0]
            .content
            .expect("the style block has a body");
        crate::assembly::ContentId::from_content_bytes(
            &source.as_bytes()[content.start as usize..content.end as usize],
        )
    }

    fn supplied_style(
        consumed_basis: Option<crate::assembly::ContentId>,
        parsed: Option<crate::style_planner::PreparedStyleIr>,
    ) -> SvelteSuppliedStyle {
        SvelteSuppliedStyle {
            producer: verter_css_syntax::PreprocessorIdentity::Named(
                verter_css_syntax::ExternalStyleProducer::new("sass", Some("1.77.0"), None)
                    .expect("named producer"),
            ),
            code: Arc::from(PRODUCED),
            source_space_token: HOST_PRODUCED_SPACE.to_string(),
            content_artifact_token: HOST_PRODUCED_ARTIFACT.to_string(),
            source_map: None,
            diagnostics: Vec::new(),
            parsed,
            consumed_basis,
        }
    }

    /// The diagnostics the producing tool reported survive the continuation
    /// boundary: they ride the published stylesheet in production order, at
    /// their stated severity and in the space each names, rather than being
    /// replaced by an empty list when the result is bound.
    #[test]
    fn a_continued_style_publishes_its_producers_diagnostics() {
        use verter_css_syntax::{StyleDiagnostic, StyleDiagnosticSeverity, StyleStage};

        let reported = vec![
            StyleDiagnostic::with_severity(
                StyleStage::Authored,
                StyleDiagnosticSeverity::Warning,
                "deprecated division",
                None,
            ),
            StyleDiagnostic::new(
                StyleStage::Preprocessed,
                "suspicious selector",
                Some(verter_span::Span::new(0, 5)),
            ),
        ];
        let supplied = SvelteSuppliedStyle {
            diagnostics: reported.clone(),
            ..supplied_style(Some(authored_style_basis(SCSS_COMPONENT)), None)
        };
        let bundle = render_with_styles(SCSS_COMPONENT, vec![Some(supplied)])
            .expect("a continued block compiles");
        let style = bundle
            .qualified_styles
            .first()
            .expect("the scoped stylesheet");
        assert_eq!(
            style.result.diagnostics(),
            reported.as_slice(),
            "the producer's diagnostics reach the published style"
        );

        let authored =
            render_with_styles(CSS_COMPONENT, Vec::new()).expect("an authored css block compiles");
        assert!(
            authored.qualified_styles[0].result.diagnostics().is_empty(),
            "an authored block has no producer to report"
        );
    }

    fn render_with_styles(
        source: &str,
        supplied_styles: Vec<Option<SvelteSuppliedStyle>>,
    ) -> Result<RuntimeCompileOutput, SvelteHostCompileRefusal> {
        render_with_styles_for(
            source,
            SvelteHostRuntimeRenderDemand::default(),
            supplied_styles,
        )
    }

    fn render_with_styles_for(
        source: &str,
        demand: SvelteHostRuntimeRenderDemand,
        supplied_styles: Vec<Option<SvelteSuppliedStyle>>,
    ) -> Result<RuntimeCompileOutput, SvelteHostCompileRefusal> {
        let artifact = svelte_artifact(source);
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, demand)
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &SvelteHostExecutionInputs {
                    supplied_styles,
                    ..Default::default()
                },
                &alloc,
            )
            .map(|rendered| rendered.bundle)
    }

    #[test]
    fn a_supplied_style_result_is_the_only_body_its_block_scopes() {
        use verter_css_syntax::StyleStage;

        let bundle = render_with_styles(
            SCSS_COMPONENT,
            vec![Some(supplied_style(
                Some(authored_style_basis(SCSS_COMPONENT)),
                None,
            ))],
        )
        .expect("a continued block compiles");
        let style = bundle
            .qualified_styles
            .first()
            .expect("the scoped stylesheet");
        let css = style.result.code();
        assert!(
            css.contains(".card.svelte-") && css.contains("color: red"),
            "the produced bytes are the scoped body: {css}"
        );
        assert!(
            !css.contains("$tone"),
            "the authored block is never scoped in its result's place: {css}"
        );
        assert_eq!(style.consumed_stage, StyleStage::Preprocessed);
        assert_eq!(style.result.stage(), StyleStage::FrameworkRewritten);
        let hash = style.scope_hash.as_deref().expect("scoped by class");
        let main = bundle.main.as_ref().expect("the main module").code();
        assert!(
            main.contains(&format!("card {hash}")),
            "the markup carries the continued stylesheet's scope class: {main}"
        );

        // Without its result the authored block never enters scoping.
        let refusal = render_with_styles(SCSS_COMPONENT, Vec::new())
            .expect_err("an authored preprocessor dialect cannot be scoped");
        assert!(
            matches!(
                refusal,
                SvelteHostCompileRefusal::RuntimeSurfaceRefused { .. }
            ),
            "{refusal:?}"
        );
    }

    /// A continued block's published output declares the HOST's identity for
    /// the produced bytes it rendered from. Minting a carrier space over the
    /// same bytes here would address them under an identity nothing else in
    /// the host holds, so the host's produced-to-authored map chain could no
    /// longer be joined to this one.
    #[test]
    fn a_continued_style_declares_the_hosts_produced_space_not_a_minted_one() {
        use crate::framework_common::carrier_compiler::RuntimeOutputDescriptor;

        let bundle = render_with_styles(
            SCSS_COMPONENT,
            vec![Some(supplied_style(
                Some(authored_style_basis(SCSS_COMPONENT)),
                None,
            ))],
        )
        .expect("a continued block compiles");
        let declared = &bundle
            .qualified_styles
            .first()
            .expect("the scoped stylesheet")
            .output_descriptor
            .source_map
            .declared_space_tokens;
        assert_eq!(
            declared.as_slice(),
            &[HOST_PRODUCED_SPACE.to_string()],
            "the continued css map declares the host's produced space"
        );
        let (minted, _) = RuntimeOutputDescriptor::carrier_source(PRODUCED);
        assert_ne!(
            declared.first().map(String::as_str),
            Some(minted.as_str()),
            "a space minted over the produced bytes is not an identity the host knows"
        );

        // An AUTHORED block has no host-supplied space: it still declares the
        // carrier source it actually rendered from.
        let authored =
            render_with_styles(CSS_COMPONENT, Vec::new()).expect("an authored css block compiles");
        let (carrier, _) = RuntimeOutputDescriptor::carrier_source(CSS_COMPONENT);
        assert_eq!(
            authored
                .qualified_styles
                .first()
                .expect("the scoped stylesheet")
                .output_descriptor
                .source_map
                .declared_space_tokens
                .as_slice(),
            &[carrier],
            "an authored block declares its carrier source"
        );
    }

    /// A continued block's css map reaches the AUTHORED block through the
    /// host's produced-to-authored map. The bare render map addresses the
    /// produced bytes under the carrier's name, so publishing it would label
    /// preprocessor-output positions as `.svelte` ones; with no host map there
    /// is no honest map, and none publishes.
    #[test]
    fn a_continued_style_map_is_chained_through_the_hosts_produced_to_authored_map() {
        use oxc_sourcemap::{SourceMap, SourceMapBuilder};

        const AUTHORED_BODY: &str = "$tone: red;\n.card { color: $tone; }";
        // The produced `.card` (line 0, column 0) was made from authored
        // line 1, column 0.
        let host_map = {
            let mut builder = SourceMapBuilder::default();
            let id = builder.add_source_and_content("Card.scss", AUTHORED_BODY);
            builder.add_token(0, 0, 1, 0, Some(id), None);
            builder.into_sourcemap().to_json_string()
        };
        let demand = || SvelteHostRuntimeRenderDemand {
            runtime: RuntimeProductRequest {
                runtime_source_map: true,
                ..Default::default()
            },
            filename: Some("Card.svelte".to_string()),
            ..Default::default()
        };
        let continued = |source_map: Option<&str>| {
            let supplied = SvelteSuppliedStyle {
                source_map: source_map.map(Arc::from),
                ..supplied_style(Some(authored_style_basis(SCSS_COMPONENT)), None)
            };
            render_with_styles_for(SCSS_COMPONENT, demand(), vec![Some(supplied)])
                .expect("a continued block compiles")
        };
        let sources = |json: &str| -> Vec<String> {
            SourceMap::from_json_string(json)
                .expect("a published map is valid")
                .get_sources()
                .map(str::to_string)
                .collect()
        };

        let bundle = continued(Some(&host_map));
        let style = bundle
            .qualified_styles
            .first()
            .expect("the scoped stylesheet");
        let json = style
            .source_map
            .as_deref()
            .expect("a demanded map is chained through the host's");
        assert_eq!(
            sources(json),
            ["Card.scss"],
            "the chained map names the authored source, not the carrier: {json}"
        );
        let css = style.result.code();
        let offset = css.find(".card").expect("the rendered selector");
        let line = css[..offset].matches('\n').count() as u32;
        let column = (offset - css[..offset].rfind('\n').map_or(0, |at| at + 1)) as u32;
        let map = SourceMap::from_json_string(json).expect("a published map is valid");
        let table = map.generate_lookup_table();
        let token = map
            .lookup_token(&table, line, column)
            .expect("the rendered selector is mapped");
        assert_eq!(
            (token.get_src_line(), token.get_src_col()),
            (1, 0),
            "the rendered selector resolves to its authored position"
        );

        let unmapped = continued(None);
        assert!(
            unmapped
                .qualified_styles
                .first()
                .expect("the scoped stylesheet")
                .source_map
                .is_none(),
            "without the host's map a continued block publishes no map, never the bare render map"
        );

        // Control: the demand is live, and an authored block's own render
        // map is published naming its carrier.
        let authored = render_with_styles_for(CSS_COMPONENT, demand(), Vec::new())
            .expect("an authored css block compiles");
        let authored_map = authored
            .qualified_styles
            .first()
            .expect("the scoped stylesheet")
            .source_map
            .as_deref()
            .expect("an authored block publishes its render map");
        assert_eq!(sources(authored_map), ["Card.svelte"]);
    }

    #[test]
    fn a_supplied_style_result_that_does_not_describe_its_block_refuses_the_compile() {
        let postcss = "<div class=\"card\">x</div>\n<style lang=\"postcss\">$tone: red;\n.card { color: $tone; }</style>\n".to_string();
        let stale = crate::assembly::ContentId::from_content_bytes(b".card { color: $other; }");
        let cases = [
            (
                "stale basis",
                SCSS_COMPONENT.to_string(),
                vec![Some(supplied_style(Some(stale), None))],
            ),
            (
                "missing basis",
                SCSS_COMPONENT.to_string(),
                vec![Some(supplied_style(None, None))],
            ),
            (
                "unknown authored dialect",
                postcss.clone(),
                vec![Some(supplied_style(
                    Some(authored_style_basis(&postcss)),
                    None,
                ))],
            ),
            (
                "no block at the slot",
                SCSS_COMPONENT.to_string(),
                vec![
                    None,
                    Some(supplied_style(
                        Some(authored_style_basis(SCSS_COMPONENT)),
                        None,
                    )),
                ],
            ),
        ];
        for (case, source, supplied) in cases {
            let refusal = render_with_styles(&source, supplied).expect_err(case);
            assert!(
                matches!(
                    &refusal,
                    SvelteHostCompileRefusal::RuntimeSurfaceRefused { diagnostic_code, .. }
                        if diagnostic_code == STYLE_CONTINUATION_REFUSED
                ),
                "{case}: {refusal:?}"
            );
        }
    }

    #[test]
    fn a_continued_block_parses_its_produced_bytes_at_most_once() {
        let basis = authored_style_basis(SCSS_COMPONENT);
        let hint = crate::style_planner::prepare_supplied_style(
            verter_css_syntax::PreprocessedStyle::admitted(
                PRODUCED,
                verter_css_syntax::PreprocessorIdentity::Anonymous,
            ),
        )
        .expect("the produced css parses");
        let parses = |parsed: Option<crate::style_planner::PreparedStyleIr>| {
            let before = verter_css_syntax::parse_style_ir_thread_invocations();
            render_with_styles(
                SCSS_COMPONENT,
                vec![Some(supplied_style(Some(basis.clone()), parsed))],
            )
            .expect("a continued block compiles");
            verter_css_syntax::parse_style_ir_thread_invocations() - before
        };
        assert_eq!(
            parses(Some(hint)),
            0,
            "the host's parse of the produced bytes is reused"
        );
        assert_eq!(
            parses(None),
            1,
            "without it, the produced bytes parse exactly once"
        );
    }

    /// Source-byte binding is structural: the execution entries take no
    /// independent source parameter, so an admission over parse A can never
    /// execute against foreign bytes B — the artifact's own registered
    /// carrier source (the bytes its parse identity was computed from) is
    /// the only byte authority the seam can reach.
    #[test]
    fn execution_source_is_the_admitted_artifacts_own_registered_bytes() {
        let artifact = svelte_artifact(COMPONENT);
        assert_eq!(
            artifact.carrier_source().as_ref(),
            COMPONENT,
            "the artifact's carrier source is the exact registered bytes"
        );
        let admission = SvelteHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let rendered = SvelteHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &SvelteHostExecutionInputs::default(),
                &alloc,
            )
            .expect("executes over the artifact's own bytes");
        assert!(rendered.runtime_bundle().has_runtime_surface());
    }

    #[test]
    fn semantic_admission_requires_the_witnessed_parse() {
        let artifact = svelte_artifact(COMPONENT);
        let other = svelte_artifact("<p>other</p>\n");
        let parse = SvelteCarrierFrontend
            .admit_registered(&artifact)
            .expect("admits");
        assert!(
            SvelteSemanticAuthority
                .admit_over_parse(&parse, &artifact)
                .is_some(),
            "the witnessed parse composes over its own artifact"
        );
        assert!(
            SvelteSemanticAuthority
                .admit_over_parse(&parse, &other)
                .is_none(),
            "a parse admission witnessed over one artifact never composes \
             a semantic admission over another"
        );
    }
}
