#![deny(missing_docs)]
//! The relocated Vue resolution delegates — public component type + the
//! FullMetadata macro surface and its prop / emit / slot / expose normalizers.
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
//! `defineSlots<S>()` / `defineExpose<T>()` / `withDefaults(defineProps<T>(),
//! …)`) to the span-rich [`VueMacroSurface`] at
//! [`TypeInfoQueryLevel::FullMetadata`] through the SHARED typeinfo surface
//! path — the macro type-argument is lowered through the shared lowering
//! dispatch and projected by the SAME empty-path `Shallow` synthesiser
//! [`crate::VerterHost::resolve_shallow_surface_for`] uses (NEVER a parallel
//! reader). The four normalizers ([`props_from_typeinfo_surface`] /
//! [`emits_from_typeinfo_surface`] / [`slots_from_typeinfo_surface`] /
//! [`exposed_from_typeinfo_surface`]) consume the policy-admitted
//! [`ResolvedVueSurface`] token (a sealed wrapper over the resolved
//! [`VueMacroSurface`] — member `value` + spans + origin + flags + JSDoc spans)
//! plus the macro-analyzer facts and produce the FINAL component-meta DTOs
//! (`AnalyzedPropField` / `AnalyzedEmitField` / `AnalyzedSlotField` /
//! `AnalyzedExposeField`), sourcing every semantic decision from the typeinfo
//! surface. The token is minted ONLY by [`resolved_vue_surface`] inside this
//! framework-surface sink (the Vue resolution path here and the Svelte
//! `macro_surface_shell`), so no non-sink code can forge a surface and
//! reverse-materialize a member `TypeExpr`:
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
//!   parameter STRIPPED), property-key members only as a fallback when no
//!   call-signature emit was found, de-duplicated by event name
//!   (first-writer-wins).
//! - **slots** — function-like members only (non-function members filtered);
//!   the first-parameter object's properties become the slot bindings; the
//!   function return type becomes the slot return.
//! - **exposed** — one field per PUBLIC named member of the `defineExpose<T>()`
//!   type argument, carrying the member value raised to a `TypeExpr` (scoped to
//!   its value-node file like props) and JSDoc sliced from the surface spans;
//!   downstream `extract_exposed_from_macro` publishes the union of these
//!   surface members and the object-literal argument fields.
//!
//! Fallthrough / root-inheritance + options are SEPARATE subsystems fed by
//! analyzer facts — out of scope here.
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

use verter_semantic::analysis::types::{AnalyzedMacroKind, JsdocTag};
use verter_type_expr::{TypeExpr, TypeExprScope};

use crate::framework::surface_store::{FullKey, StoredSurfaceDto};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    PathSegment, ProjectionMode, ProjectionReductionContext, QueryResult, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput, SurfaceProvenanceContext,
};
use crate::typeinfo::framework_surface::results::{EmitsSurface, MacroSurfaceDtos, PropsSurface};
use crate::typeinfo::framework_surface::VueSurfaceKey;
use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurface, TypeInfoSurfaceMember};
use crate::typeinfo::types::{TypeInfoQueryLevel, VueMacroSurfaceRequest};
use crate::VerterHost;

mod normalize;
mod normalize_slots;

// The per-surface normalizers live in the `normalize` submodule (file-size
// split). Re-export the ones the executor below and external consumers reach
// through the flat `vue_exec::props_from_typeinfo_surface` path.
pub(crate) use normalize::{
    emits_from_typeinfo_surface, exposed_from_typeinfo_surface, index_signatures_from_surface,
    object_members_from_typeinfo_surface, props_from_typeinfo_surface,
};
pub(crate) use normalize_slots::slots_from_typeinfo_surface;

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The Vue framework-surface executor's output-sink capability: the Vue
    /// resolution leg here (and its normalizer children) hold this to
    /// materialize a graph node into a sealed output carrier and unwrap it.
    /// Its constructor is visible ONLY within
    /// `crate::typeinfo::framework_surface::vue_exec` (this module + its
    /// children) — NOT the whole `typeinfo` subtree — so no
    /// `typeinfo` sibling can mint it (planted
    /// `TypeinfoVueSurfaceOutputCap::new` outside this leaf is `E0624`).
    pub(crate) struct TypeinfoVueSurfaceOutputCap;
    mint: pub(in crate::typeinfo::framework_surface::vue_exec)
}

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
    /// SURFACE-COMPOSITION reference arms (heritage `extends` parents,
    /// intersection / union arms) the shallow walker dropped as unresolvable
    /// while synthesising `surface` — name-sorted, deduplicated. The
    /// compile-facing collector classifies each arm (import-backed vs
    /// ambient) and tiers import-backed misses as fatal; ambient names stay
    /// silent.
    pub(crate) unresolved_surface_arms: Vec<UnresolvedSurfaceArm>,
}

