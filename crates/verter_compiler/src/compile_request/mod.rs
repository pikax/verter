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

pub use capability::{CapabilityCell, CapabilityDisposition, ALL_CAPABILITY_CELLS};
pub use product::{
    unroutable_host_request_axis, AnalysisProductRequest, CompileProduct,
    DeclarationProductRequest, IdeProductRequest, ProductKind, PublicApiProductRequest,
    RuntimeHmrStrategy, RuntimeProductRequest, RuntimeStyleProcessing, UnroutableHostRequestAxis,
};
pub use svelte::{
    SvelteCompileRequest, SvelteOption, SvelteOptionAttempt, SvelteOptionClass,
    PRESENCE_REFUSED_SVELTE_OPTIONS, VALUE_REFUSED_SVELTE_OPTIONS,
};
pub use vue::{
    VueBackendRequest, VueCompileRequest, VueOption, VueOptionAttempt, VueOptionClass,
    PRESENCE_REFUSED_VUE_OPTIONS, VALUE_REFUSED_VUE_OPTIONS,
};

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

impl FrameworkOption {
    /// The exact (`surface`, `option`) column pair of this option's row in
    /// `vue-options.tsv` / `svelte-options.tsv`.
    pub const fn tsv_row(self) -> (&'static str, &'static str) {
        match self {
            FrameworkOption::Vue(option) => option.tsv_row(),
            FrameworkOption::Svelte(option) => option.tsv_row(),
        }
    }

    /// The framework tag this option belongs to — the `vue`/`svelte`
    /// prefix a refusal message carries, and the same discriminant a
    /// cross-framework key refusal names.
    pub const fn framework(self) -> &'static str {
        match self {
            FrameworkOption::Vue(_) => "vue",
            FrameworkOption::Svelte(_) => "svelte",
        }
    }

    /// The host compile request's own slot for this option — see
    /// [`vue::VueOption::request_field`] /
    /// [`svelte::SvelteOption::request_field`].
    pub const fn request_field(self) -> Option<&'static str> {
        match self {
            FrameworkOption::Vue(option) => option.request_field(),
            FrameworkOption::Svelte(option) => option.request_field(),
        }
    }
}

impl std::fmt::Display for FrameworkOption {
    /// The caller-facing option path a request-construction refusal names:
    /// the framework tag plus the REQUEST SCHEMA's own field path for the
    /// option ([`Self::request_field`]).
    ///
    /// The request schema is the flat, camelCase object a caller writes
    /// (`packages/native/host-compile-request.generated.ts`), so the two
    /// `compatConfig` rows the official inventory records on two different
    /// surfaces are the two distinct fields `compatConfig` and
    /// `transformCompatConfig`, and `SvelteOptions.customElement.props` +
    /// `*.type` is `customElementDescriptor.props.*.propType`. Naming the
    /// property from [`Self::tsv_row`] instead would name the OFFICIAL
    /// framework's surface, which is a different namespace: a caller told
    /// to remove `vue:compatConfig.MODE` has no such field to remove, and
    /// two distinct request fields would collapse onto one path.
    ///
    /// Deriving the path from `{self:?}` would be wrong for a third
    /// reason: the variant is `Surface_option`, so a case-lowered `Debug`
    /// spelling reads `vue:transformOptionsHoistStatic` while the request
    /// field is `hoistStatic`.
    ///
    /// An option the request carries NO slot for cannot have been written
    /// by a caller. A refusal must therefore not name one, and the two
    /// refusal families keep that true structurally rather than by
    /// convention: a presence refusal can only name a row of
    /// `PRESENCE_REFUSED_VUE_SLOTS` / `PRESENCE_REFUSED_SVELTE_SLOTS`
    /// (the loop that refuses IS that table), and a value refusal is
    /// constructed only through
    /// [`CompileRequestError::malformed_option_value`], which asserts its
    /// option against the declared `VALUE_REFUSED_*` set. Every option in
    /// either set has a request field
    /// (`every_refusable_option_names_its_own_request_field`).
    ///
    /// Should an option outside both sets ever reach here anyway, it
    /// renders as its full inventory identity
    /// (`vue:compiler-core:ParserOptions.onWarn`) — the only name that
    /// option has anywhere — rather than as a bare leaf that would read
    /// like a request field.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let framework = self.framework();
        match self.request_field() {
            Some(field) => write!(f, "{framework}:{field}"),
            None => {
                let (surface, option) = self.tsv_row();
                write!(f, "{framework}:{surface}.{option}")
            }
        }
    }
}

