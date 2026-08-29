//! The canonical typed compiler request — the sole request-construction
//! authority for every reachable production compile route.
//!
//! Replaces four parallel, silently-defaulting option authorities:
//! `CompileTarget` bitflags, `CodegenOptions`, `VerterCompileOptions` (all
//! formerly in `crate::compile::types`), and the session's `CompileProfile`.
//! Every semantics-affecting option the framework conformance inventory
//! classifies (153 rows across Vue and Svelte) maps exactly once onto a
//! field here, a derived computation, a host-resolved validated
//! pass-through, or a typed unsupported refusal — never a silently-ignored
//! default. See [`vue::VueOption`] / [`svelte::SvelteOption`] for the
//! per-row exhaustive classification and [`capability::CapabilityCell`] for
//! the framework/backend/product capability lookup.

pub mod capability;
pub mod product;
pub mod svelte;
pub mod vue;

pub use capability::{CapabilityCell, CapabilityDisposition};
pub use product::{
    AnalysisProductRequest, CompileProduct, DeclarationProductRequest, IdeProductRequest,
    ProductKind, PublicApiProductRequest, RuntimeProductRequest, RuntimeStyleProcessing,
};
pub use svelte::{SvelteCompileRequest, SvelteOption, SvelteOptionAttempt, SvelteOptionClass};
pub use vue::{VueBackendRequest, VueCompileRequest, VueOption, VueOptionAttempt, VueOptionClass};

use verter_identity::profile::TypeScriptSemanticProfileId;

/// One semantics-affecting Vue or Svelte framework option, closed over both
/// TSV inventories — the exhaustiveness proof that every option maps
/// exactly once is `VueOption::class` / `SvelteOption::class`; this wrapper
/// lets a single [`CompileRequestError`] arm name either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkOption {
    Vue(VueOption),
    Svelte(SvelteOption),
}

/// Every reason canonical request construction (or, for the two
/// post-parse-only cases, resolution) refuses. Constructing/resolving a
/// `CompileRequest` either succeeds or returns exactly one of these — never
/// a silent default substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileRequestError {
    /// The named option is not admitted onto any production request —
    /// `unsupported fail-closed` in the official Vue/Svelte option
    /// inventory. Refused on presence, even when the caller supplied an
    /// explicit `false`.
    UnsupportedOption {
        option: FrameworkOption,
        capability: Option<CapabilityCell>,
    },
    /// The named option IS admitted, but the caller's value does not match
    /// any of its recognized forms (e.g. an `svelte_namespace` string
    /// outside `"html"`/`"svg"`/`"mathml"`). Distinct from
    /// [`Self::UnsupportedOption`]: the option itself is fine, only this
    /// particular value is not a decode-boundary concern the request can
    /// silently guess at.
    MalformedOptionValue {
        option: FrameworkOption,
        value: String,
    },
    /// SSR requested together with a Vapor backend request — RC.3 defines
    /// no Cartesian Vapor server compiler backend
    /// (`VUE-SSR-VAPOR-BACKEND`, `capability-matrix.tsv:6`). Returned both
    /// at construction (explicit `force_vapor` + an `SSR` product) and at
    /// post-parse resolution (`<template vapor>` + an `SSR` product) — see
    /// [`CompileRequest::resolve_vue_backend`].
    SsrVaporBackendUnsupported,
    /// Inline assembly requested together with SSR — the SSR capability has
    /// no inline axis; an operation absent from the matrix is unsupported.
    /// Never silently demoted to non-inline.
    InlineSsrUnsupported,
    /// Inline assembly requested together with a Vapor backend — the
    /// Vapor-client capability cell explicitly claims inline/separate
    /// (`capability-matrix.tsv:4`), so the request CONSTRUCTS; this is
    /// returned only at execution, once the runtime codegen backend
    /// confirms it cannot yet honor the combination (never a silent
    /// non-inline demotion).
    VaporInlineNotYetImplemented,
    /// A capability cell this request touches is `unsupported fail-closed`
    /// (e.g. `SVELTE-MODULE`, `SVELTE-HMR`, `SVELTE-COMPAT-API4`,
    /// `VUE-COMPAT-V2`) independent of any single option row.
    CapabilityUnsupported(CapabilityCell),
    /// The requested product set is empty — a request must name at least
    /// one product.
    EmptyProductSet,
    /// The same product kind was requested more than once.
    DuplicateProduct(ProductKind),
    /// The request's framework does not match the framework-specific
    /// execution path it was handed to (e.g. a `Svelte` request reaching
    /// the Vue-only internal compile driver). The request itself
    /// constructed fine — every field on it is self-consistent — this is
    /// strictly an execution-boundary mismatch between the request's own
    /// declared framework and the caller's choice of driver.
    FrameworkMismatch {
        expected: &'static str,
        actual: &'static str,
    },
}

