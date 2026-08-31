//! Vue [`FrameworkHostIntegrationBackend`] adapter for the native host.
//!
//! Composes the Vue parse and semantic admissions into the ONE
//! [`VueCompileAdmission`] token, coordinates one canonical multi-product
//! [`CompileRequest`] per admitted demand, and drives the shared Vue bundle
//! orchestration (ordered runtime / IDE-projection / template-fact
//! capability calls with one prerequisite population). Catalog lookup keys
//! adapter × epoch × host epoch × HostIntegration.
//!
//! Demand specificity lives in the admission's VALUE (the admitted demand
//! plus the requested product set): there is exactly one admission token
//! type for this host epoch, never product-scoped siblings. Capability
//! validation is demand-specific — a runtime-render demand never requires
//! projection capability — and a missing required capability is a typed
//! refusal, never a fallback onto another lane or framework.

use std::sync::Arc;
use std::sync::OnceLock;

use verter_language::{FrameworkAdapterId, LanguageId, ParseKey};

use crate::compile_request::{
    AnalysisProductRequest, CompileProduct, CompileRequest, CompileRequestError,
    FrameworkCompileRequest, ProductKind, RuntimeProductRequest, VueOption, VueOptionAttempt,
};
use crate::standalone::registered_runtime_for;

use super::capability::{
    FrameworkHostIntegrationBackend, HostEpoch, NativeHostEpoch, Present, ProductExecutionGrant,
    ProductExecutionGrants,
};
use super::carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeOutput, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeDiagnostic,
};
use super::catalog::{
    CatalogCapability, CatalogIdentity, CatalogRow, HostCap, ImmutableCapabilityCatalog,
    TypedCapabilityRegistration,
};
use super::registered_carrier_projection::{
    registered_projection_for, registered_semantic_for, TemplateFactsProduct,
};
use super::vue_bridge::{vue_carrier_bundle, VueCarrierCompiler};
use super::vue_carrier_frontend::{VueCarrierFrontend, VueParseAdmission, VueSfcV3};
use super::vue_semantic_authority::{VueSemanticAdmission, VueSemanticAuthority};
use super::{CarrierCompiler, FrameworkParseArtifact};

/// Vue host-integration backend for the native host epoch.
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
pub struct VueHostIntegrationBackend {
    _registered: (),
}

impl VueHostIntegrationBackend {
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
        static REGISTERED: VueHostIntegrationBackend = VueHostIntegrationBackend::new();
        &REGISTERED
    }
}

// The backends are sealed identities, never duplicable service values.
static_assertions::assert_not_impl_any!(VueHostIntegrationBackend: Clone, Copy, Default);

/// Host-backed multi-product demand: every requested product plus the
/// typed Vue option attempt the backend turns into the one canonical
/// request. Request construction is the backend's, not the caller's.
#[derive(Debug, Clone, Default)]
pub struct VueHostMultiProductDemand {
    /// The requested product set (runtime, IDE companion, analysis, ...).
    pub products: Vec<CompileProduct>,
    /// Typed Vue option attempt; unsupported options refuse at issuance.
    pub vue_options: VueOptionAttempt,
    /// Carrier file name for component-name + source-map identity.
    pub filename: Option<String>,
    /// Explicit component / scope id for scoped-style hashing.
    pub component_id: Option<String>,
    /// Production mode.
    pub is_production: bool,
    /// Force JavaScript output.
    pub force_js: bool,
}

/// Runtime-render (render-only) demand: the backend constructs the
/// runtime-only product set itself; no other product can ride along.
#[derive(Debug, Clone, Default)]
pub struct VueHostRuntimeRenderDemand {
    /// Runtime product options for the rendered main module.
    pub runtime: RuntimeProductRequest,
    /// Demand the template-fact producer's DIAGNOSTICS companion beside
    /// the render: the producer is the only pass that parses template
    /// directive/interpolation expressions on this route, so a render lane
    /// that must fail closed on a malformed expression demands this. The
    /// fact PAYLOAD never publishes on the render handoff — the companion
    /// is diagnostics-only ([`VueHostRenderedMain`]).
    pub template_fact_diagnostics: bool,
    /// Typed Vue option attempt; `ssr` selects the server runtime product.
    pub vue_options: VueOptionAttempt,
    /// Carrier file name for component-name + source-map identity.
    pub filename: Option<String>,
    /// Explicit component / scope id for scoped-style hashing.
    pub component_id: Option<String>,
    /// Production mode.
    pub is_production: bool,
    /// Force JavaScript output.
    pub force_js: bool,
}

/// Typed issuance refusal. Never a fallback to another lane, framework,
/// or compatibility compiler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VueHostAdmissionRefusal {
    /// The artifact is not a Vue parse this frontend admitted.
    NotAVueParse,
    /// No registered Vue semantic authority for the artifact's epoch.
    SemanticUnavailable,
    /// A demanded product's required capability has no registered row for
    /// the artifact's epoch.
    CapabilityUnavailable {
        /// The demanded product whose capability is missing.
        product: ProductKind,
        /// The missing capability.
        capability: CatalogCapability,
    },
    /// The demanded product has no Vue host production route.
    UnsupportedProduct(ProductKind),
    /// A demanded product/option shape is admissible by canonical request
    /// construction but has no production route through the host bundle
    /// execution — refused at issuance, never silently dropped or
    /// silently served by a different compile.
    UnproducibleDemand(VueHostUnproducibleDemand),
    /// Canonical request construction refused the demand.
    RequestConstructionRefused(CompileRequestError),
}

/// Why an admissible-looking demand cannot be PRODUCED by the host bundle
/// execution. Producibility validation, distinct from capability presence:
/// each variant names a demand shape whose compile would run with the
/// demanded axis dropped or substituted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueHostUnproducibleDemand {
    /// Both runtime kinds in one demand: the host bundle orchestration
    /// executes exactly one ssr mode per pass, so a dual-kind demand would
    /// serve one kind a bundle whose compile never ran. The canonical
    /// direct compile route remains the dual-kind path.
    DualRuntimeKind,
    /// A runtime product is demanded and the `vue_options.ssr` flag
    /// disagrees with its kind — execution derives its ssr mode from the
    /// product set, so honoring the flag and honoring the product would
    /// diverge. A demand carrying NO runtime product is not a mismatch:
    /// the derived mode drives only the runtime leg, which never runs.
    SsrFlagRuntimeKindMismatch,
    /// `AnalysisProductRequest.want_script_bindings`: execution never
    /// produces script bindings on this route and no accessor publishes
    /// them.
    AnalysisScriptBindings,
    /// A Vue option the bundle execution cannot represent or honor (parity
    /// target: the compatibility bundle route's option set).
    VueOption(VueOption),
}

