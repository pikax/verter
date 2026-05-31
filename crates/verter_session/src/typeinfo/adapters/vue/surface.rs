#![deny(missing_docs)]
//! The `.vue` FullMetadata macro surface + the prop / emit / slot normalizers.
//!
//! [`resolve_vue_macro_surface`] resolves ONE `.vue` macro's type-argument
//! surface (`defineProps<T>()` / `defineEmits<E>()` / `defineSlots<S>()` /
//! `withDefaults(defineProps<T>(), …)`) to the span-rich [`VueMacroSurface`]
//! at [`TypeInfoQueryLevel::FullMetadata`]. The surface is sourced through the
//! SHARED typeinfo surface path — the macro type-argument is lowered through
//! the shared lowering dispatch and projected by the SAME empty-path `Shallow`
//! synthesiser [`crate::VerterHost::resolve_shallow_surface_for`] uses. It is
//! NEVER read from `surface_view_from_base_node` — the macro surface routes
//! through the one shared surface path, not a parallel reader.
//!
//! The three normalizers ([`props_from_typeinfo_surface`] /
//! [`emits_from_typeinfo_surface`] / [`slots_from_typeinfo_surface`]) consume a
//! [`crate::typeinfo::surface::TypeInfoSurface`] (member `value` +
//! spans + origin + flags + JSDoc spans) plus the macro-analyzer facts and
//! produce the FINAL component-meta DTOs (`AnalyzedPropField` /
//! `AnalyzedEmitField` / `AnalyzedSlotField`). They reproduce the eager rail's
//! behavior (the `ImportedMacroSurface::LazyImported` arm +
//! `surface_projector`) member-for-member, sourcing every semantic decision
//! from the typeinfo surface:
//!
//! - **props** — one field per named member, carrying the surface's `optional`
//!   / `readonly` (RICHER than the eager rail's hardcoded `false` — taken from
//!   the surface) / `declared_in_macro_type_arg`, the member value raised to a
//!   `TypeExpr` (scoped to the member's VALUE-NODE file, matching the eager
//!   rail — see [`VueMacroSurface::member_expr_scope`]), the `defineModel`
//!   synthesized model prop from analyzer facts, and JSDoc sliced from the
//!   surface's JSDoc SPANS.
//! - **emits** — call-signature event extraction FIRST (the first parameter's
//!   string-literal — or union of string literals — is the event name; the
//!   payload is the call-signature function with the leading event-name
//!   parameter STRIPPED), property-key members only as a fallback when no
//!   call-signature emit was found, de-duplicated by event name
//!   (first-writer-wins).
//! - **slots** — function-like members only (non-function members filtered);
//!   the first-parameter object's properties become the slot bindings; the
//!   function return type becomes the slot return.
//!
//! Fallthrough / root-inheritance + expose / options are SEPARATE subsystems
//! fed by analyzer facts — out of scope here.

use std::sync::Arc;

use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, SurfaceProvenanceContext,
};
use crate::typeinfo::adapters::vue::store::{VueMacroDtoKey, VueMacroDtos};
use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::VerterHost;

/// A typeinfo-owned `.vue` macro surface (FullMetadata).
///
/// Carries the span-rich [`TypeInfoSurface`] for ONE macro's type argument
/// plus the macro's kind, declaration identity, and the SFC scope the macro
/// was written in. Like the surface itself it holds NO owned type / JSDoc text
/// — only the surface (spans + ids + flags), the macro kind, and interned
/// `Arc<str>` scope / canonical ids. A consumer slices source on demand at the
/// FFI boundary; the normalizers in this module do that slicing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroSurface {
    /// The macro's span-rich one-level surface (the type argument's members /
    /// call signatures / index signatures).
    pub surface: TypeInfoSurface,
    /// Which macro produced this surface.
    pub macro_kind: AnalyzedMacroKind,
    /// Canonical id of the `.vue` SFC that declares the macro.
    pub owner_canonical: Arc<str>,
    /// Stable index of the macro in the SFC's analysis snapshot.
    pub macro_index: usize,
    /// SFC-absolute span of the macro CALL (from the analyzer fact). The
    /// surface members carry their own per-member declaration spans; this is
    /// the macro call site itself.
    pub macro_call_span: verter_span::Span,
    /// The query level this surface was resolved at (always
    /// [`TypeInfoQueryLevel::FullMetadata`] for a macro surface).
    pub level: TypeInfoQueryLevel,
}

impl VueMacroSurface {
    /// The scope a member's raised `*_expr` should bind to — the member's
    /// VALUE-NODE scope (`node_scope(member.value)` → file), matching the eager
    /// rail's `ImportedMacroSurface::member_expr_scope`.
    ///
    /// The value-node scope (NOT the member's declaration_origin) is the file
    /// whose OXC parse produced the typed value expression, which is where its
    /// nested `Ref`s must resolve. The two files DIVERGE for a generic
    /// inherited member: `interface Props extends Base<Local>` over an imported
    /// `Base<T> { val: T }` substitutes `val`'s value to `Local`, a node scoped
    /// to the DERIVING file (where `Local` lives), while the member's
    /// declaration_origin is the base file (where `val: T` is declared). Scoping
    /// the raised `*_expr` to the declaration_origin (base) would make
    /// `Ref("Local")` resolve in the wrong file — a cross-file Miss. JSDoc
    /// deliberately uses the declaration_origin instead (see
    /// [`member_jsdoc_from_spans`]); the two axes intentionally use different
    /// files, exactly as the eager rail does.
    ///
    /// Falls back to the member's declaration_origin, then the SFC owner, when
    /// the value node carries no single-file scope (a structural / scope-less
    /// value node — a primitive, a shared literal-union).
    fn member_expr_scope(
        &self,
        host: &VerterHost,
        member: &TypeInfoSurfaceMember,
    ) -> TypeExprScope {
        host.project_type_store()
            .semantic_graph()
            .node_scope(member.value)
            .and_then(|scope| scope.canonical_file())
            .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            .or_else(|| {
                member
                    .origin
                    .canonical_file
                    .as_ref()
                    .map(|canonical| TypeExprScope::new(canonical.as_ref()))
            })
            .unwrap_or_else(|| TypeExprScope::new(self.owner_canonical.as_ref()))
    }

    /// The scope a call signature's stripped-payload `*_expr` should bind to —
    /// the signature's DECLARATION-origin file, derived from its spans (each
    /// [`crate::typeinfo::surface::CanonicalSpan`] carries the file the offsets
    /// index into). For a cross-file emit interface's call signature the spans
    /// live in the heritage base's file, so the payload `Ref`s resolve THERE —
    /// the file the call signature is DECLARED in. This is the correct scope
    /// even when the SFC instantiates a generic emit interface
    /// (`Emits extends TabsRootEmits<string | number>`): the call signature is
    /// declared in the package, and the SFC-supplied generic argument is encoded
    /// in the typed `payload_expr` (a `Tuple` whose element types carry their
    /// own scope), NOT by re-anchoring the whole signature's scope to the SFC.
    ///
    /// Falls back to the SFC owner when the signature carries no span (a
    /// synthetic / composed signature).
    fn signature_expr_scope(
        &self,
        sig: &crate::typeinfo::surface::TypeInfoSurfaceSignature,
    ) -> TypeExprScope {
        sig.signature_span
            .as_ref()
            .or(sig.return_type_span.as_ref())
            .or_else(|| sig.parameter_spans.iter().flatten().next())
            .map(|cspan| TypeExprScope::new(cspan.file.as_ref()))
            .unwrap_or_else(|| TypeExprScope::new(self.owner_canonical.as_ref()))
    }
}

