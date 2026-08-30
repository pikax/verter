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
    CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest, ProductKind,
    RuntimeProductRequest, SvelteOptionAttempt,
};
use crate::standalone::registered_runtime_for;
use crate::svelte::carrier::{svelte_carrier_bundle, SvelteCarrierCompiler};
use crate::svelte::carrier_frontend::{SvelteCarrierFrontend, SvelteParseAdmission, SvelteSfc5};
use crate::svelte::semantic_authority::{SvelteSemanticAdmission, SvelteSemanticAuthority};

use super::capability::{FrameworkHostIntegrationBackend, NativeHostEpoch};
use super::carrier_compiler::{
    CarrierCompileOutcome, CompileUnsupported, IdeOutput, RuntimeCompileOptions,
    RuntimeCompileOutput, RuntimeDiagnostic,
};
use super::catalog::{CatalogCapability, HostCap, TypedCapabilityRegistration};
use super::registered_carrier_projection::{
    registered_projection_for, registered_semantic_for, TemplateFactsProduct,
};
use super::{CarrierCompiler, FrameworkParseArtifact, Present};

/// Svelte host-integration backend for the native host epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SvelteHostIntegrationBackend;

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
}

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

    /// Compile a host-backed multi-product admission through the shared
    /// orchestration, publishing per-product payloads gated on the
    /// admitted set. One admitted request populates its parse, semantic,
    /// projection, plan, and emit prerequisites once — the self-contained
    /// `Main` and its requested style side-products come from that one
    /// population, never a second compile.
    pub fn compile_host_products(
        &self,
        admission: &SvelteCompileAdmission,
        source: &str,
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
        let bundle = self.execute(admission, source, artifact, inputs, alloc)?;
        Ok(SvelteHostCompiledProducts {
            admitted: admission.admitted_products(),
            bundle,
        })
    }

    /// Compile a runtime-render admission: the render-only handoff for
    /// host `Main` assembly. Never plans or publishes an IDE companion or
    /// template facts.
    pub fn compile_runtime_render(
        &self,
        admission: &SvelteCompileAdmission,
        source: &str,
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
        let mut bundle = self.execute(admission, source, artifact, inputs, alloc)?;
        // Render-only handoff: no analysis product was admitted, so no
        // fact payload was produced; keep the invariant structural even if
        // the orchestration ever grows a fact side channel.
        bundle.template_data = None;
        Ok(SvelteHostRenderedMain { bundle })
    }

    /// The one execution path both entry points share: verify the exact
    /// parse binding, derive the neutral options off the admitted request,
    /// and drive the shared Svelte bundle orchestration. Caller owns the
    /// allocator scratch lifecycle; a dropped admission simply never
    /// executes — there is no partial publication channel.
    fn execute(
        &self,
        admission: &SvelteCompileAdmission,
        source: &str,
        artifact: &FrameworkParseArtifact,
        inputs: &SvelteHostExecutionInputs,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, SvelteHostCompileRefusal> {
        record_host_backend_execution();
        if artifact.parse_key() != admission.parse_key.as_ref() {
            return Err(SvelteHostCompileRefusal::AdmissionParseMismatch);
        }
        let opts = derive_admitted_runtime_options(&admission.request, inputs);
        match svelte_carrier_bundle(source, artifact, &opts, alloc) {
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
        refuse_unproducible_products(&demand.products)?;
        refuse_unproducible_svelte_options(&demand.svelte_options)?;
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
        refuse_unproducible_svelte_options(&demand.svelte_options)?;
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
}

/// Reads the framework-neutral options off the ADMITTED request — the
/// compiler-side half of the construct-then-derive pattern: the request is
/// the identity authority, the execution inputs ride beside it.
fn derive_admitted_runtime_options(
    request: &CompileRequest,
    inputs: &SvelteHostExecutionInputs,
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
        custom_element: svelte.and_then(|s| s.custom_element).unwrap_or(false),
        // Per-leg, read faithfully off each admitted product's OWN
        // request: the runtime flag drives only the runtime leg and the
        // IDE flag only the IDE leg — demanding a map on one never
        // switches it on for the other.
        source_map: runtime.is_some_and(|r| r.runtime_source_map),
        ide_source_map: Some(ide.is_some_and(|i| i.want_source_map)),
        ssr,
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

/// Producibility validation over the demanded SVELTE OPTIONS: every axis
/// [`derive_admitted_runtime_options`] cannot route into the bundle
/// execution refuses at issuance instead of being silently dropped or
/// substituted. (The six unconditionally-unsupported option rows plus the
/// `SVELTE-MODULE`-gated pair refuse separately, at canonical request
/// construction.) Deterministic declaration order.
fn refuse_unproducible_svelte_options(
    options: &SvelteOptionAttempt,
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
        SvelteHostIntegrationBackend.adapter_id(),
        SvelteHostIntegrationBackend.carrier_language_id(),
        Present(SvelteHostIntegrationBackend),
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
        let admission = SvelteHostIntegrationBackend
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
        let admission = SvelteHostIntegrationBackend
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
        let _ = SvelteHostIntegrationBackend
            .admit_host_products(&artifact, multi_demand())
            .expect("multi-product admits");
        assert_eq!(projection_catalog_consult_count(), before + 1);
    }

    #[test]
    fn ssr_render_demand_admits_the_server_runtime_product() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend
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
        let admission = SvelteHostIntegrationBackend
            .admit_runtime_render(
                &artifact,
                SvelteHostRuntimeRenderDemand {
                    ssr: true,
                    ..Default::default()
                },
            )
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend
            .compile_runtime_render(
                &admission,
                COMPONENT,
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
        let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
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
            let refusal = SvelteHostIntegrationBackend
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
            let refusal = SvelteHostIntegrationBackend
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
            let refusal = SvelteHostIntegrationBackend
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
            let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
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
        let refusal = SvelteHostIntegrationBackend
            .admit_host_products(&foreign, multi_demand())
            .expect_err("a Vue artifact composes no Svelte parse admission");
        assert_eq!(refusal, SvelteHostAdmissionRefusal::NotASvelteParse);
        let refusal = SvelteHostIntegrationBackend
            .admit_runtime_render(&foreign, SvelteHostRuntimeRenderDemand::default())
            .expect_err("the render demand composes the same parse admission");
        assert_eq!(refusal, SvelteHostAdmissionRefusal::NotASvelteParse);
    }

    #[test]
    fn one_admitted_request_populates_prerequisites_once() {
        let artifact = svelte_artifact(COMPONENT);
        let admission = SvelteHostIntegrationBackend
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let runtime_before = runtime_backend_delegation_count();
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let products = SvelteHostIntegrationBackend
            .compile_host_products(
                &admission,
                COMPONENT,
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
        assert!(
            bundle.main.body_code.is_some(),
            "the self-contained Main module comes from the one population"
        );
        assert!(
            !bundle.styles.is_empty(),
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
        let admission = SvelteHostIntegrationBackend
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = take_projection_producer_invocations();
        let _ = take_template_facts_producer_invocations();

        let alloc = oxc_allocator::Allocator::new();
        let rendered = SvelteHostIntegrationBackend
            .compile_runtime_render(
                &admission,
                COMPONENT,
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
                .body_code
                .as_deref()
                .is_some_and(|body| body.contains("svelte/internal/client")),
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
        let render = SvelteHostIntegrationBackend
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend
            .compile_host_products(
                &render,
                COMPONENT,
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

        let multi = SvelteHostIntegrationBackend
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let refusal = SvelteHostIntegrationBackend
            .compile_runtime_render(
                &multi,
                COMPONENT,
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
        let admission = SvelteHostIntegrationBackend
            .admit_host_products(&artifact, multi_demand())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend
            .compile_host_products(
                &admission,
                COMPONENT,
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
        let admission = SvelteHostIntegrationBackend
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
        let products = SvelteHostIntegrationBackend
            .compile_host_products(
                &admission,
                COMPONENT,
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
        let admission = SvelteHostIntegrationBackend
            .admit_host_products(&artifact, multi_demand())
            .expect("issuance is producibility validation, not source support");
        let _ = take_projection_producer_invocations();
        let alloc = oxc_allocator::Allocator::new();
        let refusal = SvelteHostIntegrationBackend
            .compile_host_products(
                &admission,
                SNIPPET_COMPONENT,
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
        let outcome = SvelteCarrierCompiler
            .compile_bundle(
                COMPONENT,
                &artifact,
                &RuntimeCompileOptions::default(),
                &alloc,
            )
            .expect("the compatibility route still compiles");
        assert!(matches!(outcome, CarrierCompileOutcome::Produced(_)));
        assert_eq!(
            host_backend_execution_count(),
            before,
            "the generic production route must not execute the host backend"
        );

        let admission = SvelteHostIntegrationBackend
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let _ = SvelteHostIntegrationBackend.compile_runtime_render(
            &admission,
            COMPONENT,
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
            let admission = SvelteHostIntegrationBackend
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
            SvelteHostIntegrationBackend
                .compile_host_products(
                    &admission,
                    COMPONENT,
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
            !main.source_map.is_empty(),
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
            main.source_map.is_empty(),
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
        let admission = SvelteHostIntegrationBackend
            .admit_runtime_render(&artifact, SvelteHostRuntimeRenderDemand::default())
            .expect("admits");
        let alloc = oxc_allocator::Allocator::new();
        let rendered = SvelteHostIntegrationBackend
            .compile_runtime_render(
                &admission,
                COMPONENT,
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
            .styles
            .first()
            .expect("the scoped style side-product");
        assert!(
            style.code.contains("verter-override-1"),
            "the resolved cssHash override is the scope class, got: {}",
            style.code
        );
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