/// A Verter request axis that only the Vue projection implements.
///
/// These are not official-framework options (see [`FrameworkOption`] for
/// those) — they are Verter's own IDE-companion and request axes, and the
/// Svelte backend reads none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueOnlyAxis {
    /// The generated types module the Vue TSX projection imports from.
    TypesModuleName,
    /// Vue conditional-root narrowing in the TSX projection.
    ConditionalRootNarrowing,
    /// Vue strict slot typing in the TSX projection.
    StrictSlots,
}

impl VueOnlyAxis {
    /// The wire spelling of this axis on the host compile request's IDE
    /// product options — the field a caller wrote, so a refusal names it
    /// the way they spelled it.
    pub const fn wire_name(self) -> &'static str {
        match self {
            VueOnlyAxis::TypesModuleName => "typesModuleName",
            VueOnlyAxis::ConditionalRootNarrowing => "conditionalRootNarrowing",
            VueOnlyAxis::StrictSlots => "strictSlots",
        }
    }
}

impl std::fmt::Display for VueOnlyAxis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.wire_name())
    }
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
    ///
    /// `#[non_exhaustive]` so the declared-option rule below cannot be
    /// bypassed by construction: outside this crate the variant has no
    /// struct literal at all (E0639), leaving
    /// [`Self::malformed_option_value`] as the only way any other crate
    /// can produce one. Every current producer — the two bindings' decode
    /// boundaries and the host's request builder — already goes through
    /// it.
    #[non_exhaustive]
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
    /// A [`VueOnlyAxis`] set on a Svelte request. The Svelte backend reads
    /// none of these, so admitting the request would silently DROP the
    /// axis — the one outcome the admission contract forbids. Refused on
    /// presence, exactly like the Vue-only `inline` axis, which keeps its
    /// own older arm ([`Self::InlineSsrUnsupported`]) because its refusal
    /// predates this taxonomy and is asserted by name across the
    /// transports.
    VueOnlyAxisOnSvelteRequest(VueOnlyAxis),
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
    /// Runtime client and server products in one request must agree on which
    /// layer owns post-preprocessor style stages. Choosing the first product
    /// would make product ordering change compilation semantics.
    ConflictingRuntimeStyleProcessing {
        first: RuntimeStyleProcessing,
        conflicting: RuntimeStyleProcessing,
    },
    /// Authored-only style processing is currently a Vue bundler boundary.
    /// A Svelte request cannot silently carry and ignore that ownership mode.
    RuntimeStyleProcessingUnsupported {
        framework: &'static str,
        requested: RuntimeStyleProcessing,
    },
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

impl CompileRequestError {
    /// Whether a value refusal is allowed to name `option`.
    ///
    /// The declared set is [`VALUE_REFUSED_VUE_OPTIONS`] /
    /// [`VALUE_REFUSED_SVELTE_OPTIONS`], and every member of it has a
    /// request field. An option OUTSIDE it renders as its inventory
    /// identity (`vue:compiler-core:ParserOptions.onWarn`) — the option's
    /// only name anywhere, but a path no caller's request object has and
    /// so a path no caller can act on.
    pub fn is_value_refusable(option: FrameworkOption) -> bool {
        match option {
            FrameworkOption::Vue(option) => VALUE_REFUSED_VUE_OPTIONS.contains(&option),
            FrameworkOption::Svelte(option) => VALUE_REFUSED_SVELTE_OPTIONS.contains(&option),
        }
    }