impl VerterHost {
    /// Resolve a `.vue` macro's type-argument surface to its span-rich
    /// [`VueMacroSurface`] (FullMetadata) through the shared typeinfo surface
    /// path.
    ///
    /// Returns `None` when the SFC is not loaded, the macro index is out of
    /// range, the macro is not type-based / has no parsed type argument, or the
    /// type argument does not project to an object surface (a macro typed as a
    /// primitive / union has no one-level member surface).
    ///
    /// **`withDefaults` returns `None` here — by design.** The analyzer routes
    /// `withDefaults(defineProps<Props>(), { … })` as TWO macros: a `DefineProps`
    /// macro carrying the `Props` type argument AND an OUTER `WithDefaults`
    /// macro that has NO type argument (the type parameter lives on the inner
    /// `defineProps`, never on the `withDefaults` call — `withDefaults<…>(…)` is
    /// not valid Vue). The outer macro is therefore `is_type_based == false` and
    /// falls out at the `is_type_based` guard below. The props surface comes
    /// from the SEPARATELY-routed inner `DefineProps` macro, exactly as the
    /// eager rail resolves it (the `WithDefaults` macro's `resolved_local_types`
    /// is empty, so the eager `cold_resolver` macro loop projects nothing for
    /// it either). The defaults the `withDefaults` call supplies flip
    /// `required` / `has_default` DOWNSTREAM at the component-meta PropAnalysis
    /// layer, not on this surface.
    ///
    /// **Demand + provenance:** macro-object DTO synthesis lowers the type
    /// argument under the Vue
    /// [`ProjectionReductionContext::macro_object_surface`] demand
    /// ([`crate::semantic_query::ReductionDemand::MacroObjectSurface`]), NOT
    /// ordinary `Published`. For a `Union`-rooted type argument the macro
    /// surface is the UNION of object-arm members (a member present in ANY arm
    /// is part of the component macro surface — the Vue macro convention),
    /// whereas ordinary `Published(Shallow)` would synthesise the TS
    /// property-access INTERSECTION of common members and drop every
    /// branch-only prop / event / slot. The props macro (`DefineProps`) keeps
    /// the [`SurfaceProvenanceContext::MacroTypeArgOwnBody`] provenance so the
    /// type-argument's OWN-body members surface with
    /// `declared_in_macro_type_arg = true` and heritage-reached members stay
    /// `false`; structural macros (`DefineEmits` / `DefineSlots`) lower under
    /// [`SurfaceProvenanceContext::Structural`] (`declared_in_macro_type_arg`
    /// is a props-axis concern).
    #[must_use]
    pub fn resolve_vue_macro_surface(
        &self,
        request: &VueMacroSurfaceRequest,
    ) -> Option<VueMacroSurface> {
        // Base-view entry point (tests, the host-method `vue_macro_dtos`
        // wrapper). Builds a bare `HostResolverContext` over the base host view
        // and routes the single resolution core through it. Overlay-bearing
        // production callers MUST use `resolve_vue_macro_surface_with_ctx` with
        // their active session context so the surface reads overlay content.
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        self.resolve_vue_macro_surface_with_ctx(&host_ctx, request)
    }

    /// Context-bound resolution core for [`Self::resolve_vue_macro_surface`].
    ///
    /// Every view-sensitive read — the owner SFC's `IndexedReady`, the type
    /// argument lowering, the cross-file carrier projection, the carrier-file
    /// JSDoc source — flows through `ctx`, so an overlay session
    /// ([`crate::resolver_core::session_resolver_context::SessionResolverContext`])
    /// resolves the macro surface against its overlay content rather than the
    /// base host view. The dispatcher is `ctx.dispatch()` (the sealed
    /// `ProjectSemanticDispatch::new(ctx)`), keeping the surface inside the
    /// single resolution engine.
    #[must_use]
    pub(crate) fn resolve_vue_macro_surface_with_ctx(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        request: &VueMacroSurfaceRequest,
    ) -> Option<VueMacroSurface> {
        debug_assert_eq!(
            request.level,
            TypeInfoQueryLevel::FullMetadata,
            "resolve_vue_macro_surface serves the FullMetadata level"
        );

        let indexed = ctx.ensure_indexed_ready(request.owner_canonical.as_ref())?;
        let mac = indexed.snapshot.macros.get(request.macro_index)?;
        if !mac.is_type_based {
            return None;
        }

        // `defineModel` does NOT carry a props OBJECT type argument — its type
        // argument is the model VALUE type (`defineModel<string>()`), which has
        // no one-level member surface. Its props come from the analyzer-
        // synthesized model prop (`AnalyzedMacro.prop_fields`), so the macro
        // surface is the EMPTY object surface; `props_from_typeinfo_surface`
        // routes `DefineModel` to the analyzer-fact path.
        if request.macro_kind == AnalyzedMacroKind::DefineModel {
            return Some(VueMacroSurface {
                surface: TypeInfoSurface::empty(),
                macro_kind: request.macro_kind,
                owner_canonical: Arc::clone(&request.owner_canonical),
                macro_index: request.macro_index,
                macro_call_span: mac.span,
                level: request.level,
            });
        }

        let type_arg = mac.parsed_type_argument.as_ref()?;

        // Provenance per macro axis. Props request the macro-T own-body
        // provenance on the terminal surface synthesis so the author-declared
        // members are flagged; emits / slots are structural
        // (`declared_in_macro_type_arg` is a props-axis concern). The terminal
        // `MacroTypeArgOwnBody` synthesis restamps `declared_in_macro_type_arg =
        // true` for EXACTLY the declaration's own-body direct members — it reads
        // the prepared decl's `member_index`, which is populated from direct
        // Object members only and SKIPS heritage `extends` `Ref` arms
        // (`build.rs::overlay_macro_type_arg_own_body`). Heritage-reached members
        // are NOT in `member_index`, so they are left at the structural `false`
        // the empty-path Shallow body lowering assigned. The surface's
        // `merge_role` is independently baked per arm (`Heritage` for
        // `extends`-reached members, `OwnBody` for the declaration's own body).
        // See `props_from_typeinfo_surface`.
        // Only `DefineProps` reaches here as a props macro: `WithDefaults` is
        // never `is_type_based` (it bailed at the guard above) and `DefineModel`
        // returned its empty surface above. `DefineProps` requests the macro-T
        // own-body provenance; emits / slots are structural.
        let terminal_context = match request.macro_kind {
            AnalyzedMacroKind::DefineProps => ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::MacroTypeArgOwnBody,
            ),
            _ => ProjectionReductionContext::macro_object_surface(
                ProjectionMode::Shallow,
                SurfaceProvenanceContext::Structural,
            ),
        };

        // Dispatch is bound to the active `ctx`: an overlay session threads its
        // session view through every dispatch-tier read, so the type-argument
        // lowering and cross-file carrier projection below read overlay content.
        let dispatch = ctx.dispatch();

        // Path-precise decomposition of a deep indexed-access type argument
        // (`defineProps<DeepConfig['ui']['header']>()`). The SAME decomposition
        // the transit-shallow Class-A projector uses: the base
        // (`DeepConfig` / `WrappedConfig<Theme>`) is lowered as the carrier and
        // the string-literal hops (`['ui']`, `['header']`) become the
        // `ProjectPath` selector. The shared path walker then runs the
        // intermediate hops in `Navigate` and the TERMINAL hop under
        // `terminal_context` (Shallow), so the leaf object's members surface
        // WITHOUT the intermediate siblings leaking. Lowering the WHOLE chain as
        // a single node and projecting the empty path would instead leave an
        // unreduced `IndexedAccess` carrier whose one-level surface is empty —
        // the leaf would be lost. A non-indexed type argument decomposes to
        // `(type_arg, [])`, preserving the prior empty-path behaviour exactly.
        let (base_expr, path) =
            crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(type_arg);