/// One unresolvable SURFACE-COMPOSITION reference arm dropped during macro
/// surface synthesis: the arm's head name plus the canonical file whose
/// declaration authored it (the file whose import bindings classify the miss
/// as import-backed vs ambient).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnresolvedSurfaceArm {
    /// The reference's head name as written (`NotFound` in `extends NotFound`).
    pub(crate) name: Arc<str>,
    /// Canonical id of the file whose declaration authored the arm.
    pub(crate) owner_canonical: Arc<str>,
    /// Exact top-level lexical owner that authored the arm.
    pub(crate) owner: verter_type_expr::TopLevelOwnerId,
}

/// Extract the unresolved SURFACE-COMPOSITION arm facts from a projection's
/// walker diagnostics, name-sorted (then by declaring file) and deduplicated
/// so consumers emit deterministically ordered reports.
fn unresolved_surface_arms_from_diags(
    diags: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
) -> Vec<UnresolvedSurfaceArm> {
    let mut arms: Vec<UnresolvedSurfaceArm> = diags
        .iter()
        .filter_map(|diag| match diag {
            crate::project_semantic_dispatch::walk::ShallowDiagnostic::UnresolvedSurfaceArm {
                name,
                owner_canonical,
                owner,
            } => Some(UnresolvedSurfaceArm {
                name: Arc::clone(name),
                owner_canonical: Arc::clone(owner_canonical),
                owner: *owner,
            }),
            _ => None,
        })
        .collect();
    arms.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.owner_canonical.cmp(&b.owner_canonical))
            .then_with(|| a.owner.cmp(&b.owner))
    });
    arms.dedup();
    arms
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

/// Framework-surface mint seal. Private to `vue_exec`; with no public
/// constructor, the only code that can place a `FrameworkSurfaceSeal` into a
/// [`ResolvedVueSurface`] — and therefore the only code that can mint one — is
/// the `vue_exec`-private authority fn [`resolved_vue_surface`].
struct FrameworkSurfaceSeal;

/// A RESOLVED, policy-admitted `.vue` macro surface — a sealed token the Vue
/// per-kind NORMALIZERS accept through
/// [`super::resolved_surface_access::ResolvedSurfaceAccess`].
///
/// The leak this closes: the normalizers (`props_from_typeinfo_surface` etc.)
/// together with the member raiser (`raise_member_value`) take a
/// `VueMacroSurface` / `TypeInfoSurfaceMember` and reverse-materialize a
/// `TypeExpr`-bearing DTO.
/// `VueMacroSurface` is a `pub` wire-adjacent carrier with public fields, so
/// outside `framework_surface` ANY code could forge one and drive the
/// normalizers. Gating the normalizers on the sealed
/// [`super::resolved_surface_access::ResolvedSurfaceAccess`] trait — whose
/// supertrait seal is PRIVATE to `resolved_surface_access.rs`, where the only
/// `impl` for this token (and the Svelte token) lives — makes that a compile
/// error: outside the sink there is no way to mint a token (private
/// [`FrameworkSurfaceSeal`], private `surface` field, minted ONLY by the
/// `vue_exec`-private [`resolved_vue_surface`]), and no module outside
/// `resolved_surface_access.rs` can implement the accessor (a sibling
/// `impl Sealed` is `E0603`), so the normalizers cannot be called without one.
///
/// The wrapped surface comes from real resolution — the Vue path's
/// `resolve_vue_macro_surface_with_ctx` — never a caller-forged surface from
/// outside the sink.
pub(crate) struct ResolvedVueSurface {
    surface: VueMacroSurface,
    _seal: FrameworkSurfaceSeal,
}

