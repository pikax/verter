#![deny(missing_docs)]
//! The relocated Vue resolution delegates — public component type + the
//! FullMetadata macro surface and its prop / emit / slot normalizers.
//!
//! This module is the executor-side home of the Vue resolution machinery: the
//! `impl VerterHost` entry points stay the public API current consumers call,
//! and the framework-surface executor's private resolve ops converge on the
//! same delegate functions — ONE semantic path with two entries into it. The
//! Vue plan/normalize adapter ([`crate::typeinfo::adapters::vue::adapter`])
//! holds NO resolution; it emits typed [`crate::typeinfo::framework_surface::PlannedDemand`]
//! data and consumes the executor-resolved results.
//!
//! ## Public component type
//!
//! A TS consumer that writes `import Foo from './Foo.vue'` sees the SFC's
//! synthesized public component type: the instance surface carrying
//! `$props` / `$emit` / `$slots`. That surface is synthesized from the SFC's
//! macro type-arguments by [`crate::resolver_core::vue_default_synth`], which
//! injects a `default` value symbol whose construct-signature return type IS
//! the instance object. [`VerterHost::resolve_vue_public_type`] projects that
//! synthesized instance object into the span-rich [`TypeInfoSurface`] through
//! the shared typeinfo surface path — `Instantiate{ .vue, "default", [] }` is
//! the SOLE semantic identity for a `.vue`'s public instance.
//!
//! ## Macro surface + normalizers
//!
//! [`VerterHost::resolve_vue_macro_surface`] resolves ONE `.vue` macro's
//! type-argument surface (`defineProps<T>()` / `defineEmits<E>()` /
//! `defineSlots<S>()` / `withDefaults(defineProps<T>(), …)`) to the span-rich
//! [`VueMacroSurface`] at [`TypeInfoQueryLevel::FullMetadata`] through the
//! SHARED typeinfo surface path. The three normalizers
//! ([`props_from_typeinfo_surface`] / [`emits_from_typeinfo_surface`] /
//! [`slots_from_typeinfo_surface`]) consume a [`TypeInfoSurface`] plus the
//! macro-analyzer facts and produce the FINAL component-meta DTOs
//! (`AnalyzedPropField` / `AnalyzedEmitField` / `AnalyzedSlotField`), sourcing
//! every semantic decision from the typeinfo surface:
//!
//! - **props** — one field per named member, carrying the surface's `optional`
//!   / `readonly` / `declared_in_macro_type_arg`, the member value raised to a
//!   `TypeExpr` (scoped to the member's VALUE-NODE file — see
//!   [`VueMacroSurface::member_expr_scope`]), the `defineModel` synthesized
//!   model prop from analyzer facts, and JSDoc sliced from the surface's JSDoc
//!   SPANS.
//! - **emits** — call-signature event extraction FIRST (the first parameter's
//!   string-literal — or union of string literals — is the event name; the
//!   payload is the call-signature function with the leading event-name
//!   parameter STRIPPED), property-key members only as a fallback, de-duplicated
//!   by event name (first-writer-wins).
//! - **slots** — function-like members only; the first-parameter object's
//!   properties become the slot bindings; the function return type becomes the
//!   slot return.
//!
//! ## DTO cache
//!
//! [`vue_macro_dtos_with_ctx`] materializes a `.vue` macro's normalized DTOs
//! ONCE per `(canonical, content, macro, level)` and publishes the immutable
//! owned [`MacroSurfaceDtos`] bundle into the host-owned
//! [`crate::framework::surface_store::FrameworkSurfaceStore`] (reached through
//! [`VerterHost::vue_surface_store`]), honoring the Shallow File Processing
//! Core Invariant. The cached value is the fully-owned, immutable normalizer
//! output (owned `TypeExpr` + scope + `String`) — generation-independent and
//! stable across graph-generation flips. It deliberately does NOT cache the
//! transient [`VueMacroSurface`], whose `SemanticNodeId`s are graph-generation
//! scoped. The cache is content-addressed (the key carries the `.vue`'s
//! `whole_hash`) and fact-validated (warm reads revalidate the recorded
//! cross-file carrier fact signature + project generation against the live
//! view).

use std::sync::Arc;

use verter_semantic::analysis::type_expand::ExpandedIndexSignature;
use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_type_expr::{LiteralValue, TypeExpr, TypeExprScope};