        // Lower the carrier base in the SFC scope. `Navigate` /
        // structural-transit lowering keeps member values shallow; the
        // path-precise `Shallow` projection then synthesises the one-level
        // surface of the terminal hop under `terminal_context`.
        let base = dispatch.lower_type_expr_in_scope_with_context(
            request.owner_canonical.as_ref(),
            base_expr,
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        )?;

        let surface =
            self.project_shallow_surface_from_base(ctx, &dispatch, base, path, terminal_context)?;

        Some(VueMacroSurface {
            surface,
            macro_kind: request.macro_kind,
            owner_canonical: Arc::clone(&request.owner_canonical),
            macro_index: request.macro_index,
            macro_call_span: mac.span,
            level: request.level,
        })
    }

    /// Resolve a `.vue` macro's NORMALIZED component-meta DTOs
    /// ([`VueMacroDtos`]), consulting the host-owned
    /// [`crate::typeinfo::adapters::vue::store::VueShallowMetadataStore`] first.
    ///
    /// This is the cached FullMetadata entry point: it materializes the macro
    /// surface ONCE per `(canonical, content, macro, level)` (resolving the
    /// surface and running the appropriate normalizer), publishes the immutable
    /// owned DTO bundle into the store, and serves subsequent calls from the
    /// content-addressed cache. The DTO bundle is generation-independent (owned
    /// `TypeExpr` + scope + `String`), so caching it across requests is safe.
    ///
    /// Returns an empty (default) bundle when the macro surface cannot be
    /// resolved — the same "no surface" outcome the eager rail produces for an
    /// unresolvable macro. The bundle is still cached so a repeat request does
    /// not re-attempt the cold resolution.
    #[must_use]
    pub fn vue_macro_dtos(&self, request: &VueMacroSurfaceRequest) -> Arc<VueMacroDtos> {
        // Base-view entry point (tests + the materialiser's `typeinfo_macro_dtos`
        // helper, until the materialiser is retired). Routes the single DTO
        // resolution core through a bare `HostResolverContext` over the base
        // host view. Overlay-bearing production callers
        // (`component_meta_resolved_macros`) MUST call `vue_macro_dtos_with_ctx`
        // with their active session context so the surface reads overlay content.
        let store_view = self.resolver_store_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
        vue_macro_dtos_with_ctx(&host_ctx, request)
    }

}

/// Navigate a `TypeExpr` to its one-level object [`TypeInfoSurface`] through the
/// SHARED resolver bound to the ACTIVE `ctx`, lowering it in `scope_canonical`
/// then projecting the empty-path `Shallow` surface — the SAME machinery
/// [`VerterHost::resolve_shallow_surface_for`] uses for a named declaration.
///
/// Used by the slot-binding extractor to resolve a slot's first-parameter type
/// (`Pick<RowApi, 'name'>` / a named alias / a parenthesized form) to the
/// binding object WITHOUT a nominal shape-sniff: `Pick` is navigated,
/// `Parenthesized` is unwrapped, and an alias `Ref` is resolved by the one
/// shared resolver rather than a per-utility special case. Returns `None` when
/// the scope file is not loaded or the type does not project to an object
/// surface (a primitive / union first param has no binding object).
///
/// Bound to `ctx` (`ctx.dispatch()`), NOT a fresh base `HostResolverContext`, so
/// an overlay session resolves the slot-param object against its OVERLAY content
/// — a `defineSlots<Slots>()` whose `Slots` alias is overlaid reads the overlay
/// bindings, not the base.
#[must_use]
pub(crate) fn navigate_param_to_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical: &str,
    param_ty: &TypeExpr,
) -> Option<TypeInfoSurface> {
    let dispatch = ctx.dispatch();

    // Lower the parameter type in its scope under structural-transit
    // Navigate (member values stay shallow); the empty-path Shallow
    // projection then synthesises the one-level object surface.
    let base = dispatch.lower_type_expr_in_scope_with_context(
        scope_canonical,
        param_ty,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )?;
    // Open-generic gate: a slot-param root that is symbolic-only (an open
    // Conditional whose check carries a free `TypeParam`, an unresolved
    // `IndexedAccess` / `Mapped`, …) must NOT be materialised into a
    // committed object surface — doing so would invent phantom bindings from
    // an undetermined generic context. This is the SAME gate the
    // graph-native slot-binding synthesis applies; routing both binding
    // paths through it keeps them in agreement (a `generic="M"` component's
    // `(props: SlotProps<M>)` slot resolves to NO bindings on both paths,
    // not a branch-committed guess).
    if crate::meta_resolve::slot_binding_graph::slot_param_root_is_symbolic_only(
        &dispatch, base, 0,
    ) {
        return None;
    }
    ctx.host_for_fact_tracer_install().project_shallow_surface_from_base(
        ctx,
        &dispatch,
        base,
        Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
        ProjectionReductionContext::published(ProjectionMode::Shallow),
    )
}

/// Slice a member's leading-JSDoc DESCRIPTION + TAG spans into owned text for
/// the published DTO. The spans are already located on the surface (by
/// `with_member_jsdoc_spans`); this reads the declaring file's cache-owned
/// source and slices — it does NOT re-locate the comment block and does NOT
/// take the lazy `member_display_jsdoc` name-search path.
///
/// Returns `(None, empty)` when the member carries no JSDoc spans or the
/// declaring file's source is unavailable.
/// Slice a [`CanonicalSpan`]'s byte range out of its file's cache-owned RAW
/// source (`IndexedReady.raw_source`). [`CanonicalSpan`] offsets are
/// SFC-absolute (the eval source is position-preserving, so OXC stamps spans
/// in raw-file coordinates), so the slice indexes the raw source directly.
/// `None` when the file is not loaded or the byte range is out of bounds (a
/// stale / synthetic span). This is the single source-slicing primitive the
/// normalizers use to materialize display text from a span at the consumer
/// boundary — it does NOT re-resolve or re-parse.
fn slice_canonical_span(host: &VerterHost, cspan: &CanonicalSpan) -> Option<String> {
    let indexed = host.ensure_indexed_ready(cspan.file.as_ref())?;
    let source = Arc::clone(&indexed.raw_source);
    let start = cspan.span.start as usize;
    let end = cspan.span.end as usize;
    source.get(start..end).map(|s| s.to_string())
}

/// Normalize a multi-line JSDoc description/tag body sliced from a span.
///
/// A description/tag span is a contiguous `[start, end)` region whose FIRST line
/// already had its leading `/**`-decoration stripped (the span starts at the
/// content), but whose CONTINUATION lines still carry the `   * ` JSDoc
/// decoration verbatim (the span is contiguous source text). The published
/// `description` is DISPLAY text, not comment syntax, so strip each
/// continuation line's leading whitespace + optional single `*` decoration —
/// matching `verter_semantic::analysis::jsdoc`'s per-line stripping — and rejoin
/// with `\n`. A single-line body is returned trimmed.
pub(crate) fn normalize_jsdoc_body(raw: &str) -> String {
    let mut lines = raw.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end());
    }
    for line in lines {
        out.push('\n');
        // Strip leading whitespace, then a single `*` decoration, then the
        // whitespace after it.
        let trimmed = line.trim_start();
        let stripped = trimmed
            .strip_prefix('*')
            .map(|rest| rest.trim_start())
            .unwrap_or(trimmed);
        out.push_str(stripped.trim_end());
    }
    out.trim().to_string()
}