    /// The ONE constructor for [`Self::MalformedOptionValue`].
    ///
    /// Presence refusals cannot drift from their declared option set,
    /// because the refusing loop IS that set
    /// (`PRESENCE_REFUSED_VUE_SLOTS` / `PRESENCE_REFUSED_SVELTE_SLOTS`).
    /// Value refusals cannot be arranged that way — each site knows its
    /// own option and its own vocabulary — so the declared set is held
    /// here instead: every construction goes through this constructor, and
    /// a site naming an option outside the declared set fails loudly in
    /// every debug build and test run rather than shipping a caller a path
    /// their request object has no field for.
    ///
    /// "Every construction" is structural across the crate boundary, not a
    /// convention: the variant is `#[non_exhaustive]`, so no other crate
    /// can write its struct literal and reach the refusal around this
    /// check. Inside this crate the assertion is the rail.
    #[track_caller]
    pub fn malformed_option_value(option: FrameworkOption, value: impl Into<String>) -> Self {
        verter_debug_assert!(
            Self::is_value_refusable(option),
            "a value refusal named {option:?}, which is absent from \
             VALUE_REFUSED_VUE_OPTIONS / VALUE_REFUSED_SVELTE_OPTIONS; add \
             its row there (and confirm it has a request_field) so the \
             refusal names a field the caller actually wrote"
        );
        Self::MalformedOptionValue {
            option,
            value: value.into(),
        }
    }
}