use crate::framework::surface_store::{FullKey, StoredSurfaceDto};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::surface_projector::render_type_expr_display;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput, SurfaceProvenanceContext,
};
use crate::typeinfo::framework_surface::results::{EmitsSurface, MacroSurfaceDtos, PropsSurface};
use crate::typeinfo::framework_surface::VueSurfaceKey;
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
    /// VALUE-NODE scope (`node_scope(member.value)` → file).
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
    /// files.
    ///
    /// Falls back to the member's declaration_origin, then the SFC owner, when
    /// the value node carries no single-file scope (a structural / scope-less
    /// value node — a primitive, a shared literal-union).
    fn member_expr_scope(
        &self,
        host: &VerterHost,
        member: &TypeInfoSurfaceMember,
    ) -> TypeExprScope {
        crate::typeinfo::framework_surface::scope::member_value_expr_scope(
            host,
            member,
            self.owner_canonical.as_ref(),
        )
    }

    /// The scope a call signature's stripped-payload `*_expr` should bind to —
    /// the signature's DECLARATION-origin file, derived from its spans (each
    /// [`CanonicalSpan`] carries the file the offsets index into). For a
    /// cross-file emit interface's call signature the spans live in the heritage
    /// base's file, so the payload `Ref`s resolve THERE — the file the call
    /// signature is DECLARED in. This is the correct scope even when the SFC
    /// instantiates a generic emit interface
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
    /// Resolve a `.vue` SFC's PUBLIC component type to its span-rich one-level
    /// [`TypeInfoSurface`] — the synthesized `{ $props, $emit, $slots }`
    /// instance surface — through typeinfo, WITHOUT calling component-meta.
    ///
    /// Returns `None` when `canonical_id` is not a loaded `.vue` carrying a
    /// synthesized `default` instance object (a plain `.ts` file, or a `.vue`
    /// with no type-based macros — there is no public component surface to
    /// build).
    ///
    /// `level` is accepted for symmetry with the level-aware request surface;
    /// the public component type IS the [`TypeInfoQueryLevel::PublicType`]
    /// projection, so callers pass `PublicType`.
    #[must_use]
    pub fn resolve_vue_public_type(
        &self,
        canonical_id: &str,
        level: TypeInfoQueryLevel,
    ) -> Option<TypeInfoSurface> {
        debug_assert_eq!(
            level,
            TypeInfoQueryLevel::PublicType,
            "resolve_vue_public_type serves the PublicType level"
        );
        let _ = level;

        // The `.vue`'s synthesized public instance is the first-class semantic
        // query `Instantiate { base: ResolvedDeclSlotIdentity(canonical, "default",
        // Type, env…), args: [], context: InstantiateContext { … } }` — the
        // content-free slot carries env dims only; the live `whole_hash` is
        // re-sourced at value-compute, not carried by the key. This is the SAME
        // keyed identity a `.vue`-importing-`.vue` reference resolves through
        // (`Ref("Foo")` → `DeclRef{Foo.vue, "default"}` → `Instantiate`), so the
        // public API and import recursion share ONE semantic identity.
        //
        // Materialize the `.vue`'s `IndexedReady` first (idempotent) to observe
        // the live `whole_hash`. Gate on the SYNTHESIZED `default` instance
        // symbol's STRUCTURAL PROVENANCE flag BEFORE dispatching so a plain
        // `.ts` file (no synthesized `default`), a `.vue` with no type-based
        // macros, or a `.vue` carrying a USERLAND `export default` (synthesis
        // skipped) returns `None` here.
        let indexed = self.ensure_indexed_ready(canonical_id)?;
        let default_symbol = indexed.shallow_state.value_symbol("default")?;
        if !default_symbol.is_synthesised_component_default {
            return None;
        }
        // The synthesized default carries a construct-signature return type (the
        // instance object); its absence means no public instance surface.
        default_symbol.signatures.first()?.return_type.as_ref()?;
        let _whole_hash = indexed.whole_hash;

        // Query-RETURNER: it returns the public instance surface with no outer
        // publish fence, so it MUST resolve against a PROVEN-CURRENT snapshot.
        // On sustained churn surface a miss (`None`) rather than a surface
        // resolved against superseded state. The bounded retry terminates.
        let current_view = crate::typeinfo::current_store_view_for_query(self)?;
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);

        // Intermediate-hop demand: the keyed query lowers the instance object in
        // `structural_transit(Navigate)` so member values stay shallow. The
        // empty-path `Shallow` terminal below synthesises the one-level surface
        // under publication demand.
        let base = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base: dispatch.type_slot_for(Arc::from(canonical_id), Arc::from("default")),
            args: Arc::from(Vec::new().into_boxed_slice()),
            context: dispatch.instantiate_context_for(
                canonical_id,
                ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
            ),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(node) => node,
            QueryResult::Error(_) => return None,
        };

        // The public component type is a plain structural object
        // (`{ $props, $emit, $slots }`) — no macro own-body provenance applies
        // to the synthesized instance members, so the structural
        // `published(Shallow)` context is correct.
        self.project_shallow_surface_from_base(
            &host_ctx,
            &dispatch,
            base,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
    }

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
    /// macro that has NO type argument. The outer macro is therefore
    /// `is_type_based == false` and falls out at the `is_type_based` guard
    /// below. The props surface comes from the SEPARATELY-routed inner
    /// `DefineProps` macro. The defaults the `withDefaults` call supplies flip
    /// `required` / `has_default` DOWNSTREAM at the component-meta PropAnalysis
    /// layer, not on this surface.
    ///
    /// **Demand + provenance:** macro-object DTO synthesis lowers the type
    /// argument under the Vue
    /// [`ProjectionReductionContext::macro_object_surface`] demand, NOT ordinary
    /// `Published`. For a `Union`-rooted type argument the macro surface is the
    /// UNION of object-arm members. The props macro (`DefineProps`) keeps the
    /// [`SurfaceProvenanceContext::MacroTypeArgOwnBody`] provenance so the
    /// type-argument's OWN-body members surface with
    /// `declared_in_macro_type_arg = true` and heritage-reached members stay
    /// `false`; structural macros (`DefineEmits` / `DefineSlots`) lower under
    /// [`SurfaceProvenanceContext::Structural`].
    #[must_use]
    pub fn resolve_vue_macro_surface(
        &self,
        request: &VueMacroSurfaceRequest,
    ) -> Option<VueMacroSurface> {
        // Base-view query-RETURNER (tests, the host-method `vue_macro_dtos`
        // wrapper). It returns the macro surface with no outer publish fence, so
        // it MUST resolve against a PROVEN-CURRENT snapshot. Overlay-bearing
        // production callers MUST use `resolve_vue_macro_surface_with_ctx` with
        // their active session context. The bounded retry terminates.
        let current_view = crate::typeinfo::current_store_view_for_query(self)?;
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        self.resolve_vue_macro_surface_with_ctx(&host_ctx, request)
    }

    /// Context-bound resolution core for [`Self::resolve_vue_macro_surface`].
    ///
    /// Every view-sensitive read — the owner SFC's `IndexedReady`, the type
    /// argument lowering, the cross-file carrier projection, the carrier-file
    /// JSDoc source — flows through `ctx`, so an overlay session resolves the
    /// macro surface against its overlay content rather than the base host view.
    /// The dispatcher is `ctx.dispatch()`, keeping the surface inside the single
    /// resolution engine.
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
        // members are flagged; emits / slots are structural. The terminal
        // `MacroTypeArgOwnBody` synthesis restamps `declared_in_macro_type_arg =
        // true` for EXACTLY the declaration's own-body direct members.
        // Heritage-reached members are left at the structural `false`. Only
        // `DefineProps` reaches here as a props macro: `WithDefaults` is never
        // `is_type_based` and `DefineModel` returned its empty surface above.
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
        // (`defineProps<DeepConfig['ui']['header']>()`). The base is lowered as
        // the carrier and the string-literal hops become the `ProjectPath`
        // selector. The shared path walker runs intermediate hops in `Navigate`
        // and the TERMINAL hop under `terminal_context` (Shallow). A non-indexed
        // type argument decomposes to `(type_arg, [])`.
        let (base_expr, path) =
            crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain(type_arg);

        // Lower the carrier base in the SFC scope under structural-transit
        // Navigate (member values stay shallow); the path-precise `Shallow`
        // projection then synthesises the one-level surface of the terminal hop.
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
    /// ([`MacroSurfaceDtos`]), consulting the host-owned framework-surface store
    /// first.
    ///
    /// This is the cached FullMetadata entry point: it materializes the macro
    /// surface ONCE per `(canonical, content, macro, level)`, publishes the
    /// immutable owned DTO bundle into the store, and serves subsequent calls
    /// from the content-addressed cache. Returns an empty (default) bundle when
    /// the macro surface cannot be resolved — the same "no surface" outcome the
    /// eager rail produced for an unresolvable macro.
    #[must_use]
    pub fn vue_macro_dtos(&self, request: &VueMacroSurfaceRequest) -> Arc<MacroSurfaceDtos> {
        // Base-view query-RETURNER. It returns AND content-addressed caches the
        // DTO bundle, so it MUST resolve against a PROVEN-CURRENT snapshot — a
        // non-current execution must never warm the cache. On sustained churn
        // return the empty "no surface" bundle WITHOUT computing or caching
        // anything. Overlay-bearing production callers MUST call
        // `vue_macro_dtos_with_ctx` with their active session context.
        let Some(current_view) = crate::typeinfo::current_store_view_for_query(self) else {
            return Arc::new(MacroSurfaceDtos::default());
        };
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        vue_macro_dtos_with_ctx(&host_ctx, request)
    }
}