/// Slice a leading-JSDoc description span + tag spans into the published
/// `(description, tags)` display pair. Shared by the member path
/// ([`member_jsdoc_from_spans`]) and the call-signature emit path
/// ([`signature_jsdoc_from_spans`]) — both anchor JSDoc on the typeinfo
/// surface's spans, never a reparse.
fn jsdoc_from_spans(
    host: &VerterHost,
    description_span: Option<&CanonicalSpan>,
    tag_spans: &[crate::typeinfo::surface::JsdocTagSpan],
) -> (Option<String>, Vec<JsdocTag>) {
    let slice = |cspan: &CanonicalSpan| -> Option<String> { slice_canonical_span(host, cspan) };

    let description = description_span
        .and_then(&slice)
        .map(|text| normalize_jsdoc_body(&text))
        .filter(|text| !text.is_empty());

    let tags: Vec<JsdocTag> = tag_spans
        .iter()
        .filter_map(|tag| {
            let name = slice(&tag.name_span)?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let text = tag
                .text_span
                .as_ref()
                .and_then(&slice)
                .map(|t| normalize_jsdoc_body(&t))
                .filter(|t| !t.is_empty());
            Some(JsdocTag { name, text })
        })
        .collect();

    (description, tags)
}

fn member_jsdoc_from_spans(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        member.jsdoc_description_span.as_ref(),
        &member.jsdoc_tag_spans,
    )
}

/// Slice a call/construct signature's leading-JSDoc into `(description, tags)`.
/// A call-signature emit (`(e: 'change', v: T): void`) documents the event via
/// the JSDoc on the signature itself — extracted here from the signature's
/// typeinfo JSDoc spans (symmetric with [`member_jsdoc_from_spans`]).
fn signature_jsdoc_from_spans(
    host: &VerterHost,
    sig: &crate::typeinfo::surface::TypeInfoSurfaceSignature,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        sig.jsdoc_description_span.as_ref(),
        &sig.jsdoc_tag_spans,
    )
}

/// Raise a member's value node to a [`TypeExpr`] through the shared structural
/// raiser bound to the ACTIVE `ctx` (`ctx.dispatch()`), so an overlay session
/// raises against overlay content rather than a fresh base host view. `None`
/// when the node has no raisable shape (the caller substitutes the eager rail's
/// missing-`type_expr` fallback).
fn raise_member_value(
    ctx: &dyn crate::resolver_core::ResolverContext,
    member: &TypeInfoSurfaceMember,
) -> Option<TypeExpr> {
    ctx.dispatch().raise_node_to_type_expr(member.value)
}

/// Realize a slot member's value to its underlying callable through the SHARED
/// `realize_callable_member` substrate (Alias / Conditional / InstantiationRef /
/// DeclRef carrier normalization), then raise the realized node to a
/// [`TypeExpr`]. Falls back to the un-realized value when realization finds no
/// callable (the member is then classified non-function by the caller). This
/// keeps the DTO slot surface in agreement with
/// `slot_binding_graph::compute_bindings_via_graph`, which realizes the same
/// member value before reading `Function.params`.
fn raise_realized_callable_member_value(
    ctx: &dyn crate::resolver_core::ResolverContext,
    member: &TypeInfoSurfaceMember,
) -> Option<TypeExpr> {
    let dispatch = ctx.dispatch();
    let realized = crate::meta_resolve::dispatch_helpers::realize_callable_member(
        &dispatch,
        member.value,
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Shallow,
        ),
    )
    .unwrap_or(member.value);
    dispatch.raise_node_to_type_expr(realized)
}

/// Resolve a `.vue` macro's NORMALIZED component-meta DTOs
/// ([`VueMacroDtos`]) through the active [`crate::resolver_core::ResolverContext`].
///
/// This is the SINGLE DTO resolution core: it materializes the macro surface
/// ONCE per `(canonical, content, macro, level)` (resolving the surface through
/// `ctx` and running the appropriate normalizer), publishes the immutable owned
/// DTO bundle into the host-owned
/// [`crate::typeinfo::adapters::vue::store::VueShallowMetadataStore`], and
/// serves subsequent calls from the content-addressed cache.
///
/// EVERY view-sensitive step flows through `ctx`:
/// - the owner SFC's `IndexedReady` ([`ctx.ensure_indexed_ready`]) — so an
///   overlay session keys on its OVERLAY `whole_hash`, never the base hash, and
///   a base session can never read or poison an overlay entry (or vice-versa);
/// - the cold surface resolution
///   ([`crate::VerterHost::resolve_vue_macro_surface_with_ctx`], whose dispatch
///   is `ctx.dispatch()`) — so the type-argument lowering and cross-file
///   carrier projection read overlay content;
/// - warm validation ([`ctx.store_view`]) — so a carrier edit (which leaves the
///   SFC's own `whole_hash` unchanged) invalidates the entry lazily against the
///   SAME view the surface was resolved under.
///
/// The DTO bundle is generation-independent (owned `TypeExpr` + scope +
/// `String`), so caching it across requests is safe. Returns an empty (default)
/// bundle when the macro surface cannot be resolved.
///
/// [`ctx.ensure_indexed_ready`]: crate::resolver_core::ResolverContext::ensure_indexed_ready
/// [`ctx.dispatch()`]: crate::resolver_core::ResolverContext::dispatch
/// [`ctx.store_view`]: crate::resolver_core::ResolverContext::store_view
#[must_use]
pub(crate) fn vue_macro_dtos_with_ctx(
    ctx: &dyn crate::resolver_core::ResolverContext,
    request: &VueMacroSurfaceRequest,
) -> Arc<VueMacroDtos> {
    let host = ctx.host_for_fact_tracer_install();

    // Load the CURRENT (overlay-aware) `IndexedReady` BEFORE touching the
    // cache. The request's `root_identity` (a `whole_hash` hint) and
    // `macro_kind` are caller-supplied and may be STALE or WRONG; deriving both
    // from the authoritative `ctx`-resolved snapshot here means a stale
    // `root_identity` can never read an old entry (the live `whole_hash` keys a
    // fresh slot) and a wrong `macro_kind` can never read or poison the sibling
    // kind's entry.
    let Some(indexed) = ctx.ensure_indexed_ready(request.owner_canonical.as_ref()) else {
        // SFC not loaded — no surface, no cache entry. Returning the default
        // bundle WITHOUT publishing (we have no validated key) keeps the cache
        // free of entries keyed on an unvalidated identity.
        return Arc::new(VueMacroDtos::default());
    };
    let Some(mac) = indexed.snapshot.macros.get(request.macro_index) else {
        return Arc::new(VueMacroDtos::default());
    };
    let whole_hash = indexed.whole_hash;
    let macro_kind = mac.kind;

    let key = VueMacroDtoKey::new(
        Arc::clone(&request.owner_canonical),
        whole_hash,
        request.macro_index,
        macro_kind,
        request.level,
    );
    // Warm read against the SAME `ctx` view the surface resolves under. The
    // content-addressed key covers the SFC's OWN content, but the resolved DTOs
    // read CROSS-FILE carrier types; validating the recorded fact signature +
    // project generation against the live view invalidates the entry lazily on
    // a carrier edit.
    let generation = ctx.project_type_store().current_project_generation();
    if let Some(cached) =
        host.vue_shallow_metadata_store()
            .get_with_view(&key, ctx.store_view(), generation)
    {
        // Bubble the cached entry's cross-file carrier fact signature into any
        // active outer fact tracer so an outer component-meta cold trace
        // inherits the DTO's carrier facts on this warm hit (a carrier edit that
        // invalidates this DTO entry must also invalidate the component-meta
        // entry that read it).
        cached.read_set_signature.bubble_via_tls();
        return std::sync::Arc::clone(&cached.dtos);
    }

    // Resolve the surface through a request carrying the VALIDATED identity
    // (live `whole_hash`) and the AUTHORITATIVE kind, so the surface resolution
    // + normalizer dispatch never trust the caller's hint. The whole cold
    // resolution runs under an installed fact tracer so the CROSS-FILE carrier
    // facts it reads are captured into the entry's `ReadSetSignature`.
    let validated_request = VueMacroSurfaceRequest {
        owner_canonical: Arc::clone(&request.owner_canonical),
        macro_index: request.macro_index,
        macro_kind,
        root_identity: whole_hash,
        level: request.level,
    };
    let (dtos, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
        match host.resolve_vue_macro_surface_with_ctx(ctx, &validated_request) {
            Some(macro_surface) => match macro_kind {
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineModel => VueMacroDtos {
                    props: props_from_typeinfo_surface(ctx, &macro_surface),
                    // A props member is `properties + index signatures`: a
                    // `defineProps<{ [k: string]: string }>()` surface carries an
                    // index signature with no named property, so capture the
                    // surface's index signatures (key/value raised through `ctx`)
                    // for `define_props_shape` to publish. `DefineModel`'s surface
                    // is empty, so this is an empty vec for it.
                    prop_index_signatures: prop_index_signatures_from_surface(ctx, &macro_surface),
                    ..VueMacroDtos::default()
                },
                AnalyzedMacroKind::DefineEmits => VueMacroDtos {
                    emits: emits_from_typeinfo_surface(ctx, &macro_surface),
                    ..VueMacroDtos::default()
                },
                AnalyzedMacroKind::DefineSlots => VueMacroDtos {
                    slots: slots_from_typeinfo_surface(ctx, &macro_surface),
                    ..VueMacroDtos::default()
                },
                // `WithDefaults` is not a props-surface source on this path: the
                // outer `withDefaults` macro carries no type argument (it is not
                // `is_type_based`), so `resolve_vue_macro_surface_with_ctx`
                // returns `None` for it and this arm is unreachable. The props
                // come from the SEPARATELY-routed inner `DefineProps` macro.
                // Options / expose are separate subsystems. None of these
                // contribute a DTO bundle.
                AnalyzedMacroKind::WithDefaults
                | AnalyzedMacroKind::DefineOptions
                | AnalyzedMacroKind::DefineExpose => VueMacroDtos::default(),
            },
            None => VueMacroDtos::default(),
        }
    });

    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(facts) => {
            let entry = crate::typeinfo::adapters::vue::store::VueMacroDtosEntry {
                dtos: std::sync::Arc::new(dtos),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                validated_at_generation: generation,
            };
            std::sync::Arc::clone(&host.vue_shallow_metadata_store().insert(key, entry).dtos)
        }
        // Tracer overflowed: the DTOs are valid but cannot be admitted safely
        // (the observation set was truncated, so warm-read validation could
        // falsely pass against a changed carrier). Return the freshly-computed
        // bundle WITHOUT caching — a repeat request recomputes, never serves an
        // under-validated entry.
        crate::resolver_core::FactReadSetFinalise::Overflow => std::sync::Arc::new(dtos),
    }
}