/// Resolved once parsing determines the source's own backend marker
/// (`<template vapor>`) — the implicit half of the `SSR x Vapor` fail-closed
/// rule, which cannot be checked at construction time because construction
/// runs before parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedVueBackend {
    Vdom,
    Vapor,
}

#[derive(Debug, Clone)]
pub enum FrameworkCompileRequest {
    Vue(VueCompileRequest),
    Svelte(SvelteCompileRequest),
}

/// The canonical, exhaustively-classified compiler request.
///
/// `filename`/`component_id`/`is_production`/`force_js` are top-level
/// (framework- and product-neutral) rather than per-product: every product
/// requested together in one compile shares the same source identity and
/// dev/prod profile — the legacy `CodegenOptions`/`VerterCompileOptions`
/// split them across two structs but never let them vary per-artifact
/// within one call, so splitting them per-product here would invent a
/// distinction no caller has ever made. `force_vapor`/`force_js` are
/// Verter-internal composition knobs, not official Vue/Svelte options —
/// absent from the official option inventory, same as their legacy fields.
#[derive(Debug, Clone)]
pub struct CompileRequest {
    products: Vec<CompileProduct>,
    framework: FrameworkCompileRequest,
    semantic_profile: Option<TypeScriptSemanticProfileId>,
    filename: Option<String>,
    component_id: Option<String>,
    is_production: bool,
    force_js: bool,
}