/// Navigate a `TypeExpr` to its one-level object [`TypeInfoSurface`] through the
/// SHARED resolver bound to the ACTIVE `ctx`, lowering it in `scope_canonical`
/// then projecting the empty-path `Shallow` surface.
///
/// Used by the slot-binding extractor to resolve a slot's first-parameter type
/// (`Pick<RowApi, 'name'>` / a named alias / a parenthesized form) to the
/// binding object WITHOUT a nominal shape-sniff: `Pick` is navigated,
/// `Parenthesized` is unwrapped, and an alias `Ref` is resolved by the one
/// shared resolver. Returns `None` when the scope file is not loaded or the type
/// does not project to an object surface.
///
/// Bound to `ctx` (`ctx.dispatch()`), so an overlay session resolves the
/// slot-param object against its OVERLAY content.
#[must_use]
pub(crate) fn navigate_param_to_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical: &str,
    param_ty: &TypeExpr,
) -> Option<TypeInfoSurface> {
    let dispatch = ctx.dispatch();

    // Lower the parameter type in its scope under structural-transit Navigate
    // (member values stay shallow); the empty-path Shallow projection then
    // synthesises the one-level object surface.
    let base = dispatch.lower_type_expr_in_scope_with_context(
        scope_canonical,
        param_ty,
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
    )?;
    // Open-generic gate: a slot-param root that is symbolic-only (an open
    // Conditional whose check carries a free `TypeParam`, an unresolved
    // `IndexedAccess` / `Mapped`, …) must NOT be materialised into a committed
    // object surface. This is the SAME gate the graph-native slot-binding
    // synthesis applies; routing both binding paths through it keeps them in
    // agreement.
    if crate::meta_resolve::slot_binding_graph::slot_param_root_is_symbolic_only(&dispatch, base, 0)
    {
        return None;
    }
    ctx.host_for_fact_tracer_install()
        .project_shallow_surface_from_base(
            ctx,
            &dispatch,
            base,
            Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        )
}