/// Normalize a `.vue` props macro surface into the published
/// [`AnalyzedPropField`] set.
///
/// Reproduces the eager rail's `AnalyzedPropField` stream
/// (`surface_projector::project_macro_surfaces` for the local SFC,
/// `ImportedMacroSurface::prop_members` for cross-file) over the typeinfo
/// surface: one field per named member, carrying the surface's `optional` /
/// `readonly` / `declared_in_macro_type_arg`, the member value raised to a
/// `TypeExpr` scoped to its VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]), the display `type_annotation`
/// rendered from that typed form, and JSDoc sliced from the surface spans.
/// Own-body-vs-heritage ordering + shadowing + union-common
/// membership are ALREADY resolved on the surface (the merge ran in the shared
/// projector) — this is a thin per-member transform.
///
/// `defineModel` does NOT carry an object type argument (its type argument is
/// the MODEL value type), so its surface has no named members; the synthesized
/// model prop is appended from the analyzer facts
/// ([`AnalyzedMacroKind::DefineModel`]'s `prop_fields`). The optionality the
/// eager rail derives from `withDefaults` / `defineModel` defaults is applied
/// DOWNSTREAM by the component-meta projection (`PropAnalysis.required` /
/// `has_default`), NOT on `AnalyzedPropField`, so the field's `is_optional`
/// here stays the RAW type-argument optionality — matching the eager rail.
#[must_use]
pub(crate) fn props_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    // Host-level reads (graph node scope, JSDoc source slicing) go through the
    // host the active `ctx` is installed against; the view-sensitive type
    // resolution (`raise_member_value`) flows through `ctx` so an overlay
    // session raises member values against overlay content.
    let host = ctx.host_for_fact_tracer_install();
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type, not a props
    // object). Source from `AnalyzedMacro.prop_fields` (populated by the
    // analyzer's `extract_define_model_type`) — the model prop is genuinely
    // analyzer-derived, not a macro-T object surface member.
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(ctx, macro_surface);
    }

    macro_surface
        .surface
        .members
        .iter()
        .map(|member| {
            let type_expr = raise_member_value(ctx, member);
            let type_expr_scope = type_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let type_annotation = type_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // `declared_in_macro_type_arg`: a member belongs to the macro-T own
            // body iff it is NOT heritage-reached. The terminal
            // `MacroTypeArgOwnBody` synthesis already stamps this correctly — it
            // restamps `true` ONLY for the declaration's own-body
            // `member_index` members and leaves heritage-reached members at
            // `false` (`build.rs::overlay_macro_type_arg_own_body` skips
            // `extends` arms). So `member.declared_in_macro_type_arg` is already
            // authoritative. The `&& merge_role != Heritage` conjunct is
            // REDUNDANT defense-in-depth: a member can only carry
            // `declared_in_macro_type_arg == true` if it is an own-body
            // `member_index` member, which is never `merge_role == Heritage`.
            // It is kept as a belt-and-braces cross-check of the two
            // independently-baked provenance facts (the restamped flag and the
            // merge role), not because the surface over-stamps heritage members.
            let declared_in_macro_type_arg = member.declared_in_macro_type_arg
                && member.origin.merge_role != crate::semantic_query::MemberMergeRole::Heritage;
            AnalyzedPropField {
                name: member.name.as_ref().to_string(),
                is_optional: member.optional,
                span: verter_span::Span::default(),
                type_annotation,
                type_expr,
                type_expr_scope,
                description,
                tags,
                resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                resolution_error: None,
                declared_in_macro_type_arg,
            }
        })
        .collect()
}

/// Normalize a props macro surface's INDEX SIGNATURES into the published
/// [`ExpandedIndexSignature`] set. A props member is `properties + index
/// signatures`, so `defineProps<{ [k: string]: string }>()` (which has NO named
/// property member) still contributes its index signature to the published
/// props surface. Each signature's `key_type` / `value_type` graph node is
/// raised to a `TypeExpr` through the ACTIVE `ctx` (overlay-aware); a node that
/// does not raise is skipped (no phantom signature).
fn prop_index_signatures_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<verter_semantic::analysis::type_expand::ExpandedIndexSignature> {
    let dispatch = ctx.dispatch();
    macro_surface
        .surface
        .index_signatures
        .iter()
        .filter_map(|sig| {
            let key_type = dispatch.raise_node_to_type_expr(sig.key_type)?;
            let value_type = dispatch.raise_node_to_type_expr(sig.value_type)?;
            Some(verter_semantic::analysis::type_expand::ExpandedIndexSignature {
                key_type,
                value_type,
                readonly: sig.readonly,
            })
        })
        .collect()
}