impl CompileRequest {
    /// Constructs the canonical request, applying every construction-time
    /// fail-closed rule before any downstream compiler stage sees the
    /// result: empty/duplicate product set, and (Vue only) `SSR x Vapor`
    /// explicit-backend refusal and `inline x SSR` refusal.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        products: Vec<CompileProduct>,
        framework: FrameworkCompileRequest,
        semantic_profile: Option<TypeScriptSemanticProfileId>,
        filename: Option<String>,
        component_id: Option<String>,
        is_production: bool,
        force_js: bool,
    ) -> Result<Self, CompileRequestError> {
        if products.is_empty() {
            return Err(CompileRequestError::EmptyProductSet);
        }
        let mut seen = std::collections::HashSet::new();
        for product in &products {
            if !seen.insert(product.kind()) {
                return Err(CompileRequestError::DuplicateProduct(product.kind()));
            }
        }

        let ssr_requested = products
            .iter()
            .any(|p| matches!(p, CompileProduct::RuntimeServer(_)));
        let inline_requested = products.iter().any(|p| match p {
            CompileProduct::RuntimeClient(r) | CompileProduct::RuntimeServer(r) => {
                r.inline == Some(true)
            }
            _ => false,
        });

        if let FrameworkCompileRequest::Vue(vue) = &framework {
            if ssr_requested && vue.backend.is_vapor() {
                return Err(CompileRequestError::SsrVaporBackendUnsupported);
            }
            if inline_requested && ssr_requested {
                return Err(CompileRequestError::InlineSsrUnsupported);
            }
        } else if inline_requested {
            // `inline` is a Vue-only axis (official `compileScript({
            // inlineTemplate })` production topology); Svelte has no such
            // concept, so a caller who somehow sets it on a Svelte request
            // gets the same typed refusal rather than a silently-ignored
            // field.
            return Err(CompileRequestError::InlineSsrUnsupported);
        }

        Ok(Self {
            products,
            framework,
            semantic_profile,
            filename,
            component_id,
            is_production,
            force_js,
        })
    }

    pub fn products(&self) -> &[CompileProduct] {
        &self.products
    }

    pub fn framework(&self) -> &FrameworkCompileRequest {
        &self.framework
    }

    pub fn semantic_profile(&self) -> Option<&TypeScriptSemanticProfileId> {
        self.semantic_profile.as_ref()
    }

    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    pub fn component_id(&self) -> Option<&str> {
        self.component_id.as_deref()
    }

    pub fn is_production(&self) -> bool {
        self.is_production
    }

    pub fn force_js(&self) -> bool {
        self.force_js
    }

    pub fn vue(&self) -> Option<&VueCompileRequest> {
        match &self.framework {
            FrameworkCompileRequest::Vue(v) => Some(v),
            FrameworkCompileRequest::Svelte(_) => None,
        }
    }

    pub fn svelte(&self) -> Option<&SvelteCompileRequest> {
        match &self.framework {
            FrameworkCompileRequest::Svelte(s) => Some(s),
            FrameworkCompileRequest::Vue(_) => None,
        }
    }

    fn runtime_client_or_server(&self) -> Option<&RuntimeProductRequest> {
        self.products.iter().find_map(|p| match p {
            CompileProduct::RuntimeClient(r) | CompileProduct::RuntimeServer(r) => Some(r),
            _ => None,
        })
    }

    /// Style-pipeline ownership selected by the active runtime product.
    pub fn runtime_style_processing(&self) -> RuntimeStyleProcessing {
        self.runtime_client_or_server()
            .map_or(RuntimeStyleProcessing::Complete, |runtime| {
                runtime.style_processing
            })
    }

    fn analysis(&self) -> Option<AnalysisProductRequest> {
        self.products.iter().find_map(|p| match p {
            CompileProduct::Analysis(a) => Some(*a),
            _ => None,
        })
    }

    fn ide_companion(&self) -> Option<&IdeProductRequest> {
        self.products.iter().find_map(|p| match p {
            CompileProduct::IdeCompanion(i) => Some(i),
            _ => None,
        })
    }

    /// Resolved `inline` for the active runtime product (`None` when no
    /// `RuntimeClient`/`RuntimeServer` product is requested).
    pub fn resolve_inline(&self) -> Option<bool> {
        self.runtime_client_or_server()
            .map(|r| r.resolve_inline(self.is_production))
    }

    /// Resolves the implicit half of `SSR x Vapor` AND the `inline x
    /// Vapor` deferred-capability refusal: once parsing determines
    /// whether the source carries its own Vapor marker (`<template
    /// vapor>`), an SSR request whose backend
    /// resolves to Vapor fails closed here, and an inline request whose
    /// backend resolves to Vapor fails closed with the DEFERRED (not
    /// unsupported) capability error — the execution layer calls this
    /// immediately after parsing, before any codegen-mode selection. An
    /// explicit `force_vapor` + `ssr` combination is already refused at
    /// [`Self::new`]; this only covers what construction could not see.
    pub fn resolve_vue_backend(
        &self,
        source_marks_vapor: bool,
    ) -> Result<ResolvedVueBackend, CompileRequestError> {
        let vue = self.vue().expect("resolve_vue_backend is Vue-only");
        let is_vapor = match vue.backend {
            VueBackendRequest::Vapor => true,
            VueBackendRequest::Vdom => false,
            VueBackendRequest::Inferred => source_marks_vapor,
        };
        let ssr_requested = self
            .products
            .iter()
            .any(|p| matches!(p, CompileProduct::RuntimeServer(_)));
        if is_vapor && ssr_requested {
            return Err(CompileRequestError::SsrVaporBackendUnsupported);
        }
        if is_vapor && self.resolve_inline() == Some(true) {
            return Err(CompileRequestError::VaporInlineNotYetImplemented);
        }
        Ok(if is_vapor {
            ResolvedVueBackend::Vapor
        } else {
            ResolvedVueBackend::Vdom
        })
    }

    // ── Zero-work / product-membership predicates ──────────────────────
    //
    // Each mirrors the legacy `CompileTarget` bit-intersection accessor it
    // replaces exactly (verified against `crate::compile::types::
    // CompileTarget`'s bit values) — `RuntimeClient`/`RuntimeServer`
    // together stand in for the always-bundled `STYLE|SCRIPT|TEMPLATE`
    // preset bits, `IdeCompanion` for `TSX`, `Declarations` for `TSC`, and
    // `Analysis`'s two independent booleans for `SCRIPT`/`TEMPLATE_DATA`
    // (which the legacy presets always bundled together, but which the
    // real LSP profile — `IDE | TEMPLATE_DATA` — requests independently).

    /// The exact legacy `STYLE`/`TEMPLATE` bit membership — `pub(crate)` so
    /// `crate::compile`'s `CompileRequest -> CompileTarget` derivation can
    /// reconstruct raw bits precisely rather than through an OR'd
    /// convenience predicate.
    pub(crate) fn has_runtime_product(&self) -> bool {
        self.runtime_client_or_server().is_some()
    }

    /// The exact legacy `SCRIPT` bit's `Analysis`-only contribution.
    pub(crate) fn analysis_wants_script_bindings(&self) -> bool {
        self.analysis().is_some_and(|a| a.want_script_bindings)
    }

    /// The exact legacy `TEMPLATE_DATA` bit.
    pub(crate) fn analysis_wants_template_data(&self) -> bool {
        self.analysis().is_some_and(|a| a.want_template_data)
    }

    /// Legacy `needs_style()`: `STYLE` bit — `RuntimeClient`/`RuntimeServer`
    /// only.
    pub fn wants_style_codegen(&self) -> bool {
        self.has_runtime_product()
    }

    /// Legacy `needs_template_codegen()`: `TEMPLATE` bit —
    /// `RuntimeClient`/`RuntimeServer` only (NOT `IdeCompanion` — TSX
    /// codegen is a separate, independent gate).
    pub fn wants_template_codegen(&self) -> bool {
        self.has_runtime_product()
    }

    /// Legacy `needs_tsx()`: `TSX` bit — `IdeCompanion` only.
    pub fn wants_tsx(&self) -> bool {
        self.ide_companion().is_some()
    }

    /// Legacy `needs_tsc()`: `TSC` bit — `Declarations` only.
    pub fn wants_tsc(&self) -> bool {
        self.products
            .iter()
            .any(|p| matches!(p, CompileProduct::Declarations(_)))
    }

    /// Legacy `needs_template_data()`: `TEMPLATE_DATA` bit — `Analysis`
    /// with `want_template_data` only. A pure `RuntimeClient`/
    /// `RuntimeServer` request does NOT extract raw template data (the
    /// bundler/`BUNDLER` preset never set the `TEMPLATE_DATA` bit).
    pub fn wants_template_data(&self) -> bool {
        self.analysis_wants_template_data()
    }

    /// Legacy `needs_script()`: `SCRIPT | TEMPLATE | TEMPLATE_DATA` —
    /// true whenever a runtime product is requested OR template-data
    /// extraction is requested (extraction needs script-level analysis as
    /// a prerequisite regardless of whether `Analysis::want_script_bindings`
    /// is independently set).
    pub fn wants_script_output(&self) -> bool {
        self.has_runtime_product()
            || self.analysis_wants_script_bindings()
            || self.analysis_wants_template_data()
    }

    /// Legacy `needs_runtime_macro_semantics()`: `needs_script() ||
    /// needs_tsx()`.
    pub fn wants_runtime_macro_semantics(&self) -> bool {
        self.wants_script_output() || self.wants_tsx()
    }

    /// Legacy `publishes_runtime_module()` / the basis of
    /// `needs_runtime_prop_constructors()`: `SCRIPT | TEMPLATE` — a runtime
    /// product, OR `Analysis::want_script_bindings` alone (the `SCRIPT` bit
    /// `META`/`ANALYSIS` always carried alongside `TEMPLATE_DATA`).
    pub fn wants_runtime_prop_constructors(&self) -> bool {
        self.has_runtime_product() || self.analysis_wants_script_bindings()
    }

    /// Legacy `needs_runtime_module()`: `STYLE | SCRIPT | TEMPLATE` — same
    /// membership as [`Self::wants_runtime_prop_constructors`] under this
    /// product model (a runtime product always bundles all three legacy
    /// bits together; there is no `STYLE`-only product in production use —
    /// verified: `CompileTarget::STYLE` never constructs standalone
    /// anywhere in the workspace).
    pub fn wants_runtime_module(&self) -> bool {
        self.wants_runtime_prop_constructors()
    }

    /// Whether an IDE/TSX companion was requested.
    pub fn wants_ide_companion(&self) -> bool {
        self.wants_tsx()
    }

    /// The active runtime product's own map demand — independent of
    /// [`Self::wants_ide_companion`] (the fixed map-coupling regression:
    /// requesting an IDE companion alongside a runtime product must not
    /// silently turn this on).
    pub fn wants_runtime_source_map(&self) -> bool {
        self.runtime_client_or_server()
            .is_some_and(|r| r.runtime_source_map)
    }

    /// The IDE companion's own map demand — independent of the runtime
    /// product's `runtime_source_map`.
    pub fn wants_ide_source_map(&self) -> bool {
        self.ide_companion().is_some_and(|i| i.want_source_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vue_client(inline: Option<bool>) -> CompileProduct {
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline,
            ..Default::default()
        })
    }
    fn vue_server(inline: Option<bool>) -> CompileProduct {
        CompileProduct::RuntimeServer(RuntimeProductRequest {
            inline,
            ..Default::default()
        })
    }
    fn vue_req(backend: VueBackendRequest) -> FrameworkCompileRequest {
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend,
            ..Default::default()
        })
    }
    fn new_request(
        products: Vec<CompileProduct>,
        framework: FrameworkCompileRequest,
    ) -> Result<CompileRequest, CompileRequestError> {
        CompileRequest::new(products, framework, None, None, None, false, false)
    }

    // ── SSR x Vapor fails closed ───────────────────────────────────────
    #[test]
    fn explicit_ssr_and_vapor_backend_fails_closed() {
        let err =
            new_request(vec![vue_server(None)], vue_req(VueBackendRequest::Vapor)).unwrap_err();
        assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
    }

    #[test]
    fn ssr_without_vapor_still_succeeds() {
        let req = new_request(vec![vue_server(None)], vue_req(VueBackendRequest::Vdom))
            .expect("ssr + non-vapor must construct");
        assert!(matches!(
            req.products()[0],
            CompileProduct::RuntimeServer(_)
        ));
    }

    #[test]
    fn vapor_without_ssr_still_succeeds() {
        new_request(vec![vue_client(None)], vue_req(VueBackendRequest::Vapor))
            .expect("vapor + non-ssr must construct");
    }

    #[test]
    fn implicit_vapor_source_marker_with_ssr_fails_closed_after_parsing() {
        let req = new_request(vec![vue_server(None)], vue_req(VueBackendRequest::Inferred)).expect(
            "inferred backend + ssr constructs — the implicit case is unknown until parsing",
        );
        let err = req.resolve_vue_backend(true).unwrap_err();
        assert_eq!(err, CompileRequestError::SsrVaporBackendUnsupported);
    }

    #[test]
    fn implicit_non_vapor_source_with_ssr_resolves_ssr_topology() {
        let req =
            new_request(vec![vue_server(None)], vue_req(VueBackendRequest::Inferred)).unwrap();
        let resolved = req.resolve_vue_backend(false).unwrap();
        assert_eq!(resolved, ResolvedVueBackend::Vdom);
    }

    #[test]
    fn supported_ssr_with_vapor_metadata_product_is_not_refused() {
        // A supported SSR request may carry official Vapor *metadata*
        // (`capability-matrix.tsv:6`'s note) — this is the VDOM backend
        // selected explicitly, not a Vapor backend request; must not be
        // refused merely because the request is SSR.
        new_request(vec![vue_server(None)], vue_req(VueBackendRequest::Vdom))
            .expect("SSR + explicit VDOM backend must not be refused");
    }

    // ── inline x SSR / inline x Vapor ──────────────────────────────────
    #[test]
    fn inline_true_plus_ssr_fails_closed_no_silent_demotion() {
        let err = new_request(
            vec![vue_server(Some(true))],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap_err();
        assert_eq!(err, CompileRequestError::InlineSsrUnsupported);
    }

    #[test]
    fn inline_true_plus_explicit_vapor_constructs_capability_deferred_to_execution() {
        // Must CONSTRUCT (Vapor-client cell explicitly claims inline/
        // separate) — the runtime codegen backend has not implemented it
        // yet, so execution (not construction) is where the
        // capability-unavailable result lands.
        let req = new_request(
            vec![vue_client(Some(true))],
            vue_req(VueBackendRequest::Vapor),
        )
        .expect("inline + vapor must construct, not refuse");
        assert!(matches!(
            req.products()[0],
            CompileProduct::RuntimeClient(_)
        ));
        let err = req.resolve_vue_backend(false).unwrap_err();
        assert_eq!(err, CompileRequestError::VaporInlineNotYetImplemented);
    }

    #[test]
    fn inline_true_plus_implicit_vapor_marker_fails_closed_after_parsing() {
        let req = new_request(
            vec![vue_client(Some(true))],
            vue_req(VueBackendRequest::Inferred),
        )
        .expect("inline + inferred backend must construct — unknown until parsing");
        let err = req.resolve_vue_backend(true).unwrap_err();
        assert_eq!(err, CompileRequestError::VaporInlineNotYetImplemented);
    }

    #[test]
    fn inline_true_plus_vdom_backend_resolves_cleanly() {
        let req = new_request(
            vec![vue_client(Some(true))],
            vue_req(VueBackendRequest::Vdom),
        )
        .expect("inline + vdom must construct");
        let resolved = req.resolve_vue_backend(false).unwrap();
        assert_eq!(resolved, ResolvedVueBackend::Vdom);
    }

    #[test]
    fn inline_false_plus_ssr_still_succeeds() {
        new_request(
            vec![vue_server(Some(false))],
            vue_req(VueBackendRequest::Vdom),
        )
        .expect("inline=false is not the SSR-inline combination");
    }

    // ── product minimality ─────────────────────────────────────────────
    #[test]
    fn empty_product_set_is_refused() {
        let err = new_request(vec![], vue_req(VueBackendRequest::Vdom)).unwrap_err();
        assert_eq!(err, CompileRequestError::EmptyProductSet);
    }

    #[test]
    fn duplicate_product_kind_is_refused() {
        let err = new_request(
            vec![vue_client(None), vue_client(None)],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap_err();
        assert_eq!(
            err,
            CompileRequestError::DuplicateProduct(ProductKind::RuntimeClient)
        );
    }

    #[test]
    fn requesting_ide_companion_does_not_couple_to_runtime_source_map() {
        // The fixed map-coupling regression: an IDE companion alongside a
        // runtime client product must not silently flip that product's own
        // `runtime_source_map` on, and vice versa.
        let req = new_request(
            vec![
                CompileProduct::RuntimeClient(RuntimeProductRequest {
                    runtime_source_map: false,
                    ..Default::default()
                }),
                CompileProduct::IdeCompanion(IdeProductRequest {
                    want_source_map: true,
                    ..Default::default()
                }),
            ],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap();
        assert!(req.wants_ide_companion());
        assert!(
            !req.wants_runtime_source_map(),
            "requesting an IDE companion must not turn on the runtime client's own source map"
        );
        assert!(
            req.wants_ide_source_map(),
            "the IDE companion's own map demand must still take effect"
        );
    }

    #[test]
    fn runtime_source_map_stays_on_when_explicitly_requested() {
        // Positive pairing for the assertion above: the flag DOES turn on
        // when the caller actually asks for it, proving the predicate is
        // not vacuously false.
        let req = new_request(
            vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
                runtime_source_map: true,
                ..Default::default()
            })],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap();
        assert!(req.wants_runtime_source_map());
    }

    // ── zero-work planning ─────────────────────────────────────────────
    #[test]
    fn script_bindings_only_analysis_request_wants_zero_template_data_and_zero_template_codegen() {
        let req = new_request(
            vec![CompileProduct::Analysis(AnalysisProductRequest {
                want_script_bindings: true,
                want_template_data: false,
            })],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap();
        assert!(!req.wants_template_codegen());
        assert!(!req.wants_style_codegen());
        assert!(!req.wants_template_data());
        assert!(req.wants_script_output());
        assert!(req.wants_runtime_prop_constructors());
    }

    #[test]
    fn template_data_only_analysis_request_still_wants_script_output() {
        // The LSP's real `IDE | TEMPLATE_DATA` profile: template-data
        // extraction alone still needs script-level analysis as a
        // prerequisite, matching legacy `needs_script()`'s direct
        // `TEMPLATE_DATA` intersection — but does NOT publish the runtime
        // prop constructors (`SCRIPT` bit not set).
        let req = new_request(
            vec![
                CompileProduct::IdeCompanion(IdeProductRequest::default()),
                CompileProduct::Analysis(AnalysisProductRequest {
                    want_script_bindings: false,
                    want_template_data: true,
                }),
            ],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap();
        assert!(req.wants_template_data());
        assert!(req.wants_script_output());
        assert!(
            !req.wants_runtime_prop_constructors(),
            "template-data-alone must not publish runtime prop constructors"
        );
        assert!(req.wants_tsx());
    }

    #[test]
    fn runtime_client_request_wants_template_and_style_codegen_but_not_template_data() {
        // Positive pairing: the same predicates ARE true once a runtime
        // product is actually requested — and a pure runtime request does
        // NOT extract raw template data (the bundler preset never sets
        // `TEMPLATE_DATA`).
        let req = new_request(vec![vue_client(None)], vue_req(VueBackendRequest::Vdom)).unwrap();
        assert!(req.wants_template_codegen());
        assert!(req.wants_style_codegen());
        assert!(req.wants_script_output());
        assert!(!req.wants_template_data());
    }

    #[test]
    fn ide_companion_alone_wants_zero_script_output() {
        // Legacy `needs_script()` does NOT intersect `TSX` — a pure IDE
        // request runs no runtime SCRIPT-block emission.
        let req = new_request(
            vec![CompileProduct::IdeCompanion(IdeProductRequest::default())],
            vue_req(VueBackendRequest::Vdom),
        )
        .unwrap();
        assert!(req.wants_tsx());
        assert!(!req.wants_script_output());
        assert!(req.wants_runtime_macro_semantics());
    }
}