impl ResolvedVueSurface {
    /// The resolution-derived carrier the shared normalizers read, by borrow.
    /// Exposed to the `framework_surface`-level
    /// [`super::resolved_surface_access`] module (the SOLE implementor of
    /// `ResolvedSurfaceAccess`) while the private `surface` field + the
    /// [`FrameworkSurfaceSeal`] keep the token unmintable and unmodifiable
    /// outside `vue_exec`.
    pub(in crate::typeinfo::framework_surface) fn surface_carrier(&self) -> &VueMacroSurface {
        &self.surface
    }
}

/// Mint a [`ResolvedVueSurface`] token from a resolution-derived
/// [`VueMacroSurface`]. PRIVATE to `vue_exec`
/// (`pub(in crate::typeinfo::framework_surface::vue_exec)`): only the Vue
/// resolution path here (`vue_macro_dtos_with_ctx`) can mint, after it resolves
/// a surface. The private [`FrameworkSurfaceSeal`] field keeps the token
/// unmintable elsewhere — a `framework_surface` sibling can no longer forge a
/// `VueMacroSurface` and mint the token. Svelte mints its OWN sealed token.
pub(in crate::typeinfo::framework_surface::vue_exec) fn resolved_vue_surface(
    surface: VueMacroSurface,
) -> ResolvedVueSurface {
    ResolvedVueSurface {
        surface,
        _seal: FrameworkSurfaceSeal,
    }
}