impl std::fmt::Display for CompileRequestError {
    /// The caller-facing sentence a transport embeds when request
    /// construction refuses — one vocabulary for every binding.
    ///
    /// It renders the REASON only, without a leading "refused" clause, so
    /// each transport keeps its own framing (the native binding's
    /// "compile request construction refused: …", the browser binding's
    /// "refused host compile request: …") while the words naming the
    /// offending option, capability, product or axis come from here. A
    /// binding that rendered `{self:?}` instead would publish the Rust
    /// variant spelling — `UnsupportedOption { option: Vue(
    /// TransformOptionsHoistStatic), capability: None }` — and two
    /// bindings that each wrote their own sentence would drift apart for
    /// the same refusal.
    ///
    /// Every value this embeds renders through its OWN `Display`
    /// ([`FrameworkOption`], [`VueOnlyAxis`], [`CapabilityCell`],
    /// [`ProductKind`], [`RuntimeStyleProcessing`]), never through a
    /// direct call to that type's spelling accessor. Each of those types
    /// owns exactly one caller-facing name, and a `Display` that is not
    /// the way its name reaches a message is a second spelling waiting to
    /// disagree with the first.
    ///
    /// Exhaustive on purpose: a new refusal arm is a compile error here
    /// rather than a message that silently reads as another arm's.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileRequestError::UnsupportedOption { option, .. } => {
                write!(f, "unsupported option '{option}'")
            }
            CompileRequestError::MalformedOptionValue { option, value } => {
                write!(f, "malformed value '{value}' for option '{option}'")
            }
            CompileRequestError::SsrVaporBackendUnsupported => {
                f.write_str("SSR is unsupported with a Vapor backend")
            }
            CompileRequestError::VueOnlyAxisOnSvelteRequest(axis) => write!(
                f,
                "Vue-only option '{axis}' is not admitted on a Svelte request"
            ),
            CompileRequestError::InlineSsrUnsupported => {
                f.write_str("inline assembly is unsupported with SSR")
            }
            CompileRequestError::VaporInlineNotYetImplemented => {
                f.write_str("inline assembly is not implemented for Vapor")
            }
            CompileRequestError::CapabilityUnsupported(cell) => {
                write!(f, "unsupported capability '{cell}'")
            }
            CompileRequestError::EmptyProductSet => f.write_str("product set is empty"),
            CompileRequestError::DuplicateProduct(kind) => {
                write!(f, "duplicate product '{kind}'")
            }
            CompileRequestError::ConflictingRuntimeStyleProcessing { first, conflicting } => {
                write!(
                    f,
                    "conflicting runtime styleProcessing values '{first}' and '{conflicting}'"
                )
            }
            CompileRequestError::RuntimeStyleProcessingUnsupported {
                framework,
                requested,
            } => write!(
                f,
                "runtime styleProcessing '{requested}' is unsupported for {framework}"
            ),
            CompileRequestError::FrameworkMismatch { expected, actual } => write!(
                f,
                "compile request framework '{actual}' does not match '{expected}'"
            ),
        }
    }
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
    /// The `ssrContext.modules` manifest key form for the host Main
    /// assembly; `None` falls back to the canonical id.
    ssr_module_id: Option<String>,
    /// Dev-server tooling flavour gating the host Main assembly's
    /// `__file` and hot-accept trailer decoration.
    hmr_strategy: RuntimeHmrStrategy,
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

        let mut runtime_style_processing = None;
        for style_processing in products.iter().filter_map(|product| match product {
            CompileProduct::RuntimeClient(runtime) | CompileProduct::RuntimeServer(runtime) => {
                Some(runtime.style_processing)
            }
            _ => None,
        }) {
            if let Some(first) = runtime_style_processing {
                if first != style_processing {
                    return Err(CompileRequestError::ConflictingRuntimeStyleProcessing {
                        first,
                        conflicting: style_processing,
                    });
                }
            } else {
                runtime_style_processing = Some(style_processing);
            }
        }

        if matches!(framework, FrameworkCompileRequest::Svelte(_))
            && runtime_style_processing == Some(RuntimeStyleProcessing::AuthoredOnly)
        {
            return Err(CompileRequestError::RuntimeStyleProcessingUnsupported {
                framework: "svelte",
                requested: RuntimeStyleProcessing::AuthoredOnly,
            });
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
        } else {
            if inline_requested {
                // `inline` is a Vue-only axis (official `compileScript({
                // inlineTemplate })` production topology); Svelte has no such
                // concept, so a caller who somehow sets it on a Svelte request
                // gets the same typed refusal rather than a silently-ignored
                // field.
                return Err(CompileRequestError::InlineSsrUnsupported);
            }
            // The IDE-companion axes, same rule. A bool axis is "present"
            // only when explicitly true: `false` IS the Svelte behaviour, so
            // refusing it would reject every default request.
            //
            // `component_id` is deliberately NOT here. It rides the request's
            // SHARED identity block, existing Svelte wire payloads set it, and
            // the Svelte backend does not read it — a real gap, but closing it
            // changes an accepted wire shape and belongs to a wire decision
            // rather than to this one.
            for product in &products {
                let CompileProduct::IdeCompanion(ide) = product else {
                    continue;
                };
                if ide.types_module_name.is_some() {
                    return Err(CompileRequestError::VueOnlyAxisOnSvelteRequest(
                        VueOnlyAxis::TypesModuleName,
                    ));
                }
                if ide.conditional_root_narrowing {
                    return Err(CompileRequestError::VueOnlyAxisOnSvelteRequest(
                        VueOnlyAxis::ConditionalRootNarrowing,
                    ));
                }
                if ide.strict_slots {
                    return Err(CompileRequestError::VueOnlyAxisOnSvelteRequest(
                        VueOnlyAxis::StrictSlots,
                    ));
                }
            }
        }

        Ok(Self {
            products,
            framework,
            semantic_profile,
            filename,
            component_id,
            is_production,
            force_js,
            ssr_module_id: None,
            hmr_strategy: RuntimeHmrStrategy::default(),
        })
    }

    /// State the host Main-assembly decoration axes: the SSR-manifest key
    /// form (`ssrContext.modules` registration — root-relative under Vite;
    /// `None` falls back to the canonical id) and the dev-server tooling
    /// flavour gating the natively composed `__file` and hot-accept
    /// trailer.
    ///
    /// These are host build knobs, not framework options; the legacy
    /// `CompileProfile` carries the same pair and treats them as inert
    /// for Svelte, which this request preserves.
    pub fn with_host_assembly_axes(
        mut self,
        ssr_module_id: Option<String>,
        hmr_strategy: RuntimeHmrStrategy,
    ) -> Self {
        self.ssr_module_id = ssr_module_id;
        self.hmr_strategy = hmr_strategy;
        self
    }

    /// The SSR-manifest key form stated by the caller, if any.
    pub fn ssr_module_id(&self) -> Option<&str> {
        self.ssr_module_id.as_deref()
    }

    /// The dev-server tooling flavour stated by the caller (`None` = no
    /// decoration).
    pub fn hmr_strategy(&self) -> RuntimeHmrStrategy {
        self.hmr_strategy
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

    /// Bind the carrier file name to the source identity this request
    /// executes against, when the caller stated none.
    ///
    /// `filename` is SOURCE identity, not demand: it names the carrier the
    /// compile runs over, and it reaches the component name, the scoped
    /// style hash, and every emitted source map's `sources` entry. A
    /// caller that hands an execution entry a canonical id has already
    /// stated that identity, so an unset slot is filled from it rather
    /// than compiled as an anonymous carrier — which would silently yield
    /// a different component name, a different scope hash, and maps with
    /// no source. A caller-stated name always wins; this never overwrites
    /// one.
    pub fn bind_default_filename(&mut self, canonical_id: &str) {
        if self.filename.is_none() {
            self.filename = Some(canonical_id.to_string());
        }
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
    fn runtime_client_with_style(style_processing: RuntimeStyleProcessing) -> CompileProduct {
        CompileProduct::RuntimeClient(RuntimeProductRequest {
            style_processing,
            ..Default::default()
        })
    }
    fn runtime_server_with_style(style_processing: RuntimeStyleProcessing) -> CompileProduct {
        CompileProduct::RuntimeServer(RuntimeProductRequest {
            style_processing,
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
    fn conflicting_runtime_style_ownership_is_refused() {
        let result = new_request(
            vec![
                runtime_client_with_style(RuntimeStyleProcessing::AuthoredOnly),
                runtime_server_with_style(RuntimeStyleProcessing::Complete),
            ],
            vue_req(VueBackendRequest::Vdom),
        );
        assert_eq!(
            result.unwrap_err(),
            CompileRequestError::ConflictingRuntimeStyleProcessing {
                first: RuntimeStyleProcessing::AuthoredOnly,
                conflicting: RuntimeStyleProcessing::Complete,
            }
        );
    }

    #[test]
    fn matching_runtime_style_ownership_is_accepted() {
        let request = new_request(
            vec![
                runtime_client_with_style(RuntimeStyleProcessing::AuthoredOnly),
                runtime_server_with_style(RuntimeStyleProcessing::AuthoredOnly),
            ],
            vue_req(VueBackendRequest::Vdom),
        )
        .expect("matching client/server style ownership must construct");
        assert_eq!(
            request.runtime_style_processing(),
            RuntimeStyleProcessing::AuthoredOnly
        );
    }

    #[test]
    fn svelte_authored_only_style_ownership_is_refused() {
        let result = new_request(
            vec![runtime_client_with_style(
                RuntimeStyleProcessing::AuthoredOnly,
            )],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
        );
        assert_eq!(
            result.unwrap_err(),
            CompileRequestError::RuntimeStyleProcessingUnsupported {
                framework: "svelte",
                requested: RuntimeStyleProcessing::AuthoredOnly,
            }
        );
    }

    // ── Vue-only axes on a Svelte request ─────────────────────────────
    // The Svelte backend reads none of these, so admitting them would
    // DROP the axis silently. Each arm is its own case: one shared
    // "any Vue axis refuses" test would pass while three of the four
    // went unchecked.

    fn svelte_ide(ide: IdeProductRequest) -> Result<CompileRequest, CompileRequestError> {
        CompileRequest::new(
            vec![CompileProduct::IdeCompanion(ide)],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            None,
            None,
            false,
            false,
        )
    }

    /// `component_id` rides the request's SHARED identity block, so it stays
    /// admitted on a Svelte request even though the Svelte backend does not
    /// read it: existing wire payloads set it, and refusing it changes an
    /// accepted wire shape — a wire decision, not part of closing the
    /// IDE-axis gap. Pinned so the exemption reads as a stated boundary
    /// rather than an oversight.
    #[test]
    fn a_svelte_request_still_admits_a_component_id() {
        CompileRequest::new(
            vec![CompileProduct::RuntimeClient(
                RuntimeProductRequest::default(),
            )],
            FrameworkCompileRequest::Svelte(SvelteCompileRequest::default()),
            None,
            None,
            Some("c-2".to_string()),
            false,
            false,
        )
        .expect("the shared identity axis stays admitted on a Svelte request");
    }

    #[test]
    fn svelte_ide_companion_refuses_a_types_module_name() {
        let err = svelte_ide(IdeProductRequest {
            types_module_name: Some("./types".to_string()),
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            CompileRequestError::VueOnlyAxisOnSvelteRequest(VueOnlyAxis::TypesModuleName)
        );
    }

    #[test]
    fn svelte_ide_companion_refuses_conditional_root_narrowing() {
        let err = svelte_ide(IdeProductRequest {
            conditional_root_narrowing: true,
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            CompileRequestError::VueOnlyAxisOnSvelteRequest(VueOnlyAxis::ConditionalRootNarrowing)
        );
    }

    #[test]
    fn svelte_ide_companion_refuses_strict_slots() {
        let err = svelte_ide(IdeProductRequest {
            strict_slots: true,
            ..Default::default()
        })
        .unwrap_err();
        assert_eq!(
            err,
            CompileRequestError::VueOnlyAxisOnSvelteRequest(VueOnlyAxis::StrictSlots)
        );
    }

    /// The bool axes default to `false`, which IS the Svelte behaviour —
    /// refusing on a `false` would reject every ordinary Svelte request.
    /// This is the test that keeps the refusal presence-based rather than
    /// field-based.
    #[test]
    fn a_default_svelte_ide_companion_still_constructs() {
        svelte_ide(IdeProductRequest::default())
            .expect("the Vue-only axes are absent by default, so nothing is dropped");
    }

    /// The same axes on a VUE request are honoured, not refused — the
    /// refusal is about the Svelte backend not reading them, not about
    /// the axes being invalid.
    #[test]
    fn the_same_axes_construct_on_a_vue_request() {
        CompileRequest::new(
            vec![CompileProduct::IdeCompanion(IdeProductRequest {
                types_module_name: Some("./types".to_string()),
                conditional_root_narrowing: true,
                strict_slots: true,
                ..Default::default()
            })],
            vue_req(VueBackendRequest::Vdom),
            None,
            None,
            Some("data-v-abc123".to_string()),
            false,
            false,
        )
        .expect("every Vue-only axis is honoured on a Vue request");
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

    /// The declared value-refusable set is exactly the options a value
    /// refusal may name, and nothing else is admitted through the one
    /// constructor.
    ///
    /// The distinction it protects: an option in the set renders as the
    /// caller's own request field, and an option outside it renders as its
    /// OFFICIAL-inventory identity — a path the caller's request object has
    /// no field for, so a path they cannot act on.
    ///
    /// Mutation recipes:
    /// - Delete a row from `VALUE_REFUSED_SVELTE_OPTIONS`: that row's
    ///   membership assertion fails, and every site constructing it trips
    ///   the constructor's own assertion.
    /// - Make `is_value_refusable` answer `true` unconditionally: the
    ///   negative half fails, and the constructor stops discriminating a
    ///   refusal site that names a slot-less option.
    #[test]
    fn only_declared_options_are_value_refusable() {
        for option in VALUE_REFUSED_VUE_OPTIONS {
            let option = FrameworkOption::Vue(option);
            assert!(
                CompileRequestError::is_value_refusable(option),
                "{option:?} is declared value-refusable but the predicate denies it"
            );
            assert!(
                option.request_field().is_some(),
                "{option:?} may be named by a value refusal but has no request field, \
                 so the refusal would quote an inventory path no caller wrote"
            );
        }
        for option in VALUE_REFUSED_SVELTE_OPTIONS {
            let option = FrameworkOption::Svelte(option);
            assert!(
                CompileRequestError::is_value_refusable(option),
                "{option:?} is declared value-refusable but the predicate denies it"
            );
            assert!(
                option.request_field().is_some(),
                "{option:?} may be named by a value refusal but has no request field, \
                 so the refusal would quote an inventory path no caller wrote"
            );
        }

        // An option with no request slot at all: a caller cannot have
        // written it, so a value refusal must never be constructed for it.
        let slotless = FrameworkOption::Vue(VueOption::ParserOptionsOnWarn);
        assert!(
            slotless.request_field().is_none(),
            "this case needs an option the request carries no slot for"
        );
        assert!(
            !CompileRequestError::is_value_refusable(slotless),
            "a slot-less option must not be admitted as value-refusable"
        );
    }

    /// The one constructor produces the arm it names, with the caller's
    /// value intact.
    #[test]
    fn the_value_refusal_constructor_carries_option_and_value() {
        let error = CompileRequestError::malformed_option_value(
            FrameworkOption::Svelte(SvelteOption::CompileOptionsCss),
            "purple",
        );
        assert_eq!(
            error,
            CompileRequestError::MalformedOptionValue {
                option: FrameworkOption::Svelte(SvelteOption::CompileOptionsCss),
                value: "purple".to_string(),
            }
        );
        assert_eq!(
            error.to_string(),
            "malformed value 'purple' for option 'svelte:css'"
        );
    }
}