/// Slice a [`CanonicalSpan`]'s byte range out of its file's cache-owned RAW
/// source (`IndexedReady.raw_source`). [`CanonicalSpan`] offsets are
/// SFC-absolute (the eval source is position-preserving, so OXC stamps spans in
/// raw-file coordinates), so the slice indexes the raw source directly. `None`
/// when the file is not loaded or the byte range is out of bounds (a stale /
/// synthetic span). This is the single source-slicing primitive the normalizers
/// use to materialize display text from a span at the consumer boundary — it
/// does NOT re-resolve or re-parse.
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
/// decoration verbatim. The published `description` is DISPLAY text, not comment
/// syntax, so strip each continuation line's leading whitespace + optional
/// single `*` decoration — matching `verter_semantic::analysis::jsdoc`'s
/// per-line stripping — and rejoin with `\n`. A single-line body is returned
/// trimmed.
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
/// when the node has no raisable shape.
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
/// callable. This keeps the DTO slot surface in agreement with
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
/// ([`MacroSurfaceDtos`]) through the active [`crate::resolver_core::ResolverContext`].
///
/// This is the SINGLE DTO resolution core: it materializes the macro surface
/// ONCE per `(canonical, content, macro, level)` (resolving the surface through
/// `ctx` and running the appropriate normalizer), publishes the immutable owned
/// DTO bundle into the host-owned
/// [`crate::framework::surface_store::FrameworkSurfaceStore`], and serves
/// subsequent calls from the content-addressed cache.
///
/// EVERY view-sensitive step flows through `ctx`:
/// - the owner SFC's `IndexedReady` ([`ctx.ensure_indexed_ready`]) — so an
///   overlay session keys on its OVERLAY `whole_hash`;
/// - the cold surface resolution
///   ([`VerterHost::resolve_vue_macro_surface_with_ctx`], whose dispatch is
///   `ctx.dispatch()`) — so the type-argument lowering and cross-file carrier
///   projection read overlay content;
/// - warm validation ([`ctx.store_view`]) — so a carrier edit invalidates the
///   entry lazily against the SAME view the surface was resolved under.
///
/// The cold producer populates EXACTLY the surface matching the macro kind
/// (`props` for `DefineProps` / `DefineModel`, `emits` for `DefineEmits`,
/// `slots` for `DefineSlots`); the others stay `None`. Returns an empty
/// (default) bundle when the macro surface cannot be resolved.
///
/// [`ctx.ensure_indexed_ready`]: crate::resolver_core::ResolverContext::ensure_indexed_ready
/// [`ctx.store_view`]: crate::resolver_core::ResolverContext::store_view
#[must_use]
pub(crate) fn vue_macro_dtos_with_ctx(
    ctx: &dyn crate::resolver_core::ResolverContext,
    request: &VueMacroSurfaceRequest,
) -> Arc<MacroSurfaceDtos> {
    let host = ctx.host_for_fact_tracer_install();

    // Load the CURRENT (overlay-aware) `IndexedReady` BEFORE touching the cache.
    // The request's `root_identity` and `macro_kind` are caller-supplied and may
    // be STALE or WRONG; deriving both from the authoritative `ctx`-resolved
    // snapshot here means a stale `root_identity` can never read an old entry
    // and a wrong `macro_kind` can never read or poison the sibling kind's entry.
    let Some(indexed) = ctx.ensure_indexed_ready(request.owner_canonical.as_ref()) else {
        // SFC not loaded — no surface, no cache entry.
        return Arc::new(MacroSurfaceDtos::default());
    };
    let Some(mac) = indexed.snapshot.macros.get(request.macro_index) else {
        return Arc::new(MacroSurfaceDtos::default());
    };
    let whole_hash = indexed.whole_hash;
    let macro_kind = mac.kind;

    // The framework-neutral key: the four common columns (kind / query_level /
    // canonical / owner_whole_hash) plus the Vue adapter's typed remainder
    // (`macro_index` + `macro_kind`). The macro KIND is derived from the
    // authoritative snapshot, so a wrong caller hint can never alias the sibling
    // kind's slot. The wire surface kind is derived from the macro kind so the
    // store column matches the macro the DTO bundle was normalized for.
    let key = FullKey {
        kind: surface_kind_for_macro(macro_kind),
        query_level: request.level,
        canonical: Arc::clone(&request.owner_canonical),
        owner_whole_hash: whole_hash,
        adapter_key: VueSurfaceKey {
            macro_index: request.macro_index,
            macro_kind,
        },
    };
    let store = host.vue_surface_store();
    // Warm read against the SAME `ctx` view the surface resolves under. The
    // content-addressed key covers the SFC's OWN content, but the resolved DTOs
    // read CROSS-FILE carrier types; validating the recorded fact signature +
    // project generation against the live view invalidates the entry lazily on a
    // carrier edit.
    let generation = ctx.project_type_store().current_project_generation();
    if let Some(cached) = store.get_with_view(&key, ctx.store_view(), generation) {
        // Bubble the cached entry's cross-file carrier fact signature into any
        // active outer fact tracer so an outer component-meta cold trace inherits
        // the DTO's carrier facts on this warm hit.
        cached.read_set_signature.bubble_via_tls();
        return Arc::clone(&cached.dto_bundle);
    }

    // Resolve the surface through a request carrying the VALIDATED identity
    // (live `whole_hash`) and the AUTHORITATIVE kind. The whole cold resolution
    // runs under an installed fact tracer so the CROSS-FILE carrier facts it
    // reads are captured into the entry's `ReadSetSignature`.
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
                AnalyzedMacroKind::DefineProps => MacroSurfaceDtos {
                    // A props member is `properties + index signatures`: capture
                    // the surface's index signatures (key/value raised through
                    // `ctx`) for `define_props_shape` to publish.
                    props: Some(PropsSurface {
                        fields: props_from_typeinfo_surface(ctx, &macro_surface),
                        index_signatures: index_signatures_from_surface(ctx, &macro_surface),
                    }),
                    ..MacroSurfaceDtos::default()
                },
                AnalyzedMacroKind::DefineModel => {
                    // `defineModel` synthesizes a prop (the model's value type),
                    // surfaced in BOTH slots: the `props` slot keeps the
                    // component-meta contract (the model contributes a prop), and
                    // the `model` slot carries the binding(s) the MODEL framework
                    // surface reads. `model_prop_fields` is the shared synthesis
                    // source for both, so the two slots stay in lock-step.
                    let prop_fields = props_from_typeinfo_surface(ctx, &macro_surface);
                    let bindings = prop_fields
                        .iter()
                        .map(
                            |prop| crate::typeinfo::framework_surface::results::ModelBinding {
                                name: prop.name.clone(),
                                prop: prop.clone(),
                            },
                        )
                        .collect();
                    MacroSurfaceDtos {
                        props: Some(PropsSurface {
                            fields: prop_fields,
                            index_signatures: index_signatures_from_surface(ctx, &macro_surface),
                        }),
                        model: Some(crate::typeinfo::framework_surface::results::ModelSurface {
                            bindings,
                        }),
                        ..MacroSurfaceDtos::default()
                    }
                }
                AnalyzedMacroKind::DefineEmits => MacroSurfaceDtos {
                    // The emits object is `events + index signatures`: capture
                    // the index signature (key/value raised through `ctx`) for
                    // `define_emits_shape` to publish — the retired materialiser
                    // surfaced it, so dropping it on the dispatch path was a
                    // regression.
                    emits: Some(EmitsSurface {
                        fields: emits_from_typeinfo_surface(ctx, &macro_surface),
                        index_signatures: index_signatures_from_surface(ctx, &macro_surface),
                    }),
                    ..MacroSurfaceDtos::default()
                },
                AnalyzedMacroKind::DefineSlots => MacroSurfaceDtos {
                    slots: Some(slots_from_typeinfo_surface(ctx, &macro_surface)),
                    ..MacroSurfaceDtos::default()
                },
                // `defineOptions<T>()` / `defineExpose<T>()` are object-member
                // surfaces: the type argument projects to the SAME one-level
                // object surface props/emits/slots resolve through (the SHARED
                // resolver), normalized here as the pass-through
                // `NamedTypeMember` set. A SUPPORTED-with-members surface — never
                // a silent supported-empty / unsupported-because-present.
                AnalyzedMacroKind::DefineOptions => MacroSurfaceDtos {
                    options: Some(
                        crate::typeinfo::framework_surface::results::OptionsSurface {
                            members: object_members_from_typeinfo_surface(ctx, &macro_surface),
                        },
                    ),
                    ..MacroSurfaceDtos::default()
                },
                AnalyzedMacroKind::DefineExpose => MacroSurfaceDtos {
                    expose: Some(crate::typeinfo::framework_surface::results::ExposeSurface {
                        members: object_members_from_typeinfo_surface(ctx, &macro_surface),
                    }),
                    ..MacroSurfaceDtos::default()
                },
                // `WithDefaults` is not a props-surface source on this path: the
                // outer `withDefaults` macro carries no type argument, so
                // `resolve_vue_macro_surface_with_ctx` returns `None` for it and
                // this arm is unreachable.
                AnalyzedMacroKind::WithDefaults => MacroSurfaceDtos::default(),
            },
            None => MacroSurfaceDtos::default(),
        }
    });

    match finalise {
        crate::resolver_core::FactReadSetFinalise::Ok(facts) => {
            let entry = StoredSurfaceDto {
                dto_bundle: Arc::new(dtos),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                validated_at_generation: generation,
            };
            Arc::clone(&store.insert(key, entry).dto_bundle)
        }
        // Tracer overflowed: the DTOs are valid but cannot be admitted safely
        // (the observation set was truncated). Return the freshly-computed
        // bundle WITHOUT caching — a repeat request recomputes, never serves an
        // under-validated entry.
        crate::resolver_core::FactReadSetFinalise::Overflow => Arc::new(dtos),
    }
}