/// TEST-ONLY minter for the typeinfo adapter tests, which resolve a REAL
/// `VueMacroSurface` (via `resolve_vue_macro_surface`) and then exercise a
/// normalizer directly. They live under `typeinfo::typeinfo_tests` — outside
/// the `framework_surface` sink — so they cannot reach the production
/// [`resolved_vue_surface`] minter. Named `_for_test` so it can never
/// masquerade as the production minter; gated `#[cfg(test)]` for zero
/// footprint outside test builds.
#[cfg(test)]
pub(crate) fn resolved_vue_surface_for_test(surface: VueMacroSurface) -> ResolvedVueSurface {
    resolved_vue_surface(surface)
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
        let indexed = self.ensure_indexed_ready_serve(canonical_id)?.indexed;
        let crate::resolver_core::shallow_file_state::ExportTarget::Local {
            owner: default_owner,
            symbol_name: default_name,
        } = indexed.shallow_state.exports.get("default")?
        else {
            return None;
        };
        let default_owner = *default_owner;
        let default_symbol = indexed
            .shallow_state
            .value_symbol_in(default_owner, default_name.as_str())?;
        if !default_symbol.is_synthesised_component_default {
            return None;
        }
        // The synthesized default carries the instance object as the
        // annotation-borne closed SOURCE on the synthesized BODY (not the slim
        // header); its absence means no public instance surface. The synth's
        // construct signature deliberately carries no authored return position
        // (`return_ty` is an honest `None`), so the annotation is the gate.
        let default_body = indexed
            .shallow_state
            .value_decl_in(default_owner, default_name.as_str())?;
        default_body.type_annotation.annotation.as_ref()?;
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
        let base = match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                dispatch.type_slot_for(
                    Arc::from(canonical_id),
                    default_owner,
                    Arc::from(default_name.as_str()),
                ),
                Arc::from(Vec::new().into_boxed_slice()),
                dispatch.instantiate_context_for(
                    canonical_id,
                    ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                ),
            ),
        )) {
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
            None,
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

        // Structurally read-only: surface resolution runs inside the
        // DTO producer's traced scope; fenced-ness gates admission there.
        let indexed = ctx
            .ensure_indexed_ready_serve(request.owner_canonical.as_ref())?
            .indexed;
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
                // The empty model surface projects nothing — no arms.
                unresolved_surface_arms: Vec::new(),
            });
        }

        let _ = mac.parsed_type_argument.as_ref()?;

        // Provenance per macro axis. Props request the macro-T own-body
        // provenance on the terminal surface synthesis so the author-declared
        // members are flagged; emits / slots / exposed are structural
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
        // own-body provenance; emits / slots / exposed are structural.
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

        // Read the macro arg's mode-neutral mirror handle (the ONE producer),
        // then decompose its indexed-access structure GRAPH-NATIVE. A deep
        // indexed-access type argument (`defineProps<DeepConfig['ui']['header']>()`)
        // lowered to nested `IndexedAccess` carrier shells; this walks those
        // shells into `(base_node, path)` WITHOUT lowering the base a second
        // time — the base node IS a different DEMAND on the same handle. The
        // shared path walker runs intermediate hops in `Navigate` and the
        // TERMINAL hop under `terminal_context` (Shallow). A non-indexed type
        // argument decomposes to `(handle_node, [])`.
        let handle = crate::structural_carrier_producer::macro_type_arg_hot_ref(
            ctx,
            request.owner_canonical.as_ref(),
            request.macro_index,
        )?;
        let (base_carrier, path) =
            crate::meta_resolve::dispatch_helpers::decompose_indexed_access_chain_node(
                ctx,
                handle.node(),
            );
        // Resolve the carrier base ONE Navigate hop through the shared dispatch
        // (carrier head resolution — a `BareRef` head routes to its `DeclRef`,
        // a `TypeOf` shell executes its value root, member values stay shallow),
        // reproducing the eager structural-transit-Navigate base lowering. The
        // path-precise `Shallow` projection then synthesises the one-level
        // surface of the terminal hop.
        let base = dispatch.resolve_hot_handle_with_context(
            crate::semantic_query::HotTypeRef::new(base_carrier),
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate),
        );

        // Collect the walker's side-band diagnostics so unresolvable
        // SURFACE-COMPOSITION arms (heritage / intersection / union) the
        // shallow synthesis dropped ride the resolved surface to the
        // compile-facing collector.
        let mut walker_diagnostics = Vec::new();
        let surface = self.project_shallow_surface_from_base(
            ctx,
            &dispatch,
            base,
            path,
            terminal_context,
            Some(&mut walker_diagnostics),
        )?;

        Some(VueMacroSurface {
            surface,
            macro_kind: request.macro_kind,
            owner_canonical: Arc::clone(&request.owner_canonical),
            macro_index: request.macro_index,
            macro_call_span: mac.span,
            level: request.level,
            unresolved_surface_arms: unresolved_surface_arms_from_diags(&walker_diagnostics),
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
        // Base-view returner: hand back the DTO bundle. A partial surface is
        // already refused store admission inside `vue_macro_dtos_with_ctx`;
        // this bare-host entry has no request-result completeness to fold.
        vue_macro_dtos_with_ctx(&host_ctx, request).dtos
    }
}

/// Navigate an authored type-payload REF to its one-level object
/// [`TypeInfoSurface`] through the SHARED resolver bound to the ACTIVE `ctx`,
/// raising the content-free locator in `scope_canonical` then projecting the
/// empty-path `Shallow` surface.
///
/// Used by the framework-surface resolvers to resolve a captured authored
/// payload (a `$props()` runes type, a `createEventDispatcher<E>` event map —
/// `Pick<RowApi, 'name'>` / a named alias / a parenthesized form) to its
/// object surface WITHOUT a nominal shape-sniff: the locator raises through
/// the one shared raise bridge ([`ProjectSemanticDispatch::raise_semantic_type_source_to_hot`]
/// → the memoized `LowerLocator` / macro type-arg producer), `Pick` is
/// navigated, and an alias `Ref` is resolved by the one shared resolver.
/// Returns `None` when the scope file is not loaded, the locator does not
/// deref under the current view, or the type does not project to an object
/// surface.
///
/// Bound to `ctx` (`ctx.dispatch()`), so an overlay session resolves the
/// slot-param object against its OVERLAY content.
#[must_use]
pub(crate) fn navigate_param_to_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical: &str,
    payload: &verter_type_expr::locators::AuthoredTypePayloadRef,
) -> Option<TypeInfoSurface> {
    let dispatch = ctx.dispatch();
    let scope_owner = match &payload.locator {
        verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot) => slot.anchor.owner,
        verter_type_expr::locators::AuthoredBodyLocator::AugmentationBody(body) => {
            body.anchor.owner
        }
        verter_type_expr::locators::AuthoredBodyLocator::JsdocTypedefBody(body) => {
            body.anchor.owner
        }
        verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(payload) => {
            payload.anchor.owner
        }
    };

    // Raise the authored payload locator to its base node through the shared
    // source-raise bridge under structural-transit Navigate (member values
    // stay shallow); the empty-path Shallow projection then synthesises the
    // one-level object surface. An undeferenceable locator is an honest
    // `None` — never a fabricated stand-in node.
    let base = dispatch
        .raise_semantic_type_source_to_hot(
            &verter_type_expr::facts::SemanticTypeSource::Authored(payload.locator.clone()),
            crate::project_semantic_dispatch::semantic_source::SourceRaiseContext {
                scope_canonical_id: scope_canonical,
                scope_owner,
                context: ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                ),
                interior_failures: None,
            },
        )?
        .node();
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
            None,
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
pub(super) fn slice_canonical_span(host: &VerterHost, cspan: &CanonicalSpan) -> Option<String> {
    let indexed = host
        .ensure_indexed_ready_serve(cspan.file.as_ref())?
        .indexed;
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

/// Slice a surface MEMBER's leading-JSDoc spans into `(description, tags)`.
pub(super) fn member_jsdoc_from_spans(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        member.jsdoc_description_span.as_ref(),
        &member.jsdoc_tag_spans,
    )
}