/// Build the `defineModel` synthesized prop field from the analyzer facts.
/// `defineModel<T>('name', { … })` synthesizes a prop named `name`
/// (default `modelValue`) typed `T`; the analyzer already captured this as the
/// macro's single `prop_fields` entry. Re-scope the typed form to the SFC owner
/// so nested `Ref`s resolve in the SFC.
///
/// The owner SFC's `IndexedReady` is fetched through the ACTIVE `ctx`
/// (`ctx.ensure_indexed_ready`), NOT the base `VerterHost`, so an overlay
/// session reads the OVERLAY `defineModel` macro facts — a `defineModel<number>`
/// edit no longer rereads the base host's `defineModel<string>` snapshot.
fn model_prop_fields(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    let Some(indexed) = ctx.ensure_indexed_ready(macro_surface.owner_canonical.as_ref()) else {
        return Vec::new();
    };
    let Some(mac) = indexed.snapshot.macros.get(macro_surface.macro_index) else {
        return Vec::new();
    };
    mac.prop_fields
        .iter()
        .map(|field| {
            // The analyzer stamps an empty scope on the synthesized model prop;
            // re-anchor it to the SFC owner so the pairing invariant holds with
            // a real scope.
            let type_expr_scope = field
                .type_expr
                .as_ref()
                .map(|_| TypeExprScope::new(macro_surface.owner_canonical.as_ref()));
            AnalyzedPropField {
                type_expr_scope,
                ..field.clone()
            }
        })
        .collect()
}