/// The wire framework-surface kind a macro kind publishes its DTO bundle under.
///
/// `DefineProps` / `WithDefaults` / `DefineModel` contribute the PROPS slot;
/// `DefineEmits` the EMITS slot; `DefineSlots` the SLOTS slot; the object
/// macros (`DefineOptions` / `DefineExpose`) their own slots. This keys the
/// store column so two macros of different kinds never alias a slot.
fn surface_kind_for_macro(
    macro_kind: AnalyzedMacroKind,
) -> verter_protocol::typeinfo::graph::FrameworkSurfaceKind {
    use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => FrameworkSurfaceKind::Props,
        AnalyzedMacroKind::DefineEmits => FrameworkSurfaceKind::Emits,
        AnalyzedMacroKind::DefineSlots => FrameworkSurfaceKind::Slots,
        AnalyzedMacroKind::DefineOptions => FrameworkSurfaceKind::Options,
        AnalyzedMacroKind::DefineExpose => FrameworkSurfaceKind::Expose,
    }
}

/// Normalize a `.vue` props macro surface into the published
/// [`AnalyzedPropField`] set.
///
/// Reproduces the eager rail's `AnalyzedPropField` stream over the typeinfo
/// surface: one field per named member, carrying the surface's `optional` /
/// `readonly` / `declared_in_macro_type_arg`, the member value raised to a
/// `TypeExpr` scoped to its VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]), the display `type_annotation`
/// rendered from that typed form, and JSDoc sliced from the surface spans.
/// Own-body-vs-heritage ordering + shadowing + union-common membership are
/// ALREADY resolved on the surface — this is a thin per-member transform.
///
/// `defineModel` does NOT carry an object type argument; its surface has no
/// named members and the synthesized model prop is appended from the analyzer
/// facts ([`AnalyzedMacroKind::DefineModel`]'s `prop_fields`).
#[must_use]
pub(crate) fn props_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedPropField> {
    // Host-level reads (graph node scope, JSDoc source slicing) go through the
    // host the active `ctx` is installed against; the view-sensitive type
    // resolution (`raise_member_value`) flows through `ctx`.
    let host = ctx.host_for_fact_tracer_install();
    // `defineModel` contributes its synthesized model prop directly from the
    // analyzer facts (the type argument is the model VALUE type).
    if macro_surface.macro_kind == AnalyzedMacroKind::DefineModel {
        return model_prop_fields(ctx, macro_surface);
    }

    macro_surface
        .surface
        .members
        .iter()
        // Publication-boundary visibility filter: the shared surface RECORDS
        // non-public class members, but Vue does NOT expose `private` /
        // `protected` class fields as props.
        .filter(|member| member.visibility.is_public())
        .map(|member| {
            let type_expr = raise_member_value(ctx, member);
            let type_expr_scope = type_expr
                .as_ref()
                .map(|_| macro_surface.member_expr_scope(host, member));
            let type_annotation = type_expr.as_ref().and_then(render_type_expr_display);
            let (description, tags) = member_jsdoc_from_spans(host, member);
            // `declared_in_macro_type_arg`: a member belongs to the macro-T own
            // body iff it is NOT heritage-reached. The terminal
            // `MacroTypeArgOwnBody` synthesis already stamps this correctly. The
            // `&& merge_role != Heritage` conjunct is REDUNDANT defense-in-depth
            // (a member can only carry `declared_in_macro_type_arg == true` if it
            // is an own-body `member_index` member).
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

/// Normalize a `defineOptions<T>()` / `defineExpose<T>()` macro surface into the
/// neutral [`NamedTypeMember`] set — the pass-through object surface (D-s
/// options/expose are an object-member surface, NOT a prop/emit/slot normalize).
///
/// The macro surface is ALREADY the one-level object surface
/// [`VerterHost::resolve_vue_macro_surface_with_ctx`] projected from the type
/// argument through the SHARED resolver (no special-case there — only
/// `defineModel` is). This is the thin per-member normalize: one
/// [`NamedTypeMember`] per public named member carrying its name, optionality,
/// and the member value raised to a `TypeExpr` through the active `ctx`. The
/// shallow-by-default rule holds — `raise_member_value` raises the member's
/// one-level value node, it does not eagerly expand it.
#[must_use]
pub(crate) fn object_members_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<crate::typeinfo::framework_surface::results::NamedTypeMember> {
    macro_surface
        .surface
        .members
        .iter()
        // Publication-boundary visibility filter (symmetric with props): the
        // shared surface RECORDS non-public class members, but the published
        // object surface exposes only public members.
        .filter(|member| member.visibility.is_public())
        .map(
            |member| crate::typeinfo::framework_surface::results::NamedTypeMember {
                name: member.name.as_ref().to_string(),
                is_optional: member.optional,
                type_expr: raise_member_value(ctx, member),
            },
        )
        .collect()
}

/// Normalize a macro surface's INDEX SIGNATURES into the published
/// [`ExpandedIndexSignature`] set. A props member is `properties + index
/// signatures` and an emits object is `events + index signatures`. Kind-neutral:
/// it raises whatever index signatures the surface carries. Each signature's
/// `key_type` / `value_type` graph node is raised to a `TypeExpr` through the
/// ACTIVE `ctx` (overlay-aware); a node that does not raise is skipped (no
/// phantom signature).
fn index_signatures_from_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<ExpandedIndexSignature> {
    let dispatch = ctx.dispatch();
    macro_surface
        .surface
        .index_signatures
        .iter()
        .filter_map(|sig| {
            let key_type = dispatch.raise_node_to_type_expr(sig.key_type)?;
            let value_type = dispatch.raise_node_to_type_expr(sig.value_type)?;
            Some(ExpandedIndexSignature {
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
/// session reads the OVERLAY `defineModel` macro facts.
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
/// 1. **Call-signature emits FIRST.** Each call signature's first parameter is
///    the event name (a `String` literal, or a `Union` of `String` literals);
///    the typed `payload_expr` is the call-signature function with the leading
///    event-name parameter STRIPPED. The event name is NEVER read from `keyof`.
///    The display `payload_type` is a CONSISTENT source-span slice.
/// 2. **Property-style emits as a FALLBACK** — only when no call-signature emit
///    was found.
/// 3. **De-duplicate by event name, first-writer-wins.**
#[must_use]
pub(crate) fn emits_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedEmitField> {
    // View-sensitive type resolution flows through the active `ctx`
    // (`ctx.dispatch()`). Host-level reads (JSDoc source slicing, node scope)
    // use the host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    let dispatch = ctx.dispatch();

    let mut emits: Vec<AnalyzedEmitField> = Vec::new();

    // (1) Call-signature emits.
    for sig in macro_surface.surface.call_signatures.iter() {
        // `TypeExpr` implements `Drop`, so `func` cannot be moved out of the
        // raised value; bind it and borrow the function.
        let raised = dispatch.raise_node_to_type_expr(sig.node);
        let Some(TypeExpr::Function(func)) = &raised else {
            continue;
        };
        let Some(first) = func.parameters.first() else {
            continue;
        };
        // Payload = the call signature's REMAINING parameters (after the leading
        // event-name parameter) as a TUPLE — the Vue emit payload shape. This
        // matches the eager OXC rail's `AnalyzedEmitField.payload_expr` (a
        // `TypeExpr::Tuple`). Each surviving parameter maps to a labelled tuple
        // element preserving its name / optional / rest.
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
        // Scope the payload to the call signature's DECLARATION-origin file so an
        // inherited cross-file emit signature's payload `Ref`s resolve in the
        // base file. Falls back to the SFC owner.
        let payload_scope = macro_surface.signature_expr_scope(sig);
        // `payload_type` (→ `rawType`) is DISPLAY-ONLY — no consumer parses it.
        // It mirrors the payload TUPLE rendered as `[label: T, ...]`.
        let payload_type = render_type_expr_display(&payload_tuple);
        // The event's JSDoc rides on the call signature itself, sliced from the
        // signature's typeinfo JSDoc spans. A union of event-name literals on
        // ONE signature shares that signature's JSDoc across each event.
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
        for member in macro_surface
            .surface
            .members
            .iter()
            // Public-only publication: a `private` / `protected` class member
            // recorded on the shared surface must NOT leak as a published emit.
            .filter(|member| member.visibility.is_public())
        {
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
/// types, NOT a single pre-merged `Function`. A slot typed as a union of
/// function aliases resolves its `default` member to a `Union` of two function
/// types. Both are slot-callable. Returns:
///
/// - `Function(f)` → `f`'s first-param type + return type directly.
/// - `Intersection(arms)` / `Union(arms)` where EVERY resolvable arm is a
///   function → the INTERSECTION of the arms' first-param types plus the
///   combined return type (intersection of returns for an intersection, union of
///   returns for a union).
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
            // an unresolved reference (`VNode` not imported).
            func.spans.return_type,
        )),
        // Intersection of slot-callable arms: param = intersection of first
        // params (required-wins merge), return = intersection of returns.
        TypeExpr::Intersection(arms) => {
            slot_callable_param_and_return_from_arms(arms, ArmCombine::Intersection)
        }
        // Union of slot-callable arms (`SlotA | SlotB`): param stays the
        // INTERSECTION of first params (a slot prop the template can rely on must
        // be present in every arm — contravariant param), but the return is the
        // UNION of the arms' return types (covariant).
        TypeExpr::Union(arms) => slot_callable_param_and_return_from_arms(arms, ArmCombine::Union),
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
/// member not slot-like → `None`). The first params are intersected; the returns
/// are combined per `combine`.
///
/// SOUNDNESS — a slot binding is guaranteed only if EVERY arm supplies a first
/// parameter. A template destructuring `<template #default="{ x }">` runs for
/// WHICHEVER arm the slot actually is, so a binding the template can rely on must
/// be present across all arms. If ANY arm is a no-param callable (`() => any`),
/// the multi-arm callable can be invoked with no slot props in that branch, so
/// there are NO guaranteed bindings — the first param is dropped to `None`. The
/// return type still combines across arms.
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
    let mut all_arms_have_first_param = true;
    for arm in arms.iter() {
        let TypeExpr::Function(func) = arm else {
            // A non-function arm means the member is not purely slot-callable.
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
    // First params: the INTERSECTION — but ONLY when every arm supplied a first
    // param. A no-param arm guarantees nothing, so the bindings are dropped.
    let first_param = if all_arms_have_first_param {
        match first_params.len() {
            0 => None,
            1 => Some(first_params.into_iter().next().unwrap()),
            _ => Some(TypeExpr::Intersection(Arc::from(
                first_params.into_boxed_slice(),
            ))),
        }
    } else {
        None
    };
    // Returns: combine per the arm kind.
    let return_ty = match returns.len() {
        0 => None,
        1 => Some(returns.into_iter().next().unwrap()),
        _ => {
            let boxed = Arc::from(returns.into_boxed_slice());
            Some(match combine {
                ArmCombine::Intersection => TypeExpr::Intersection(boxed),
                ArmCombine::Union => TypeExpr::Union(boxed),
            })
        }
    };
    // A composed multi-arm callable has no single return-type span.
    Some((first_param, return_ty, None))
}

/// Normalize a `.vue` slots macro surface into the published
/// [`AnalyzedSlotField`] set.
///
/// Keep FUNCTION-LIKE members only (the value raises to a `TypeExpr::Function`;
/// non-function members are filtered); the slot's `bindings` come from resolving
/// the function's first-parameter type to its object surface (a literal object,
/// a `Pick<…>`, or a named alias — see [`binding_fields_from_param_ty`]); the
/// `return_expr` / `return_type` come from the function's return type. Bindings +
/// return are scoped to the slot member's VALUE-NODE file (see
/// [`VueMacroSurface::member_expr_scope`]).
#[must_use]
pub(crate) fn slots_from_typeinfo_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    macro_surface: &VueMacroSurface,
) -> Vec<AnalyzedSlotField> {
    // View-sensitive slot type resolution flows through the active `ctx`.
    // Host-level reads (JSDoc / return-type source slicing, node scope) use the
    // host the `ctx` is installed against.
    let host = ctx.host_for_fact_tracer_install();
    macro_surface
        .surface
        .members
        .iter()
        // Public-only publication: a `private` / `protected` class member must
        // NOT leak as a published slot.
        .filter(|member| member.visibility.is_public())
        .filter_map(|member| {
            // A slot member's value may be a non-`Function` carrier shell under
            // the transit-shallow macro surface — most notably a generic slot
            // alias that lowers to an `InstantiationRef` / alias carrier rather
            // than a reduced `Function`. Realize the value through the SHARED
            // callable-realization substrate so a decidable callable surfaces as
            // a `Function` BEFORE the function-like filter — otherwise the
            // generic slot is silently dropped.
            let value = raise_realized_callable_member_value(ctx, member)?;
            // A slot member is function-like: a single `Function`, or an
            // `Intersection` of functions, or a `Union` of functions.
            let (first_param, return_expr, return_span) = slot_callable_param_and_return(&value)?;
            let scope = macro_surface.member_expr_scope(host, member);
            let bindings = first_param
                .as_ref()
                .map(|param_ty| binding_fields_from_param_ty(ctx, param_ty, &scope))
                .unwrap_or_default();
            let return_expr_scope = return_expr.as_ref().map(|_| scope.clone());
            // Display `return_type`: prefer the EXACT source text sliced from the
            // return-type annotation span — this preserves a name the typed
            // return cannot surface (an unresolved imported `VNode`). Fall back
            // to rendering the typed return when there is no span. Display-only.
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

/// Reconstruct a slot's binding fields from its function's first-parameter type.
/// Each member of the parameter's OBJECT surface becomes one
/// [`AnalyzedSlotFieldBinding`] carrying that member's value `TypeExpr` as
/// `binding_expr`.
///
/// The first parameter is the slot-props object. It can be written several ways:
/// a literal object, a `Pick<T, 'k'>` over a named type, or a parenthesized
/// form. To handle all of them WITHOUT a nominal shape-sniff, the binding object
/// is obtained by RESOLVING the first-parameter type through the SHARED
/// resolver:
///
/// - A literal [`TypeExpr::Object`] is read directly (no resolution needed).
/// - Any other shape (`Pick<…>` / `Omit<…>` / a `Ref` to a named alias /
///   `Parenthesized`) is lowered and projected to its one-level object surface
///   ([`navigate_param_to_object_surface`]); each surface member becomes a
///   binding.
///
/// A first parameter that does not resolve to an object surface yields no
/// bindings.
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
                // Public-only publication.
                verter_type_expr::ObjectMember::Property(prop) if prop.visibility.is_public() => {
                    Some(AnalyzedSlotFieldBinding {
                        name: prop.name.clone(),
                        type_annotation: render_type_expr_display(&prop.ty),
                        binding_expr: Some(prop.ty.clone()),
                        binding_expr_scope: Some(scope.clone()),
                        span: verter_span::Span::default(),
                    })
                }
                _ => None,
            })
            .collect();
    }

    // Non-object first param (`Pick<…>` / alias `Ref` / `Parenthesized`):
    // navigate it through the shared resolver to its object surface.
    let Some(surface) = navigate_param_to_object_surface(ctx, scope.as_str(), param_ty) else {
        return Vec::new();
    };
    // Shallow-by-default Pick member publication: when the slot param is a
    // `Pick<NamedRoot, K>` the picked members stay SYMBOLIC at the published
    // binding surface — each binding's value is the typed indexed access
    // `NamedRoot['member']` (built from the typed param, not reparsed). Other
    // shapes keep the navigated value.
    let pick_symbolic_root = pick_named_source_root(param_ty);
    surface
        .members
        .iter()
        // Public-only publication.
        .filter(|member| member.visibility.is_public())
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

/// When `param_ty` is structurally `Pick<NamedRoot, K>` (modulo `Parenthesized`
/// wrappers) with `NamedRoot` a nominal [`TypeExpr::Ref`], return that
/// source-root `Ref` so a slot binding can publish each picked member as the
/// symbolic `NamedRoot['member']` indexed access. This is a STRUCTURAL match on
/// the typed IR — no type-text sniffing, no reparse. Any other shape returns
/// `None`.
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