/// Slice a call-SIGNATURE's leading-JSDoc spans into `(description, tags)` (the
/// call-signature emit path).
pub(super) fn signature_jsdoc_from_spans(
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
///
/// Confined to `vue_exec` (`pub(in …vue_exec)`, NOT the broader `pub(super)`):
/// the only callers are the token-gated normalizers in [`normalize`], so a
/// `framework_surface` sibling cannot forge a `&TypeInfoSurfaceMember` and
/// reverse-materialize a `TypeExpr` here. The forgeable-input boundary is
/// closed at the normalizer (it requires a [`ResolvedVueSurface`] token).
pub(in crate::typeinfo::framework_surface::vue_exec) fn raise_member_value(
    ctx: &dyn crate::resolver_core::ResolverContext,
    member: &TypeInfoSurfaceMember,
) -> Option<TypeExpr> {
    // Publication sink (DTO surface): materialize into a sealed carrier and
    // unwrap via the typeinfo output capability.
    let dispatch = ctx.dispatch();
    let cap = TypeinfoVueSurfaceOutputCap::new(&dispatch);
    cap.materialize_output_type_expr(member.value)
        .map(|raised| raised.into_type_expr(&cap))
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
/// - the owner SFC's `IndexedReady` ([`ctx.ensure_indexed_ready_serve`]) — so an
///   overlay session keys on its OVERLAY `whole_hash`, never the base hash, and
///   a base session can never read or poison an overlay entry (or vice-versa);
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
/// [`ctx.ensure_indexed_ready_serve`]: crate::resolver_core::ResolverContext::ensure_indexed_ready_serve
/// [`ctx.dispatch()`]: crate::resolver_core::ResolverContext::dispatch
/// [`ctx.store_view`]: crate::resolver_core::ResolverContext::store_view
#[must_use]
pub(crate) fn vue_macro_dtos_with_ctx(
    ctx: &dyn crate::resolver_core::ResolverContext,
    request: &VueMacroSurfaceRequest,
) -> crate::typeinfo::framework_surface::MacroDtosRead {
    use crate::semantic_query::ResultCompleteness;
    use crate::typeinfo::framework_surface::MacroDtosRead;

    let host = ctx.host_for_fact_tracer_install();

    // Load the CURRENT (overlay-aware) `IndexedReady` BEFORE touching the
    // cache. The request's `root_identity` (a `whole_hash` hint) and
    // `macro_kind` are caller-supplied and may be STALE or WRONG; deriving both
    // from the authoritative `ctx`-resolved snapshot here means a stale
    // `root_identity` can never read an old entry (the live `whole_hash` keys a
    // fresh slot) and a wrong `macro_kind` can never read or poison the sibling
    // kind's entry.
    let Some(indexed) = ctx
        .ensure_indexed_ready_serve(request.owner_canonical.as_ref())
        .map(|serve| serve.indexed)
    else {
        // SFC not loaded — no surface, no cache entry. Returning the default
        // bundle WITHOUT publishing (we have no validated key) keeps the cache
        // free of entries keyed on an unvalidated identity. A default bundle is
        // a COMPLETE empty surface (no fuse tripped), not a partial.
        return MacroDtosRead {
            dtos: Arc::new(MacroSurfaceDtos::default()),
            completeness: ResultCompleteness::Complete,
        };
    };
    let Some(mac) = indexed.snapshot.macros.get(request.macro_index) else {
        return MacroDtosRead {
            dtos: Arc::new(MacroSurfaceDtos::default()),
            completeness: ResultCompleteness::Complete,
        };
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
        // Only `Complete` bundles ever enter the store (see the cold-compute
        // gate below), so a warm hit is always complete.
        return MacroDtosRead {
            dtos: Arc::clone(&cached.dto_bundle),
            completeness: ResultCompleteness::Complete,
        };
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
    // Per-cold-compute completeness scope: the surface resolution re-enters
    // the shared dispatch (`resolve_vue_macro_surface_with_ctx` →
    // `project_shallow_surface_from_base` → `execute_read`), whose
    // `fold_cache_read_rails` folds any genuine partial (a tripped projection
    // budget / fatal `QueryError`) into the active scope. Reading the scope
    // AFTER the compute gives this surface's OWN completeness — so a PARTIAL
    // surface bundle is returned to the caller but NEVER admitted into
    // `vue_surface_store` (the no-poison invariant; a partial in the store
    // would launder a warm complete replay on the next request). Single-thread
    // by construction (the calling flight's thread); on drop the scope bubbles
    // its completeness into any enclosing compute scope, so an outer
    // component-meta cold compute inherits this surface's partiality.
    let _completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
    let (dtos, finalise) = crate::fact_signature_helpers::install_fact_tracer(host, || {
        match host.resolve_vue_macro_surface_with_ctx(ctx, &validated_request) {
            Some(macro_surface) => {
                // Mint the policy-admitted framework-surface token from the
                // RESOLVED surface; the normalizers consume the token, never a
                // forgeable `&VueMacroSurface`. The mint lives here — the
                // framework-surface resolution sink — so no non-sink code can
                // reverse-materialize a member `TypeExpr` from a forged surface.
                let resolved = resolved_vue_surface(macro_surface);
                match macro_kind {
                    AnalyzedMacroKind::DefineProps => MacroSurfaceDtos {
                        // A props member is `properties + index signatures`: capture
                        // the surface's index signatures (key/value raised through
                        // `ctx`) for `define_props_shape` to publish.
                        props: Some(PropsSurface {
                            fields: props_from_typeinfo_surface(ctx, &resolved),
                            index_signatures: index_signatures_from_surface(ctx, &resolved),
                            ..Default::default()
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
                        let prop_fields = props_from_typeinfo_surface(ctx, &resolved);
                        let bindings = prop_fields
                            .iter()
                            .map(
                                |row| crate::typeinfo::framework_surface::results::ModelBinding {
                                    name: row.analysis.name.clone(),
                                    prop: row.analysis.clone(),
                                },
                            )
                            .collect();
                        MacroSurfaceDtos {
                            props: Some(PropsSurface {
                                fields: prop_fields,
                                index_signatures: index_signatures_from_surface(ctx, &resolved),
                                ..Default::default()
                            }),
                            model: Some(
                                crate::typeinfo::framework_surface::results::ModelSurface {
                                    bindings,
                                },
                            ),
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
                            fields: emits_from_typeinfo_surface(ctx, &resolved),
                            index_signatures: index_signatures_from_surface(ctx, &resolved),
                        }),
                        ..MacroSurfaceDtos::default()
                    },
                    AnalyzedMacroKind::DefineSlots => MacroSurfaceDtos {
                        slots: Some(slots_from_typeinfo_surface(ctx, &resolved)),
                        ..MacroSurfaceDtos::default()
                    },
                    // `defineOptions<T>()` / `defineExpose<T>()` are object-member
                    // surfaces: the type argument projects to the SAME one-level
                    // object surface props/emits/slots resolve through (the SHARED
                    // resolver), normalized here as the pass-through
                    // `NamedTypeMember` set. A SUPPORTED-with-members surface —
                    // never a silent supported-empty / unsupported-because-present.
                    AnalyzedMacroKind::DefineOptions => MacroSurfaceDtos {
                        options: Some(
                            crate::typeinfo::framework_surface::results::OptionsSurface {
                                members: object_members_from_typeinfo_surface(ctx, &resolved),
                            },
                        ),
                        ..MacroSurfaceDtos::default()
                    },
                    AnalyzedMacroKind::DefineExpose => MacroSurfaceDtos {
                        expose: Some(crate::typeinfo::framework_surface::results::ExposeSurface {
                            members: object_members_from_typeinfo_surface(ctx, &resolved),
                        }),
                        // The component-meta extract layer needs the richer
                        // `AnalyzedExposeField` shape (scope + JSDoc) the neutral
                        // `ExposeSurface` pass-through drops, so resolve it here
                        // alongside the wire surface from the SAME macro surface.
                        exposed_fields: exposed_from_typeinfo_surface(ctx, &resolved),
                        ..MacroSurfaceDtos::default()
                    },
                    // `WithDefaults` is not a props-surface source on this path: the
                    // outer `withDefaults` macro carries no type argument, so
                    // `resolve_vue_macro_surface_with_ctx` returns `None` for it and
                    // this arm is unreachable.
                    AnalyzedMacroKind::WithDefaults => MacroSurfaceDtos::default(),
                }
            }
            None => MacroSurfaceDtos::default(),
        }
    });
    let non_cacheable_read_observed = matches!(
        &finalise,
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
    );

    // This surface's OWN completeness — folded from the cold compute's
    // contributing reads via the scope entered above. A genuine partial (a
    // tripped projection budget / fatal `QueryError`) is refused store
    // admission below; the bundle still returns to the caller.
    let completeness = crate::request_context::current_cold_compute_completeness();

    // ReturnOnly never publishes — fenced-serve arm: DTOs resolved from
    // a served-without-publication artifact must not enter the shared
    // metadata store (their carrier facts validate against the live
    // view). Return the freshly-computed bundle WITHOUT caching.
    if non_cacheable_read_observed {
        return MacroDtosRead {
            dtos: Arc::new(dtos),
            completeness,
        };
    }
    match finalise {
        // A `Complete` result with a sound fact signature is the ONLY bundle
        // admitted into the store. A `Partial` (budget exhaustion / fatal
        // `QueryError` mid-surface-resolution) is returned but NEVER cached —
        // caching a partial would launder a warm complete replay (re-running
        // the budget-constrained owner against warm per-arm memos no longer
        // re-trips the fuse).
        crate::resolver_core::FactReadSetFinalise::Ok(facts) if !completeness.is_partial() => {
            let entry = StoredSurfaceDto {
                dto_bundle: Arc::new(dtos),
                read_set_signature: crate::fact_signature_helpers::ReadSetSignature::new(facts),
                validated_at_generation: generation,
            };
            MacroDtosRead {
                dtos: Arc::clone(&store.insert(key, entry).dto_bundle),
                completeness,
            }
        }
        // Genuine partial (`Ok` finalise but partial completeness): valid
        // bundle, refused store admission. A repeat request recomputes against
        // the fresh budget.
        crate::resolver_core::FactReadSetFinalise::Ok(_) => MacroDtosRead {
            dtos: Arc::new(dtos),
            completeness,
        },
        crate::resolver_core::FactReadSetFinalise::NonCacheable(_) => MacroDtosRead {
            dtos: Arc::new(dtos),
            completeness,
        },
        // Tracer overflowed: the DTOs are valid but cannot be admitted safely
        // (the observation set was truncated). Return the freshly-computed
        // bundle WITHOUT caching — a repeat request recomputes, never serves an
        // under-validated entry.
        crate::resolver_core::FactReadSetFinalise::Overflow => MacroDtosRead {
            dtos: Arc::new(dtos),
            completeness,
        },
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