/// Which demand an admission was issued for. Value-level demand
/// specificity — never a sibling token type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueAdmittedDemand {
    /// Host-backed multi-product demand.
    HostMultiProduct,
    /// Runtime-render (render-only) demand.
    RuntimeRender,
}

// Consume-once by-value evidence must never be duplicable or
// round-trippable through a serialized form: one issuance drives at most
// one execution.
static_assertions::assert_not_impl_any!(
    VueCompileAdmission: Clone, Copy, serde::Serialize, serde::Deserialize<'static>
);

/// The sole Vue compile-admission token for the native host epoch:
/// the admitted demand, the one canonical request, the exact parse
/// binding, and the composed parse + semantic admissions. It is not a
/// capability or service bag — execution re-selects capabilities from the
/// immutable catalog.
#[derive(Debug)]
pub struct VueCompileAdmission {
    demand: VueAdmittedDemand,
    request: CompileRequest,
    parse_key: Arc<ParseKey>,
    _parse: VueParseAdmission,
    _semantic: VueSemanticAdmission,
}

impl VueCompileAdmission {
    /// The admitted demand.
    #[must_use]
    pub fn demand(&self) -> VueAdmittedDemand {
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
/// bytes and resolved Vue facts).
#[derive(Debug, Clone, Default)]
pub struct VueHostExecutionInputs {
    /// Host-selected block bytes for supplied templates, scripts, styles.
    pub block_content: super::carrier_compiler::RuntimeBlockContentInputs,
    /// Host-resolved Vue cross-file inputs (macro DTO, style v-bind facts).
    pub vue_facts: Option<crate::compile::types::VueExecutionInputs>,
    /// Host-retained parsed style IRs in inventory order.
    pub prepared_styles: Vec<Option<crate::style_planner::PreparedStyleIr>>,
}

/// Typed execution refusal. All-or-none: a refusal publishes no product.
#[derive(Debug)]
pub enum VueHostCompileRefusal {
    /// The presented artifact is not the parse this admission was issued
    /// over.
    AdmissionParseMismatch,
    /// The admission was issued for the other demand kind.
    WrongDemand {
        /// The demand this entry point serves.
        expected: VueAdmittedDemand,
        /// The demand the admission actually admits.
        actual: VueAdmittedDemand,
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
        /// Non-fatal diagnostics collected before the refusal.
        diagnostics: Vec<RuntimeDiagnostic>,
    },
}

/// Per-product publication payloads of one admitted multi-product compile.
/// Every accessor gates on the admitted product set: a prerequisite that
/// was produced but not admitted is never published.
#[derive(Debug)]
pub struct VueHostCompiledProducts {
    admitted: Vec<ProductKind>,
    bundle: RuntimeCompileOutput,
}

impl VueHostCompiledProducts {
    fn admits(&self, kind: ProductKind) -> bool {
        self.admitted.contains(&kind)
    }

    /// The CLIENT runtime bundle publication payload (main module and its
    /// script / template / style / custom-block side-files), when the
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
}

/// Render-only handoff of one admitted runtime-render compile: the
/// runtime bundle the host assembles into the `Main` virtual module. No
/// IDE or analysis payload exists on this handoff — a demanded
/// template-fact companion contributes only its producer DIAGNOSTICS to
/// the bundle's channel; the fact payload itself is never published here.
#[derive(Debug)]
pub struct VueHostRenderedMain {
    bundle: RuntimeCompileOutput,
}

impl VueHostRenderedMain {
    /// The runtime bundle for host `Main` assembly.
    #[must_use]
    pub fn runtime_bundle(&self) -> &RuntimeCompileOutput {
        &self.bundle
    }
}

impl VueHostIntegrationBackend {
    /// Adapter this backend answers to.
    #[must_use]
    pub fn adapter_id(&self) -> FrameworkAdapterId {
        VueCarrierCompiler.adapter_id()
    }

    /// Carrier language this backend integrates.
    #[must_use]
    pub fn carrier_language_id(&self) -> LanguageId {
        VueCarrierCompiler.carrier_language_id()
    }

    /// Compose the parse + semantic admissions over the artifact — the
    /// shared first half of both issuance entry points.
    fn compose_admissions(
        &self,
        artifact: &FrameworkParseArtifact,
    ) -> Result<(VueParseAdmission, VueSemanticAdmission), VueHostAdmissionRefusal> {
        let parse = VueCarrierFrontend
            .admit_registered(artifact)
            .ok_or(VueHostAdmissionRefusal::NotAVueParse)?;
        let semantic = VueSemanticAuthority
            .admit_over_parse(&parse, artifact)
            .ok_or(VueHostAdmissionRefusal::SemanticUnavailable)?;
        Ok((parse, semantic))
    }

    #[allow(clippy::too_many_arguments)]
    fn issue(
        &self,
        demand: VueAdmittedDemand,
        products: Vec<CompileProduct>,
        vue_options: VueOptionAttempt,
        filename: Option<String>,
        component_id: Option<String>,
        is_production: bool,
        force_js: bool,
        parse: VueParseAdmission,
        semantic: VueSemanticAdmission,
    ) -> Result<VueCompileAdmission, VueHostAdmissionRefusal> {
        let vue = vue_options
            .into_request()
            .map_err(VueHostAdmissionRefusal::RequestConstructionRefused)?;
        let request = CompileRequest::new(
            products,
            FrameworkCompileRequest::Vue(vue),
            None,
            filename,
            component_id,
            is_production,
            force_js,
        )
        .map_err(VueHostAdmissionRefusal::RequestConstructionRefused)?;
        Ok(VueCompileAdmission {
            demand,
            request,
            // The binding is the parse admission's own witnessed key —
            // the exact identity `admit_registered` issued over.
            parse_key: Arc::clone(parse.parse_key()),
            _parse: parse,
            _semantic: semantic,
        })
    }

    /// Compile a host-backed multi-product admission through the shared
    /// orchestration, publishing per-product payloads gated on the
    /// admitted set. One admitted request populates its parse, semantic,
    /// projection, plan, and emit prerequisites once. Consumes the
    /// admission by value: one issuance drives at most one execution.
    pub fn compile_host_products(
        &self,
        admission: VueCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &VueHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<VueHostCompiledProducts, VueHostCompileRefusal> {
        if admission.demand != VueAdmittedDemand::HostMultiProduct {
            return Err(VueHostCompileRefusal::WrongDemand {
                expected: VueAdmittedDemand::HostMultiProduct,
                actual: admission.demand,
            });
        }
        let admitted = admission.admitted_products();
        let bundle = self.execute(admission, artifact, inputs, alloc)?;
        Ok(VueHostCompiledProducts { admitted, bundle })
    }

    /// Compile a runtime-render admission: the render-only handoff for
    /// host `Main` assembly. Never plans or publishes an IDE companion or
    /// template facts. Consumes the admission by value: one issuance
    /// drives at most one execution.
    pub fn compile_runtime_render(
        &self,
        admission: VueCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &VueHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<VueHostRenderedMain, VueHostCompileRefusal> {
        if admission.demand != VueAdmittedDemand::RuntimeRender {
            return Err(VueHostCompileRefusal::WrongDemand {
                expected: VueAdmittedDemand::RuntimeRender,
                actual: admission.demand,
            });
        }
        let mut bundle = self.execute(admission, artifact, inputs, alloc)?;
        // Diagnostics-only companion: a demanded template-fact pass has
        // already merged its producer diagnostics into the bundle's
        // channel; the fact payload never publishes on the render handoff.
        bundle.template_data = None;
        Ok(VueHostRenderedMain { bundle })
    }

    /// The one execution path both entry points share: verify the exact
    /// parse binding, derive the neutral options off the admitted request,
    /// and drive the shared Vue bundle orchestration over the artifact's
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
        admission: VueCompileAdmission,
        artifact: &FrameworkParseArtifact,
        inputs: &VueHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, VueHostCompileRefusal> {
        record_host_backend_execution();
        if artifact.parse_key() != admission.parse_key.as_ref() {
            return Err(VueHostCompileRefusal::AdmissionParseMismatch);
        }
        let source = artifact.carrier_source();
        let opts = derive_admitted_runtime_options(&admission.request, inputs);
        let grants = execution_grants_for_request(&admission.request);
        drop(admission);
        match vue_carrier_bundle(source, artifact, &opts, alloc, grants) {
            Ok(CarrierCompileOutcome::Produced(bundle)) => Ok(bundle),
            // All-or-none: a refused runtime surface publishes nothing;
            // sibling projection/analysis products never warm or publish
            // after the refusal.
            Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(refusal)) => {
                Err(VueHostCompileRefusal::RuntimeSurfaceRefused {
                    diagnostic_code: refusal.diagnostic_code,
                    message: refusal.message,
                    diagnostics: refusal.diagnostics,
                })
            }
            Err(unsupported) => Err(VueHostCompileRefusal::Unsupported(unsupported)),
        }
    }
}

impl FrameworkHostIntegrationBackend<VueSfcV3, NativeHostEpoch> for VueHostIntegrationBackend {
    type CompileAdmission = VueCompileAdmission;
    type ParseArtifact = FrameworkParseArtifact;
    type MultiProductDemand = VueHostMultiProductDemand;
    type RuntimeRenderDemand = VueHostRuntimeRenderDemand;
    type AdmissionRefusal = VueHostAdmissionRefusal;

    fn admit_host_products(
        &self,
        artifact: &FrameworkParseArtifact,
        demand: VueHostMultiProductDemand,
    ) -> Result<VueCompileAdmission, VueHostAdmissionRefusal> {
        let (parse, semantic) = self.compose_admissions(artifact)?;
        refuse_unproducible_products(&demand.products, demand.vue_options.ssr)?;
        refuse_unproducible_vue_options(&demand.vue_options)?;
        for product in &demand.products {
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
                    return Err(VueHostAdmissionRefusal::UnsupportedProduct(kind));
                }
            };
            if let Err(capability) = required {
                return Err(VueHostAdmissionRefusal::CapabilityUnavailable {
                    product: product.kind(),
                    capability,
                });
            }
        }
        self.issue(
            VueAdmittedDemand::HostMultiProduct,
            demand.products,
            demand.vue_options,
            demand.filename,
            demand.component_id,
            demand.is_production,
            demand.force_js,
            parse,
            semantic,
        )
    }

    fn admit_runtime_render(
        &self,
        artifact: &FrameworkParseArtifact,
        demand: VueHostRuntimeRenderDemand,
    ) -> Result<VueCompileAdmission, VueHostAdmissionRefusal> {
        let (parse, semantic) = self.compose_admissions(artifact)?;
        refuse_unproducible_vue_options(&demand.vue_options)?;
        // Demand-specific validation: the render lane requires ONLY the
        // runtime capability — projection is never consulted.
        if registered_runtime_for(artifact.adapter_id(), artifact.epoch()).is_none() {
            return Err(VueHostAdmissionRefusal::CapabilityUnavailable {
                product: if demand.vue_options.ssr {
                    ProductKind::RuntimeServer
                } else {
                    ProductKind::RuntimeClient
                },
                capability: CatalogCapability::Runtime,
            });
        }
        let mut products = vec![if demand.vue_options.ssr {
            CompileProduct::RuntimeServer(demand.runtime)
        } else {
            CompileProduct::RuntimeClient(demand.runtime)
        }];
        if demand.template_fact_diagnostics {
            // The diagnostics companion rides the canonical request as the
            // template-data analysis product (the semantic capability is
            // already composed above); the render handoff strips the fact
            // payload and keeps only the merged producer diagnostics.
            products.push(CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: false,
                want_template_data: true,
            }));
        }
        self.issue(
            VueAdmittedDemand::RuntimeRender,
            products,
            demand.vue_options,
            demand.filename,
            demand.component_id,
            demand.is_production,
            demand.force_js,
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
    inputs: &VueHostExecutionInputs,
) -> RuntimeCompileOptions {
    use crate::compile_request::VueBackendRequest;

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
    let vue = request.vue();

    let mut prepared_styles = inputs.prepared_styles.clone();
    for (index, slot) in inputs.block_content.styles.iter().enumerate() {
        if let Some(parsed) = slot.as_ref().and_then(|input| input.parsed.clone()) {
            if prepared_styles.len() <= index {
                prepared_styles.resize(index + 1, None);
            }
            prepared_styles[index] = Some(parsed);
        }
    }

    RuntimeCompileOptions {
        filename: request.filename().map(str::to_string),
        is_production: request.is_production(),
        custom_element: vue.and_then(|v| v.script_custom_element).unwrap_or(false),
        // Per-leg, read faithfully off each admitted product's OWN
        // request: the runtime flag drives only the runtime leg and the
        // IDE flag only the IDE leg — demanding a map on one never
        // switches it on for the other.
        source_map: runtime.is_some_and(|r| r.runtime_source_map),
        ide_source_map: Some(ide.is_some_and(|i| i.want_source_map)),
        ssr,
        runtime_module_name: vue.and_then(|v| v.runtime_module_name.clone()),
        component_id: request.component_id().map(str::to_string),
        force_js: request.force_js(),
        force_vapor: vue.is_some_and(|v| matches!(v.backend, VueBackendRequest::Vapor)),
        inline: runtime.and_then(|r| r.inline),
        comments: vue.and_then(|v| v.comments),
        delimiters: vue.and_then(|v| v.delimiters.clone()),
        custom_elements: vue
            .map(|v| v.is_custom_element.clone())
            .filter(|v| !v.is_empty()),
        want_runtime: runtime.is_some(),
        want_ide: ide.is_some(),
        want_template_data,
        types_module_name: ide.and_then(|i| i.types_module_name.clone()),
        embed_ambient_types: ide.is_some_and(|i| i.embed_ambient_types),
        conditional_root_narrowing: ide.is_some_and(|i| i.conditional_root_narrowing),
        strict_slots: ide.is_some_and(|i| i.strict_slots),
        block_content: inputs.block_content.clone(),
        vue_facts: inputs.vue_facts.clone(),
        prepared_styles,
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
    ssr_flag: bool,
) -> Result<(), VueHostAdmissionRefusal> {
    let wants_client = products
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeClient);
    let wants_server = products
        .iter()
        .any(|p| p.kind() == ProductKind::RuntimeServer);
    if wants_client && wants_server {
        return Err(VueHostAdmissionRefusal::UnproducibleDemand(
            VueHostUnproducibleDemand::DualRuntimeKind,
        ));
    }
    // The ssr flag has to agree with the product set only when a runtime
    // product is actually demanded. `derive_admitted_runtime_options`
    // reads the mode off the product set to drive the RUNTIME leg alone,
    // and reports `want_runtime: false` when no runtime product was
    // admitted — so with no runtime product the flag drives nothing and
    // cannot be a dropped or substituted axis. An IDE-only or
    // analysis-only demand therefore serves regardless of the flag, while
    // a demand that does carry a runtime product must agree with it or
    // execution would serve the other kind.
    if (wants_client || wants_server) && ssr_flag != wants_server {
        return Err(VueHostAdmissionRefusal::UnproducibleDemand(
            VueHostUnproducibleDemand::SsrFlagRuntimeKindMismatch,
        ));
    }
    if products
        .iter()
        .any(|p| matches!(p, CompileProduct::Analysis(a) if a.want_script_bindings))
    {
        return Err(VueHostAdmissionRefusal::UnproducibleDemand(
            VueHostUnproducibleDemand::AnalysisScriptBindings,
        ));
    }
    Ok(())
}

/// Producibility validation over the demanded VUE OPTIONS: every option
/// [`derive_admitted_runtime_options`] cannot route into the bundle
/// execution refuses at issuance instead of being silently dropped.
/// Parity target is the compatibility bundle route
/// ([`super::carrier_compiler::RuntimeCompileOptions`]): whatever it
/// honors for the same request is honored here; whatever it cannot
/// represent refuses. Deterministic declaration order.
fn refuse_unproducible_vue_options(
    options: &VueOptionAttempt,
) -> Result<(), VueHostAdmissionRefusal> {
    let unroutable: [(bool, VueOption); 14] = [
        (
            options.whitespace.is_some(),
            VueOption::ParserOptionsWhitespace,
        ),
        (
            options.hoist_static.is_some(),
            VueOption::TransformOptionsHoistStatic,
        ),
        (
            options.cache_handlers.is_some(),
            VueOption::TransformOptionsCacheHandlers,
        ),
        (options.hmr.is_some(), VueOption::TransformOptionsHmr),
        (
            options.optimize_imports.is_some(),
            VueOption::CodegenOptionsOptimizeImports,
        ),
        (
            options.ssr_runtime_module_name.is_some(),
            VueOption::CodegenOptionsSsrRuntimeModuleName,
        ),
        (options.parse_pad.is_some(), VueOption::ParsePad),
        (options.ignore_empty.is_some(), VueOption::ParseIgnoreEmpty),
        (
            !options.babel_parser_plugins.is_empty(),
            VueOption::CompileScriptBabelParserPlugins,
        ),
        (
            options.gen_default_as.is_some(),
            VueOption::CompileScriptGenDefaultAs,
        ),
        (
            options.props_destructure.is_some(),
            VueOption::CompileScriptPropsDestructure,
        ),
        (
            options.transform_asset_urls.is_some(),
            VueOption::CompileTemplateTransformAssetUrls,
        ),
        (options.style_trim.is_some(), VueOption::CompileStyleTrim),
        (
            options.css_modules.is_some(),
            VueOption::CompileStyleModules,
        ),
    ];
    for (present, option) in unroutable {
        if present {
            return Err(VueHostAdmissionRefusal::UnproducibleDemand(
                VueHostUnproducibleDemand::VueOption(option),
            ));
        }
    }
    Ok(())
}

/// Typed Vue host-integration catalog row for the native host epoch.
#[must_use]
pub fn vue_host_integration_registration(
) -> TypedCapabilityRegistration<HostCap<VueHostIntegrationBackend>> {
    TypedCapabilityRegistration::register_host_integration::<VueSfcV3, NativeHostEpoch, _>(
        VueHostIntegrationBackend::new().adapter_id(),
        VueHostIntegrationBackend::new().carrier_language_id(),
        Present(VueHostIntegrationBackend::new()),
    )
}

/// Installed host-integration row stored in the immutable host catalog.
///
/// The payload is the SAME registered backend value the typed
/// [`super::capability::FrameworkHostIntegrationBackend`] row carries —
/// the closed set of host-integration backends, one catalog for every
/// framework's row (mirrors the installed runtime-backend sum).
#[derive(Debug, PartialEq, Eq)]
pub enum InstalledHostIntegration {
    /// Vue host-integration backend (the registered instance).
    Vue(&'static VueHostIntegrationBackend),
    /// Svelte host-integration backend (the registered instance).
    Svelte(&'static super::svelte_host_integration::SvelteHostIntegrationBackend),
}

/// Frozen host-integration catalog. Built once from the host-integration
/// registration constructors; no insert after.
#[must_use]
pub fn built_in_host_integration_catalog(
) -> &'static ImmutableCapabilityCatalog<(), (), (), (), InstalledHostIntegration> {
    static CATALOG: OnceLock<ImmutableCapabilityCatalog<(), (), (), (), InstalledHostIntegration>> =
        OnceLock::new();
    CATALOG.get_or_init(|| {
        ImmutableCapabilityCatalog::try_from_rows([
            CatalogRow::from(vue_host_integration_registration().map_host_integration(|_| {
                InstalledHostIntegration::Vue(VueHostIntegrationBackend::registered())
            })),
            CatalogRow::from(
                super::svelte_host_integration::svelte_host_integration_registration()
                    .map_host_integration(|_| {
                        InstalledHostIntegration::Svelte(
                            super::svelte_host_integration::SvelteHostIntegrationBackend::registered(),
                        )
                    }),
            ),
        ])
        .expect("the built-in Vue and Svelte host-integration identities are unique")
    })
}

/// Catalog lookup for a registered host-integration backend by adapter ×
/// framework epoch × host epoch, returning the matched row's identity
/// beside the installed backend so a consumer carries single-catalog
/// identity and audit attribution. Unknown or mismatched identity returns
/// `None` — no framework fallback.
#[must_use]
pub fn registered_host_integration_for<HostE: HostEpoch>(
    adapter_id: &FrameworkAdapterId,
    epoch: &super::capability::FrameworkEpochId,
) -> Option<(&'static CatalogIdentity, &'static InstalledHostIntegration)> {
    built_in_host_integration_catalog().iter().find_map(|row| {
        let identity = row.identity();
        (identity.capability() == CatalogCapability::HostIntegration
            && identity.adapter_id() == adapter_id
            && identity.epoch() == epoch
            && identity
                .host_epoch()
                .is_some_and(|host| host.as_str() == HostE::ID))
        .then(|| row.host_integration().map(|backend| (identity, backend)))
        .flatten()
    })
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
    use super::super::HostEpochId;
    use super::*;
    use crate::compile_request::{IdeProductRequest, VueBackendRequest};
    use crate::standalone::runtime_backend_delegation_count;
    use verter_language::carrier_grammar::CarrierGrammarConfig;
    use verter_language::FileLanguage;

    fn vue_artifact(source: &str) -> Arc<FrameworkParseArtifact> {
        parse_registered_source_for_tests(
            FileLanguage::vue(),
            CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap(),
            source,
        )
    }

    const SFC: &str = "<script setup lang=\"ts\">const a: number = 1</script>\n<template><div>{{ a }}</div></template>";

    fn multi_demand() -> VueHostMultiProductDemand {
        VueHostMultiProductDemand {
            products: vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                CompileProduct::IdeCompanion(IdeProductRequest::default()),
                CompileProduct::Analysis(AnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
            filename: Some("App.vue".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn host_integration_catalog_registers_the_vue_native_row() {
        let catalog = built_in_host_integration_catalog();
        let identity = catalog
            .iter()
            .map(|row| row.identity())
            .find(|identity| identity.adapter_id() == &FrameworkAdapterId::vue())
            .expect("the catalog holds the Vue host-integration row");
        assert_eq!(identity.capability(), CatalogCapability::HostIntegration);
        assert_eq!(identity.adapter_id(), &FrameworkAdapterId::vue());
        assert_eq!(identity.epoch().as_str(), "vue");
        assert_eq!(
            identity.host_epoch(),
            Some(&HostEpochId::new(NativeHostEpoch::ID))
        );
        let (row_identity, installed) = registered_host_integration_for::<NativeHostEpoch>(
            &FrameworkAdapterId::vue(),
            identity.epoch(),
        )
        .expect("the native Vue host row resolves");
        assert_eq!(
            row_identity, identity,
            "the lookup returns the matched row's own identity"
        );
        assert!(
            matches!(installed, InstalledHostIntegration::Vue(_)),
            "the installed payload is the Vue arm of the one host catalog"
        );
    }

    /// One admission carves one consume-once grant per admitted
    /// product-backend leg, each admitting exactly its demand's kind —
    /// the sole out-of-crate source of execution grants.
    #[test]
    fn admission_carves_one_grant_per_admitted_product_leg() {
        let artifact = vue_artifact(SFC);
        let grants = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits")
            .into_execution_grants();
        let runtime = grants.runtime.expect("the runtime leg was admitted");
        assert!(runtime.admits(ProductKind::RuntimeClient));
        assert!(!runtime.admits(ProductKind::RuntimeServer));
        let projection = grants.projection.expect("the projection leg was admitted");
        assert!(projection.admits(ProductKind::IdeCompanion));
        assert!(!projection.admits(ProductKind::RuntimeClient));

        // A render-only admission carves NO projection grant: the carve is
        // demand-specific, never a blanket capability bag.
        let render_grants = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("admits")
            .into_execution_grants();
        assert!(render_grants.runtime.is_some());
        assert!(
            render_grants.projection.is_none(),
            "an unadmitted leg receives no execution grant"
        );
    }

    #[test]
    fn multi_product_admission_composes_one_canonical_request() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("a Vue parse with registered capabilities admits");
        assert_eq!(admission.demand(), VueAdmittedDemand::HostMultiProduct);
        assert_eq!(
            admission.admitted_products(),
            vec![
                ProductKind::RuntimeClient,
                ProductKind::IdeCompanion,
                ProductKind::Analysis
            ]
        );
        assert_eq!(admission.request().filename(), Some("App.vue"));
    }

    #[test]
    fn runtime_render_admission_is_runtime_only_and_never_consults_projection() {
        let artifact = vue_artifact(SFC);
        let before = projection_catalog_consult_count();
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("a Vue parse with a registered runtime backend admits");
        assert_eq!(admission.demand(), VueAdmittedDemand::RuntimeRender);
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
        let _ = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("multi-product admits");
        assert_eq!(projection_catalog_consult_count(), before + 1);
    }

    #[test]
    fn ssr_render_demand_admits_the_server_runtime_product() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(
                &artifact,
                VueHostRuntimeRenderDemand {
                    vue_options: VueOptionAttempt {
                        ssr: true,
                        ..Default::default()
                    },
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
    fn unsupported_product_refuses_typed_and_issues_nothing() {
        let artifact = vue_artifact(SFC);
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::PublicApi(Default::default())],
                    ..Default::default()
                },
            )
            .expect_err("the Vue host route has no public-api production path");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::UnsupportedProduct(ProductKind::PublicApi)
        );
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::Declarations(Default::default())],
                    ..Default::default()
                },
            )
            .expect_err("the Vue host route has no declarations production path");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::UnsupportedProduct(ProductKind::Declarations)
        );
    }

    #[test]
    fn dual_runtime_kind_demand_refuses_at_issuance() {
        let artifact = vue_artifact(SFC);
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![
                        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
                        CompileProduct::RuntimeServer(RuntimeProductRequest::default()),
                    ],
                    vue_options: VueOptionAttempt {
                        ssr: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect_err("one bundle pass runs one ssr mode; a dual-kind demand cannot be served");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::UnproducibleDemand(VueHostUnproducibleDemand::DualRuntimeKind)
        );
    }

    #[test]
    fn ssr_flag_disagreeing_with_the_runtime_kind_refuses_at_issuance() {
        let artifact = vue_artifact(SFC);
        // ssr=true with a CLIENT product would silently compile client.
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeClient(
                        RuntimeProductRequest::default(),
                    )],
                    vue_options: VueOptionAttempt {
                        ssr: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect_err("the ssr flag and the demanded runtime kind must agree");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::UnproducibleDemand(
                VueHostUnproducibleDemand::SsrFlagRuntimeKindMismatch
            )
        );
        // The inverse: a SERVER product with ssr=false.
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeServer(
                        RuntimeProductRequest::default(),
                    )],
                    ..Default::default()
                },
            )
            .expect_err("a server product without the ssr flag is the same divergence");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::UnproducibleDemand(
                VueHostUnproducibleDemand::SsrFlagRuntimeKindMismatch
            )
        );
    }

    /// The ssr flag is an axis of the RUNTIME leg. A demand that asks for
    /// no runtime product at all runs no runtime leg, so the flag drives
    /// nothing and cannot be silently dropped — such a demand is admitted
    /// whatever the flag says, and the admitted request carries the
    /// demanded products unchanged.
    #[test]
    fn ssr_flag_without_any_runtime_product_admits() {
        let artifact = vue_artifact(SFC);
        for products in [
            vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
            vec![CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: false,
                want_template_data: true,
            })],
            vec![
                CompileProduct::IdeCompanion(IdeProductRequest::default()),
                CompileProduct::Analysis(AnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
        ] {
            let admission = VueHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    VueHostMultiProductDemand {
                        products: products.clone(),
                        vue_options: VueOptionAttempt {
                            ssr: true,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .expect("no runtime product is demanded, so the ssr flag drives nothing");
            assert_eq!(
                admission
                    .request()
                    .products()
                    .iter()
                    .map(CompileProduct::kind)
                    .collect::<Vec<_>>(),
                products
                    .iter()
                    .map(CompileProduct::kind)
                    .collect::<Vec<_>>(),
                "the admitted request carries exactly the demanded products"
            );
            let grants = admission.into_execution_grants();
            assert!(
                grants.runtime.is_none(),
                "no runtime leg is admitted, so the ssr mode is never derived"
            );
        }
    }

    #[test]
    fn analysis_script_bindings_demand_refuses_at_issuance() {
        let artifact = vue_artifact(SFC);
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
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
            VueHostAdmissionRefusal::UnproducibleDemand(
                VueHostUnproducibleDemand::AnalysisScriptBindings
            )
        );
    }

    #[test]
    fn every_unroutable_vue_option_refuses_at_issuance_on_both_entry_points() {
        use crate::compile_request::vue::{
            VueAssetUrlTransform, VueCssModulesOptions, VueParsePad, VueWhitespaceStrategy,
        };
        let artifact = vue_artifact(SFC);
        let variants: [(VueOptionAttempt, VueOption); 14] = [
            (
                VueOptionAttempt {
                    whitespace: Some(VueWhitespaceStrategy::Preserve),
                    ..Default::default()
                },
                VueOption::ParserOptionsWhitespace,
            ),
            (
                VueOptionAttempt {
                    hoist_static: Some(false),
                    ..Default::default()
                },
                VueOption::TransformOptionsHoistStatic,
            ),
            (
                VueOptionAttempt {
                    cache_handlers: Some(true),
                    ..Default::default()
                },
                VueOption::TransformOptionsCacheHandlers,
            ),
            (
                VueOptionAttempt {
                    hmr: Some(true),
                    ..Default::default()
                },
                VueOption::TransformOptionsHmr,
            ),
            (
                VueOptionAttempt {
                    optimize_imports: Some(true),
                    ..Default::default()
                },
                VueOption::CodegenOptionsOptimizeImports,
            ),
            (
                VueOptionAttempt {
                    ssr_runtime_module_name: Some("vue/server-renderer".to_string()),
                    ..Default::default()
                },
                VueOption::CodegenOptionsSsrRuntimeModuleName,
            ),
            (
                VueOptionAttempt {
                    parse_pad: Some(VueParsePad::Line),
                    ..Default::default()
                },
                VueOption::ParsePad,
            ),
            (
                VueOptionAttempt {
                    ignore_empty: Some(true),
                    ..Default::default()
                },
                VueOption::ParseIgnoreEmpty,
            ),
            (
                VueOptionAttempt {
                    babel_parser_plugins: vec!["jsx".to_string()],
                    ..Default::default()
                },
                VueOption::CompileScriptBabelParserPlugins,
            ),
            (
                VueOptionAttempt {
                    gen_default_as: Some("__default__".to_string()),
                    ..Default::default()
                },
                VueOption::CompileScriptGenDefaultAs,
            ),
            (
                VueOptionAttempt {
                    props_destructure: Some(true),
                    ..Default::default()
                },
                VueOption::CompileScriptPropsDestructure,
            ),
            (
                VueOptionAttempt {
                    transform_asset_urls: Some(VueAssetUrlTransform::Disabled),
                    ..Default::default()
                },
                VueOption::CompileTemplateTransformAssetUrls,
            ),
            (
                VueOptionAttempt {
                    style_trim: Some(true),
                    ..Default::default()
                },
                VueOption::CompileStyleTrim,
            ),
            (
                VueOptionAttempt {
                    css_modules: Some(VueCssModulesOptions::default()),
                    ..Default::default()
                },
                VueOption::CompileStyleModules,
            ),
        ];
        for (options, expected) in variants {
            let refusal = VueHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    VueHostMultiProductDemand {
                        products: vec![CompileProduct::RuntimeClient(
                            RuntimeProductRequest::default(),
                        )],
                        vue_options: options.clone(),
                        ..Default::default()
                    },
                )
                .expect_err("an admitted-but-unroutable option must refuse, not drop");
            assert_eq!(
                refusal,
                VueHostAdmissionRefusal::UnproducibleDemand(VueHostUnproducibleDemand::VueOption(
                    expected
                ))
            );
            let refusal = VueHostIntegrationBackend::new()
                .admit_runtime_render(
                    &artifact,
                    VueHostRuntimeRenderDemand {
                        vue_options: options,
                        ..Default::default()
                    },
                )
                .expect_err("the render demand validates the same producibility class");
            assert_eq!(
                refusal,
                VueHostAdmissionRefusal::UnproducibleDemand(VueHostUnproducibleDemand::VueOption(
                    expected
                ))
            );
        }
    }

    #[test]
    fn admission_is_issued_only_over_a_vue_parse() {
        let foreign = parse_registered_source_for_tests(
            FileLanguage::svelte(),
            CarrierGrammarConfig::Svelte,
            "<p>a</p>",
        );
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(&foreign, multi_demand())
            .expect_err("a Svelte artifact composes no Vue parse admission");
        assert_eq!(refusal, VueHostAdmissionRefusal::NotAVueParse);
        let refusal = VueHostIntegrationBackend::new()
            .admit_runtime_render(&foreign, VueHostRuntimeRenderDemand::default())
            .expect_err("the render demand composes the same parse admission");
        assert_eq!(refusal, VueHostAdmissionRefusal::NotAVueParse);
    }

    #[test]
    fn ssr_vapor_demand_refuses_at_issuance() {
        let artifact = vue_artifact(SFC);
        let refusal = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeServer(Default::default())],
                    vue_options: VueOptionAttempt {
                        backend: VueBackendRequest::Vapor,
                        ssr: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect_err("SSR x Vapor refuses at canonical request construction");
        assert_eq!(
            refusal,
            VueHostAdmissionRefusal::RequestConstructionRefused(
                CompileRequestError::SsrVaporBackendUnsupported
            )
        );
    }

    #[test]
    fn one_admitted_request_populates_prerequisites_once() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let runtime_before = runtime_backend_delegation_count();
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let products = VueHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
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
            "exactly one runtime-backend population for the whole request"
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
        assert!(products.runtime_client_bundle().is_some());
        assert!(
            products.runtime_server_bundle().is_none(),
            "the server accessor never serves a bundle whose ssr compile did not run"
        );
        assert!(products.ide_companion().is_some());
        assert!(products.template_facts().is_some());
    }

    #[test]
    fn render_execution_is_render_only() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let rendered = VueHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the render-only compile produces");

        assert!(
            rendered.runtime_bundle().has_runtime_surface(),
            "the render handoff carries the runtime main surface"
        );
        assert!(rendered.runtime_bundle().tsx.is_none());
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
        let artifact = vue_artifact(SFC);
        let render = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = VueHostIntegrationBackend::new()
            .compile_host_products(
                render,
                &artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("a render admission does not admit the multi-product entry");
        assert!(matches!(
            refusal,
            VueHostCompileRefusal::WrongDemand {
                expected: VueAdmittedDemand::HostMultiProduct,
                actual: VueAdmittedDemand::RuntimeRender,
            }
        ));
    }

    #[test]
    fn admission_binds_to_the_exact_admitted_parse() {
        let artifact = vue_artifact(SFC);
        let other = vue_artifact("<template><span>other</span></template>");
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = VueHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &other,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect_err("an admission never executes against a different parse");
        assert!(matches!(
            refusal,
            VueHostCompileRefusal::AdmissionParseMismatch
        ));
    }

    #[test]
    fn publication_payloads_gate_on_the_admitted_product_set() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(
                &artifact,
                VueHostMultiProductDemand {
                    products: vec![CompileProduct::RuntimeClient(
                        RuntimeProductRequest::default(),
                    )],
                    ..Default::default()
                },
            )
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let products = VueHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
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

    #[test]
    fn refusal_is_atomic_no_product_publishes_after_it() {
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        // A selected script content artifact makes the IDE leg refuse
        // (`BlockContentIdeUnavailable`); the whole transaction fails
        // closed — the already-producible runtime leg publishes nothing.
        let inputs = VueHostExecutionInputs {
            block_content: super::super::carrier_compiler::RuntimeBlockContentInputs {
                script_setup: Some(super::super::RuntimeBlockContentInput {
                    code: Arc::from("const answer = 42"),
                    source_map: None,
                    lang: "ts".to_string(),
                    content_artifact_token: "content:ts".to_string(),
                    source_space_token: "space:ts".to_string(),
                    parsed: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let refusal = VueHostIntegrationBackend::new()
            .compile_host_products(admission, &artifact, &inputs, &alloc)
            .expect_err("the transaction is all-or-none");
        assert!(matches!(
            refusal,
            VueHostCompileRefusal::Unsupported(
                CompileUnsupported::BlockContentIdeUnavailable { .. }
            )
        ));
    }

    #[test]
    fn generic_compile_route_never_consults_the_host_backend() {
        let artifact = vue_artifact(SFC);
        let alloc = oxc_allocator::Allocator::new();
        let before = host_backend_execution_count();
        let outcome = VueCarrierCompiler
            .compile_bundle(
                SFC,
                &artifact,
                &super::super::RuntimeCompileOptions::default(),
                &alloc,
            )
            .expect("the compatibility route still compiles");
        assert!(matches!(outcome, CarrierCompileOutcome::Produced(_)));
        assert_eq!(
            host_backend_execution_count(),
            before,
            "the generic production route must not execute the host backend"
        );

        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = VueHostIntegrationBackend::new().compile_runtime_render(
            admission,
            &artifact,
            &VueHostExecutionInputs::default(),
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
        let artifact = vue_artifact(SFC);
        let alloc = oxc_allocator::Allocator::new();
        let compile = |runtime_map: bool, ide_map: bool| {
            let admission = VueHostIntegrationBackend::new()
                .admit_host_products(
                    &artifact,
                    VueHostMultiProductDemand {
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
                        ..Default::default()
                    },
                )
                .expect("admits");
            VueHostIntegrationBackend::new()
                .compile_host_products(
                    admission,
                    &artifact,
                    &VueHostExecutionInputs::default(),
                    &alloc,
                )
                .expect("produces")
        };

        let products = compile(true, false);
        let script = &products
            .runtime_client_bundle()
            .expect("client bundle")
            .script
            .as_ref()
            .expect("script block")
            .source_map;
        assert!(
            !script.is_empty(),
            "runtime_source_map=true must populate the runtime leg's own map"
        );
        assert!(
            products.ide_companion().expect("ide").source_map.is_empty(),
            "want_source_map=false must keep the IDE leg's map OFF even \
             though the runtime leg demanded one"
        );

        let products = compile(false, true);
        let script = &products
            .runtime_client_bundle()
            .expect("client bundle")
            .script
            .as_ref()
            .expect("script block")
            .source_map;
        assert!(
            script.is_empty(),
            "an IDE-only map demand must not switch the runtime map on"
        );
        assert!(
            !products.ide_companion().expect("ide").source_map.is_empty(),
            "want_source_map=true must populate the IDE leg's own map"
        );
    }

    #[test]
    fn render_entry_refuses_the_multi_product_admission() {
        let artifact = vue_artifact(SFC);
        let multi = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = VueHostIntegrationBackend::new()
            .compile_runtime_render(multi, &artifact, &VueHostExecutionInputs::default(), &alloc)
            .expect_err("a multi-product admission does not admit the render entry");
        assert!(matches!(
            refusal,
            VueHostCompileRefusal::WrongDemand {
                expected: VueAdmittedDemand::RuntimeRender,
                actual: VueAdmittedDemand::HostMultiProduct,
            }
        ));
    }

    #[test]
    fn ssr_render_execution_produces_the_server_surface() {
        use super::super::TemplateRenderExport;
        let artifact = vue_artifact(SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(
                &artifact,
                VueHostRuntimeRenderDemand {
                    vue_options: VueOptionAttempt {
                        ssr: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let rendered = VueHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the SSR render compile produces");
        assert!(rendered.runtime_bundle().has_runtime_surface());
        assert_eq!(
            rendered
                .runtime_bundle()
                .template
                .as_ref()
                .expect("template block")
                .render_export,
            TemplateRenderExport::SsrRender,
            "the SSR demand's bundle attaches the server render function"
        );
    }

    const MALFORMED_EXPR_SFC: &str = concat!(
        "<script setup lang=\"ts\">\n",
        "const count = 1;\n",
        "</script>\n",
        "<template>\n",
        "  <div v-if=\"count ===\">{{ count }}</div>\n",
        "</template>\n",
    );

    #[test]
    fn demanded_template_fact_companion_surfaces_producer_diagnostics_payload_free() {
        let artifact = vue_artifact(MALFORMED_EXPR_SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(
                &artifact,
                VueHostRuntimeRenderDemand {
                    template_fact_diagnostics: true,
                    ..Default::default()
                },
            )
            .expect("admits");
        let _ = take_template_facts_producer_invocations();
        let alloc = oxc_allocator::Allocator::new();
        let rendered = VueHostIntegrationBackend::new()
            .compile_runtime_render(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect("the render compile produces a bundle carrying the refusing diagnostics");
        assert_eq!(
            take_template_facts_producer_invocations(),
            1,
            "the demanded companion runs the fact producer exactly once"
        );
        assert!(
            rendered
                .runtime_bundle()
                .diagnostics
                .iter()
                .any(|d| d.code == "XInvalidExpression"),
            "the fact producer's expression diagnostic must surface on the \
             render bundle so the host lane fails the render closed, got {:?}",
            rendered.runtime_bundle().diagnostics
        );
        assert!(
            rendered.runtime_bundle().template_data.is_none(),
            "the companion is diagnostics-only: the fact payload never \
             publishes on the render handoff"
        );
    }

    #[test]
    fn undemanded_template_fact_companion_runs_no_fact_producer() {
        let artifact = vue_artifact(MALFORMED_EXPR_SFC);
        let admission = VueHostIntegrationBackend::new()
            .admit_runtime_render(&artifact, VueHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = take_template_facts_producer_invocations();
        let alloc = oxc_allocator::Allocator::new();
        let _ = VueHostIntegrationBackend::new().compile_runtime_render(
            admission,
            &artifact,
            &VueHostExecutionInputs::default(),
            &alloc,
        );
        assert_eq!(
            take_template_facts_producer_invocations(),
            0,
            "an undemanded companion never runs the fact producer"
        );
    }

    /// Source-byte binding is structural: the execution entries take no
    /// independent source parameter, so an admission over parse A can never
    /// execute against foreign bytes B — the artifact's own registered
    /// carrier source (the bytes its parse identity was computed from) is
    /// the only byte authority the seam can reach.
    #[test]
    fn execution_source_is_the_admitted_artifacts_own_registered_bytes() {
        let artifact = vue_artifact(SFC);
        assert_eq!(
            artifact.carrier_source().as_ref(),
            SFC,
            "the artifact's carrier source is the exact registered bytes"
        );
        let admission = VueHostIntegrationBackend::new()
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let products = VueHostIntegrationBackend::new()
            .compile_host_products(
                admission,
                &artifact,
                &VueHostExecutionInputs::default(),
                &alloc,
            )
            .expect("executes over the artifact's own bytes");
        assert!(products.runtime_client_bundle().is_some());
    }

    #[test]
    fn semantic_admission_requires_the_witnessed_parse() {
        use super::super::vue_carrier_frontend::VueCarrierFrontend;
        use super::super::vue_semantic_authority::VueSemanticAuthority;
        let artifact = vue_artifact(SFC);
        let other = vue_artifact("<template><span>other</span></template>");
        let parse = VueCarrierFrontend
            .admit_registered(&artifact)
            .expect("admits");
        assert!(
            VueSemanticAuthority
                .admit_over_parse(&parse, &artifact)
                .is_some(),
            "the witnessed parse composes over its own artifact"
        );
        assert!(
            VueSemanticAuthority
                .admit_over_parse(&parse, &other)
                .is_none(),
            "a parse admission witnessed over one artifact never composes \
             a semantic admission over another"
        );
    }
}