/// Normalize a `.vue` emits macro surface into the published
/// [`AnalyzedEmitField`] set.
///
/// Reproduces `ImportedMacroSurface::emit_members` over the typeinfo surface:
///
/// 1. **Call-signature emits FIRST.** Each call signature's first parameter is
///    the event name (a `String` literal, or a `Union` of `String` literals);
///    the typed `payload_expr` is the call-signature function with the leading
///    event-name parameter STRIPPED (`(e: 'change', v: number) => void` → event
///    `change`, payload `(v: number) => void`). The event name is NEVER read
///    from `keyof` (which would surface numeric tuple indices). The display
///    `payload_type` (→ `rawType`, no consumer parses it) is a CONSISTENT
///    source-span slice of the call signature as written (local + cross-file
///    alike); `None` for a synthetic signature with no span.
/// 2. **Property-style emits as a FALLBACK** — only when no call-signature emit
///    was found. Each named member is an event; its value type is the payload.
/// 3. **De-duplicate by event name, first-writer-wins** (matching the eager
///    projector's `retain`).
#[must_use]
pub(crate) fn emits_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedEmitField> {
    // View-sensitive type resolution flows through the active `ctx`
    // (`ctx.dispatch()`), NOT a fresh base `HostResolverContext`, so an overlay
    // session raises emit call-signatures / property payloads against overlay
    // content. Host-level reads (JSDoc source slicing, node scope) use the host
    // the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();

    let mut emits: Vec<AnalyzedEmitField> = Vec::new();

    // (1) Call-signature emits.
    for sig in macro_surface.surface.call_signatures.iter() {
        let Some(TypeExpr::Function(func)) = dispatch.raise_node_to_type_expr(sig.node) else {
            continue;
        };
        let Some(first) = func.parameters.first() else {
            continue;
        };
        // Payload = the call signature's REMAINING parameters (after the leading
        // event-name parameter) as a TUPLE -- the Vue emit payload shape (the
        // args passed to `emit('name', ...)`). `(e: 'change', payload: T) =>
        // void` yields event `change` with payload tuple `[payload: T]`. This
        // matches the eager OXC rail's `AnalyzedEmitField.payload_expr` (a
        // `TypeExpr::Tuple`, NOT the whole call-signature function) so the
        // downstream projector publishes an identical `event.payload`. Each
        // surviving parameter maps to a labelled tuple element preserving its
        // name / optional / rest.
        let payload_tuple = TypeExpr::Tuple {
            elements: func
                .parameters
                .iter()
                .skip(1)
                .map(|param| verter_type_expr::TupleElement {
                    label: param.name.clone(),
                    ty: param.ty.clone(),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            readonly: false,
        };
        // Scope the payload to the call signature's DECLARATION-origin file
        // (derived from its spans) so an inherited cross-file emit signature's
        // payload `Ref`s resolve in the base file, matching the eager rail's
        // per-signature `member_expr_scope`. Falls back to the SFC owner for a
        // signature written in the SFC's own defineEmits type argument.
        let payload_scope = macro_surface.signature_expr_scope(sig);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it
        // (the typed `payload_expr` carries the semantics). It mirrors the
        // payload TUPLE the typed `payload_expr` holds (the `emit('name', ...)`
        // args AFTER the leading event-name parameter), rendered as
        // `[label: T, ...]` — matching the typed `payload_expr` form a consumer
        // would otherwise reconstruct. `render_type_expr_display` renders the
        // tuple (incl. labelled / optional / rest elements); `None` when an
        // element type cannot be surfaced as a single inline display fragment.
        let payload_type = render_type_expr_display(&payload_tuple);
        // The event's JSDoc rides on the call signature itself (the leading
        // `/** */` block documenting `(e: 'change', …): void`), sliced from the
        // signature's typeinfo JSDoc spans — symmetric with the property-style
        // fallback's `member_jsdoc_from_spans`. A union of event-name literals on
        // ONE signature shares that signature's JSDoc across each event it names.
        let (description, tags) = signature_jsdoc_from_spans(host, sig);
        let mut push_event = |name: String| {
            emits.push(AnalyzedEmitField {
                name,
                span: verter_span::Span::default(),
                payload_type: payload_type.clone(),
                payload_expr: Some(payload_tuple.clone()),
                payload_expr_scope: Some(payload_scope.clone()),
                description: description.clone(),
                tags: tags.clone(),
            });
        };
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => push_event(name.clone()),
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    if let TypeExpr::Literal(LiteralValue::String(name)) = ty {
                        push_event(name.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // (2) Property-style emits — fallback only when no call-signature emit fired.
    if emits.is_empty() {
        for member in macro_surface.surface.members.iter() {
            let payload_expr = raise_member_value(ctx, member);
            let payload_expr_scope = payload_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let payload_type = payload_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            emits.push(AnalyzedEmitField {
                name: member.name.as_ref().to_string(),
                span: verter_span::Span::default(),
                payload_type,
                payload_expr,
                payload_expr_scope,
                description,
                tags,
            });
        }
    }

    // (3) De-duplicate by event name, first-writer-wins.
    let mut seen = std::collections::HashSet::new();
    emits.retain(|emit| seen.insert(emit.name.clone()));
    emits
}

/// Extract a slot callable's first-parameter type + return type from a slot
/// member value, handling an INTERSECTION or a UNION of function types.
///
/// A slot typed via an intersection of interfaces
/// (`defineSlots<SlotA & SlotB>()`) has its `default` member resolve to
/// `SlotA['default'] & SlotB['default']` — an `Intersection` of two function
/// types (the TS-correct meaning of indexing an intersection), NOT a single
/// pre-merged `Function`. A slot typed as a union of function aliases
/// (`defineSlots<{ default: SlotA | SlotB }>()`) resolves its `default` member
/// to a `Union` of two function types. Both are slot-callable. Returns:
///
/// - `Function(f)` → `f`'s first-param type + return type directly.
/// - `Intersection(arms)` / `Union(arms)` where EVERY resolvable arm is a
///   function → the INTERSECTION of the arms' first-param types (so
///   `{ value?: string } & { value: string }` flows into
///   [`binding_fields_from_param_ty`], whose resolver-navigation merges it
///   required-wins; for a UNION the param is contravariant, so the bindings a
///   template can SAFELY destructure are those present across all arms — again
///   the intersection merge) plus the combined return type (intersection of
///   returns for an intersection, union of returns for a union). A non-function
///   arm makes the member not slot-like.
/// - Anything else → `None` (the member is not a slot).
fn slot_callable_param_and_return(
    value: &TypeExpr,
) -> Option<(
    Option<TypeExpr>,
    Option<TypeExpr>,
    Option<verter_span::Span>,
)> {
    match value {
        TypeExpr::Function(func) => Some((
            func.parameters.first().map(|p| p.ty.clone()),
            func.return_type.as_ref().map(|rt| (**rt).clone()),
            // The return-type annotation span (file-relative to the slot
            // member's value-node file). Lets the caller slice the EXACT source
            // text for the display `return_type` when the typed return contains
            // an unresolved reference (`VNode` not imported) that
            // `render_type_expr_display` cannot surface.
            func.spans.return_type,
        )),
        // Intersection of slot-callable arms: param = intersection of first
        // params (required-wins merge), return = intersection of returns.
        TypeExpr::Intersection(arms) => {
            slot_callable_param_and_return_from_arms(arms, ArmCombine::Intersection)
        }
        // Union of slot-callable arms (`SlotA | SlotB`): param stays the
        // INTERSECTION of first params (a slot prop the template can rely on
        // must be present in every arm — contravariant param), but the return
        // is the UNION of the arms' return types (covariant). Without this arm
        // a union-of-functions slot was silently dropped.
        TypeExpr::Union(arms) => {
            slot_callable_param_and_return_from_arms(arms, ArmCombine::Union)
        }
        _ => None,
    }
}

/// How to combine the RETURN types of a multi-arm slot callable. The first
/// params are ALWAYS intersected (the bindings a template can rely on must hold
/// across every arm); only the return-type combiner differs.
#[derive(Clone, Copy)]
enum ArmCombine {
    Intersection,
    Union,
}

/// Shared multi-arm slot-callable extractor for `Intersection` / `Union` of
/// function types. Every arm MUST be a `Function` (a non-function arm makes the
/// member not slot-like → `None`). The first params are intersected; the
/// returns are combined per `combine`.
///
/// SOUNDNESS — a slot binding is guaranteed only if EVERY arm supplies a first
/// parameter. A template destructuring `<template #default="{ x }">` runs for
/// WHICHEVER arm the slot actually is, so a binding the template can rely on must
/// be present across all arms. If ANY arm is a no-param callable (`() => any`),
/// the multi-arm callable can be invoked with no slot props in that branch, so
/// there are NO guaranteed bindings — the first param is dropped to `None`
/// (otherwise a union like `(() => any) | ((props: { a }) => any)` would
/// wrongly publish `a`). The return type still combines across arms.
fn slot_callable_param_and_return_from_arms(
    arms: &[TypeExpr],
    combine: ArmCombine,
) -> Option<(
    Option<TypeExpr>,
    Option<TypeExpr>,
    Option<verter_span::Span>,
)> {
    let mut first_params: Vec<TypeExpr> = Vec::new();
    let mut returns: Vec<TypeExpr> = Vec::new();
    // A binding is guaranteed only when EVERY arm contributes a first param.
    // A single no-param arm makes the slot callable with no props in that
    // branch, so no binding is sound.
    let mut all_arms_have_first_param = true;
    for arm in arms.iter() {
        let TypeExpr::Function(func) = arm else {
            // A non-function arm means the member is not purely slot-callable;
            // fall out (not a slot).
            return None;
        };
        if let Some(p) = func.parameters.first() {
            first_params.push(p.ty.clone());
        } else {
            all_arms_have_first_param = false;
        }
        if let Some(rt) = func.return_type.as_ref() {
            returns.push((**rt).clone());
        }
    }
    if first_params.is_empty() && returns.is_empty() {
        return None;
    }
    // First params: the INTERSECTION (the slot prop object a template can
    // destructure must be guaranteed across every arm) — but ONLY when every
    // arm actually supplied a first param. A no-param arm guarantees nothing, so
    // the bindings are dropped entirely (the return type is still combined).
    let first_param = if all_arms_have_first_param {
        match first_params.len() {
            0 => None,
            1 => Some(first_params.into_iter().next().unwrap()),
            _ => Some(TypeExpr::Intersection(std::sync::Arc::from(
                first_params.into_boxed_slice(),
            ))),
        }
    } else {
        None
    };
    // Returns: combine per the arm kind (intersection of returns for an
    // intersection of functions; union of returns for a union of functions).
    let return_ty = match returns.len() {
        0 => None,
        1 => Some(returns.into_iter().next().unwrap()),
        _ => {
            let boxed = std::sync::Arc::from(returns.into_boxed_slice());
            Some(match combine {
                ArmCombine::Intersection => TypeExpr::Intersection(boxed),
                ArmCombine::Union => TypeExpr::Union(boxed),
            })
        }
    };
    // A composed multi-arm callable has no single return-type span; the caller
    // renders the composed return from the typed form.
    Some((first_param, return_ty, None))
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Reproduces `ImportedMacroSurface::slot_members` over the typeinfo surface:
/// keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from
/// resolving the function's first-parameter type to its object surface (a
/// literal object, a `Pick<…>`, or a named alias — see
/// [`binding_fields_from_param_ty`]); the `return_expr` / `return_type` come
/// from the function's return type. Bindings + return are scoped to the slot
/// member's VALUE-NODE file (see [`VueMacroSurface::member_expr_scope`]).
#[must_use]
pub(crate) fn slots_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedSlotField> {
    // View-sensitive slot type resolution (callable realization, first-param
    // object navigation) flows through the active `ctx`, NOT a fresh base
    // `HostResolverContext`, so an overlay session resolves slot bindings
    // against overlay content. Host-level reads (JSDoc / return-type source
    // slicing, node scope) use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        .filter_map(|member| {
            // A slot member's value may be a non-`Function` carrier shell under
            // the transit-shallow macro surface — most notably a generic slot
            // alias (`default: SlotFn<T>` where `type SlotFn<T> = (props:
            // SlotProps<T>) => any`) that lowers to an `InstantiationRef` / alias
            // carrier rather than a reduced `Function`. Realize the value through
            // the SHARED callable-realization substrate (the SAME primitive
            // `slot_binding_graph::compute_bindings_via_graph` uses) so a
            // decidable callable surfaces as a `Function` BEFORE the
            // function-like filter — otherwise the generic slot is silently
            // dropped from the published slot surface (a one-engine divergence
            // between the DTO slot path and the slot-binding-graph).
            let value = raise_realized_callable_member_value(ctx, member)?;
            // A slot member is function-like: a single `Function`, or an
            // `Intersection` of functions (`(SlotA & SlotB)['default']`), or a
            // `Union` of functions (`SlotA | SlotB`). A non-callable member is
            // not a slot.
            let (first_param, return_expr, return_span) = slot_callable_param_and_return(&value)?;
            let scope = macro_surface.member_expr_scope(host, member);
            let bindings = first_param
                .as_ref()
                .map(|param_ty| binding_fields_from_param_ty(ctx, param_ty, &scope))
                .unwrap_or_default();
            let return_expr_scope = return_expr.as_ref().map(|_| scope.clone());
            // Display `return_type`: prefer the EXACT source text sliced from the
            // return-type annotation span in the slot member's value-node file —
            // this preserves a name the typed return cannot surface (an
            // unresolved imported `VNode` raises to `Unknown { raw:
            // "semanticMiss" }`, which `render_type_expr_display` renders as
            // `None`, yet the source says `VNode[]`). Fall back to rendering the
            // typed return when there is no span (synthetic / intersection
            // return). Display-only; `return_expr` stays the semantic authority.
            let return_type = return_span
                .map(|span| CanonicalSpan::new(scope.as_str().into(), span))
                .and_then(|cspan| slice_canonical_span(host, &cspan))
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .or_else(|| return_expr.as_ref().and_then(render_type_expr_display));
            let (description, tags) = member_jsdoc_from_spans(host, member);
            Some(AnalyzedSlotField {
                name: member.name.as_ref().to_string(),
                is_required: !member.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type,
                return_expr,
                return_expr_scope,
                description,
                tags,
            })
        })
        .collect()
}

/// Reconstruct a slot's binding fields from its function's first-parameter
/// type. Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's value `TypeExpr` as
/// `binding_expr`.
///
/// The first parameter is the slot-props object. It can be written several
/// ways, all of which the LOCAL eager rail
/// (`surface_projector::bindings_from_first_param_ty`) accepts: a literal
/// object (`(props: { item: string })`), a `Pick<T, 'k'>` over a named type
/// (`(props: Pick<RowApi, 'name'>)`), or a parenthesized form. To handle all of
/// them WITHOUT a nominal shape-sniff (no `name == "Pick"` matching, no text
/// splitting), the binding object is obtained by RESOLVING the first-parameter
/// type through the SHARED resolver:
///
/// - A literal [`TypeExpr::Object`] is read directly (no resolution needed — it
///   is already the binding object). This is a STRUCTURAL match on the typed
///   IR, not a nominal sniff.
/// - Any other shape (`Pick<…>` / `Omit<…>` / a `Ref` to a named alias /
///   `Parenthesized`) is lowered in the slot member's value-node scope and
///   projected to its one-level object surface
///   ([`VerterHost::navigate_param_to_object_surface`]); each surface member
///   becomes a binding. The shared resolver navigates `Pick` / unwraps
///   `Parenthesized` / resolves the alias — there is no per-utility special
///   case here.
///
/// A first parameter that does not resolve to an object surface yields no
/// bindings — matching the eager rail's non-object outcome.
fn binding_fields_from_param_ty(
    ctx: &dyn crate::resolver_core::ResolverContext,
    param_ty: &TypeExpr,
    scope: &TypeExprScope,
) -> Vec<AnalyzedSlotFieldBinding> {
    // View-sensitive navigation / raising flows through `ctx`; host-level node
    // scope reads use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    // Literal object: read its properties directly (structural typed-IR match).
    if let TypeExpr::Object(obj) = param_ty {
        return obj
            .properties
            .iter()
            .filter_map(|member| match member {
                verter_type_expr::ObjectMember::Property(prop) => Some(AnalyzedSlotFieldBinding {
                    name: prop.name.clone(),
                    type_annotation: render_type_expr_display(&prop.ty),
                    binding_expr: Some(prop.ty.clone()),
                    binding_expr_scope: Some(scope.clone()),
                    span: verter_span::Span::default(),
                }),
                _ => None,
            })
            .collect();
    }

    // Non-object first param (`Pick<…>` / alias `Ref` / `Parenthesized`):
    // navigate it through the shared resolver to its object surface and read
    // the resolved members. Each member's `binding_expr` is its raised value
    // type, scoped to that value's own file (so a `Pick<RowApi,'k'>` whose
    // picked member is a cross-file `Ref` resolves in the right file).
    let Some(surface) = navigate_param_to_object_surface(ctx, scope.as_str(), param_ty) else {
        return Vec::new();
    };
    // Shallow-by-default Pick member publication: when the slot param is a
    // `Pick<NamedRoot, K>` the picked members stay SYMBOLIC at the published
    // binding surface — each binding's value is the typed indexed access
    // `NamedRoot['member']` (built from the typed param, not reparsed) so the
    // raw type renders as e.g. `CalendarCellTriggerProps['day']`. A consumer
    // that wants the concrete picked member re-resolves that member path on
    // demand. Resolving the picked member eagerly here would both violate the
    // shallow contract and (for a cross-file picked value) collapse to
    // `Unknown(semanticMiss)` / `None`. Other shapes (plain alias `Ref`,
    // `Parenthesized`, direct `IndexedAccess`) keep the navigated value.
    let pick_symbolic_root = pick_named_source_root(param_ty);
    surface
        .members
        .iter()
        .map(|member| {
            if let Some(root) = pick_symbolic_root {
                let symbolic = TypeExpr::IndexedAccess {
                    object: Arc::new(root.clone()),
                    index: Arc::new(TypeExpr::Literal(LiteralValue::String(
                        member.name.as_ref().to_string(),
                    ))),
                };
                return AnalyzedSlotFieldBinding {
                    name: member.name.as_ref().to_string(),
                    type_annotation: render_type_expr_display(&symbolic),
                    binding_expr: Some(symbolic),
                    binding_expr_scope: Some(scope.clone()),
                    span: verter_span::Span::default(),
                };
            }
            let binding_expr = raise_member_value(ctx, member);
            let binding_expr_scope = binding_expr
                .as_ref()
                .map(|_| macro_member_value_scope(host, member, scope));
            let type_annotation = binding_expr.as_ref().and_then(render_type_expr_display);
            AnalyzedSlotFieldBinding {
                name: member.name.as_ref().to_string(),
                type_annotation,
                binding_expr,
                binding_expr_scope,
                span: verter_span::Span::default(),
            }
        })
        .collect()
}

/// When `param_ty` is structurally `Pick<NamedRoot, K>` (modulo
/// `Parenthesized` wrappers) with `NamedRoot` a nominal [`TypeExpr::Ref`],
/// return that source-root `Ref` so a slot binding can publish each picked
/// member as the symbolic `NamedRoot['member']` indexed access. This is a
/// STRUCTURAL match on the typed IR — no type-text sniffing, no reparse. Any
/// other shape (a non-`Ref` Pick source, `Omit`, a plain alias `Ref`, a direct
/// `IndexedAccess`) returns `None` and the navigated member value is used.
fn pick_named_source_root(param_ty: &TypeExpr) -> Option<&TypeExpr> {
    match param_ty {
        TypeExpr::Parenthesized(inner) => pick_named_source_root(inner),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == "Pick" && type_arguments.len() == 2 => {
            let mut source = &type_arguments[0];
            while let TypeExpr::Parenthesized(inner) = source {
                source = inner;
            }
            matches!(source, TypeExpr::Ref { .. }).then_some(source)
        }
        _ => None,
    }
}

/// The [`TypeExprScope`] a navigated binding member's `binding_expr` binds to —
/// its value-node scope (matching [`VueMacroSurface::member_expr_scope`]),
/// falling back to the slot's scope when the member's value node is
/// structural / scope-less.
fn macro_member_value_scope(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
    fallback: &TypeExprScope,
) -> TypeExprScope {
    host.project_type_store()
        .semantic_graph()
        .node_scope(member.value)
        .and_then(|scope| scope.canonical_file())
        .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        .or_else(|| {
            member
                .origin
                .canonical_file
                .as_ref()
                .map(|canonical| TypeExprScope::new(canonical.as_ref()))
        })
        .unwrap_or_else(|| fallback.clone())
}
