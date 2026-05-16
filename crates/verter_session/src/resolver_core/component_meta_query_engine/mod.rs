//! Declaration-aware component-meta query engine.
//!
//! `ComponentMetaQueryEngine` is the per-request execution surface for
//! one `get_component_meta()` call. It resolves type declarations
//! lazily from the ctx's prepared-decl bundles. All solve-like
//! operations dispatch through [`ProjectSemanticDispatch`].
//!
//! ## Authority model — what is durable vs. scratch
//!
//! The engine sits **above** the host's authoritative caches and
//! **below** the public component-meta API. It does NOT own any
//! durable cache state. The authoritative caches that survive
//! beyond a single request are listed below; the engine reads from
//! them via [`ResolverContext`] and (where applicable) writes to
//! them through cooperative-admission `post_publish` so concurrent
//! requests collapse onto one cold build.
//!
//! ### Authoritative host-owned caches (durable, dep-validated, reused across queries)
//!
//! - [`MaterializeMemoDb`](crate::component_meta_caches::MaterializeMemoDb)
//!   — interned semantic instantiations keyed by
//!   `(target_decl, mode, args)`. Final-result reuse across requests.
//! - [`ComponentMetaResultDb`](crate::component_meta_caches::ComponentMetaResultDb)
//!   — final `ComponentMetaAnalysis` payloads keyed by `(canonical, profile)`.
//!   `get_component_meta` consults this first; the engine only runs on cold misses.
//! - [`SemanticGraphStore`](crate::semantic_query_memo::SemanticGraphStore)
//!   — interned semantic-node arena and resolved-named-type identity map.
//!   Engine subqueries dispatch via
//!   [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch)
//!   which deduplicates against this store.
//! - [`RefCycleResultDb`](crate::component_meta_caches::RefCycleResultDb)
//!   — host-cached transitive cycle-detection BFS results keyed by
//!   parameterized generic helpers. Cooperative-admission cold build.
//! - [`MaterializeStructureDb`](crate::component_meta_caches::MaterializeStructureDb)
//!   — interned structural projections produced by the canonical
//!   materialiser; sole authoritative materialiser cache.
//!
//! All five participate in `ProjectTypeStore`'s invalidation cascade
//! and are dep-signature-validated on warm hit via
//! [`HostFenceValidator`](crate::host_manage::HostFenceValidator).
//!
//! ### Per-request scratch (NOT promoted, dies with the engine)
//!
//! The engine retains a small set of `RefCell`-wrapped maps used to
//! avoid recomputing the same projection within one request. These
//! are scratch only:
//!
//! - `prepared_surface_cache` — read-through view of the ctx's
//!   `prepared_surface_db`; mirrors the durable result for the current
//!   request only.
//! - `routed_expr_surface_cache` — same shape, for routed-expr surfaces.
//! - `prepared_member_cache` — request-local memo of prepared-member
//!   projections.
//! - Type-param substitution maps and projection-chain scopes —
//!   per-frame state for the current dispatch path.
//!
//! **None of these are written back to the host store directly.**
//! When durable population is required, the engine uses the ctx's
//! cooperative-admission `post_publish` path so the host store sees
//! exactly one canonical write per cache key.
//!
//! ### Never-promoted results
//!
//! The following partial outcomes MUST NOT be admitted to the
//! authoritative caches above. They produce request-local outputs only
//! and are discarded when the engine drops:
//!
//! - cancelled requests (cooperative cancellation),
//! - superseded results (a later request's input changed before this
//!   one published),
//! - interrupted results (panic / stack overflow / OS error caught by
//!   the cooperative-admission guard),
//! - budget-exceeded results (FuseBudgets tripped before the projection
//!   converged), and
//! - partial results (any path that returned `Opaque(Miss)` or an
//!   intentionally incomplete shape because a strict-legality precondition
//!   was not met).
//!
//! The engine's `post_publish` discipline enforces this: it commits
//! only when the cooperative-admission guard records a complete success.
//!
//! See the `/component-meta` skill for the public API surface and the
//! `/type-resolution` skill for the cross-file resolver query modes.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::hash::Hash;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_solver::query_engine::ProjectedMember;
use verter_type_expr::TypeExpr;

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::ResolverContext;
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::semantic_query::SemanticNodeId;

// Surface-projection helpers, prepared-substitution
// machinery, and arc cache-key constructors live in the private
// `surface` child module. The `pub(crate) use` block re-exports the
// existing public-API symbols so external `crate::resolver_core::component_meta_query_engine::<name>`
// paths remain stable.
mod helpers;
mod prepared_surface;
mod registry_decl;
mod route_keys;
mod routed_expr;
mod shallow_preserve;
mod surface;

pub(crate) use surface::{
    projected_surface_from_semantic_node, projected_surface_to_expanded_shape,
    projected_surface_to_type_expr, semantic_query_error_raw, surface_view_to_projected_surface,
    type_expr_contains_semantic_miss, type_expr_has_any_object_arm, type_expr_is_expanded_surface,
};

// Items needed inside this module (mod.rs) — engine impl methods and
// supporting code. All `pub(super)` in surface.rs.
#[cfg(test)]
use surface::type_expr_references_substitutions;
use surface::{
    apply_type_param_substitutions, build_default_type_param_substitutions,
    PreparedSurfaceProjection,
};

// Predicate/utility helpers (route-expr surface keys,
// package-canonical predicates, prepared-decl shape predicates,
// registry-symbol resolution with budget) live in the private
// `helpers` child module. All entries are `pub(super)` and used only
// from the engine impl in this file plus the inline test module.
use helpers::is_package_source;
#[cfg(test)]
use helpers::type_expr_references_type_params;

pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";

/// Build an R28 path-precise `Arc<[FactVersionRef]>` for a cache
/// whose validity depends on a single MEMBER of an exporter type.
/// Observes `MemberPresence(exporter, member)` and `Member(exporter,
/// member)` facts in the `Type` symbol space so the consumer
/// invalidates ONLY when the named member's header or body changes;
/// sibling-member edits in the same file keep the consumer warm.
pub(crate) fn engine_fact_signature_for_canonical_member(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    exporter: &str,
    member: &str,
) -> std::sync::Arc<[crate::resolver_core::FactVersionRef]> {
    crate::fact_signature_helpers::fact_signature_for_canonical_member(
        ctx,
        canonical_id,
        exporter,
        member,
        verter_semantic::facts::registry::SymbolSpace::Type,
    )
}

/// Build an R28 signature for a cache whose validity depends on the
/// IDENTITY of a top-level type at `(canonical, type_name)`. Observes
/// `Export(name)`, `LocalDecl(name)`, and `MemberShape(exporter=name)`
/// facts. The consumer invalidates when the type is added, removed,
/// renamed, or when its member shape changes; editing a single
/// member's body does NOT invalidate.
pub(crate) fn engine_fact_signature_for_exported_type(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
    type_name: &str,
) -> std::sync::Arc<[crate::resolver_core::FactVersionRef]> {
    crate::fact_signature_helpers::fact_signature_for_exported_type(
        ctx,
        canonical_id,
        type_name,
        verter_semantic::facts::registry::SymbolSpace::Type,
    )
}

/// Build an R28 path-precise `Arc<[FactVersionRef]>` for a cache
/// keyed on a scope canonical whose cold compute enumerates the
/// file's surface (e.g. expression projection over `scope_canonical`).
/// Observes `SyntacticExportSet` — adding/removing exports invalidates,
/// cosmetic edits do not.
pub(crate) fn engine_fact_signature_for_canonical_surface(
    ctx: &dyn ResolverContext,
    canonical_id: &str,
) -> std::sync::Arc<[crate::resolver_core::FactVersionRef]> {
    crate::fact_signature_helpers::fact_signature_for_canonical_surface(ctx, canonical_id)
}

/// Build the fact signature for a `PreparedTargetDb` entry.
///
/// A `PreparedTargetDb` entry maps `(active_scope, target_name)` to a
/// resolved `(canonical, symbol)` pair. The entry is keyed on BOTH the
/// active scope and the declaring canonical, so both are self-roots:
/// the resolved target depends on the top-level identity of
/// `target_name` in `active_scope` AND on the declaring file's own
/// decl identity. The signature observes the top-level-identity facts
/// for the target name in `active_scope`, and — when the declaring
/// canonical/symbol differs from the active-scope target — also for
/// the declaring `(decl_canonical, decl_symbol)`. A content edit to
/// either file shifts its self-root `FileWholeHash` and rejects the
/// entry.
pub(crate) fn engine_fact_signature_for_prepared_target(
    ctx: &dyn ResolverContext,
    active_scope: &str,
    target_name: &str,
    decl_canonical: &str,
    decl_symbol: &str,
) -> std::sync::Arc<[crate::resolver_core::FactVersionRef]> {
    let mut entries: Vec<crate::resolver_core::FactVersionRef> =
        engine_fact_signature_for_exported_type(ctx, active_scope, target_name).to_vec();
    if decl_canonical != active_scope || decl_symbol != target_name {
        entries.extend(
            engine_fact_signature_for_exported_type(ctx, decl_canonical, decl_symbol)
                .iter()
                .cloned(),
        );
    }
    std::sync::Arc::from(entries)
}

/// Build the fact signature for a `MaterializeMemoDb` entry.
///
/// A `MaterializeMemoDb` entry caches the materialised form of a type
/// expression in a `scope` canonical. The keyed `scope` is the entry's
/// self-root (the `SyntacticExportSet` surface signature already leads
/// with a current-content `FileWholeHash` for it). The materialised
/// value also depends on every canonical the materialisation walk
/// observed — `materialized_dep_signature` carries those — so each is
/// rooted as a cross-file dependency `FileWholeHash`: an edit to any
/// contributing file invalidates the memo.
///
/// Each observed canonical's dependency `FileWholeHash` is sourced
/// directly from the authoritative current-content oracle
/// ([`crate::fact_signature_helpers::current_content_whole_hash_fact`]),
/// independent of the canonical's [`crate::semantic_query::DepVersion`]
/// variant on `materialized_dep_signature`. A `DepVersion::RouteGeneration`
/// or `DepVersion::ProjectGeneration` entry has no `FileWholeHash` of
/// its own — the legacy `dep_signature_to_fact_signature` bridge drops
/// such entries entirely — but the canonical it names is still a
/// contributing file whose content edit must invalidate the memo, so
/// it is rooted here by its current whole hash rather than skipped.
pub(crate) fn engine_fact_signature_for_materialize_memo(
    ctx: &dyn ResolverContext,
    scope_canonical_id: &str,
    materialized_dep_signature: &crate::semantic_query::DepSignature,
) -> std::sync::Arc<[crate::resolver_core::FactVersionRef]> {
    let mut entries: Vec<crate::resolver_core::FactVersionRef> =
        engine_fact_signature_for_canonical_surface(ctx, scope_canonical_id).to_vec();
    // Root every observed canonical by its CURRENT-content whole hash —
    // regardless of the `DepVersion` variant recorded for it. Relying
    // on `dep_signature_to_fact_signature`'s `WholeHash`-only filter
    // would leave a canonical observed via a `RouteGeneration` /
    // `ProjectGeneration` dependency unrooted.
    for (observed_canonical, _dep_version) in materialized_dep_signature.iter() {
        if observed_canonical.as_ref() == scope_canonical_id {
            // The keyed scope is already self-rooted by the surface
            // signature above; do not double-root it.
            continue;
        }
        if let Some(fact) = crate::fact_signature_helpers::current_content_whole_hash_fact(
            ctx,
            observed_canonical.as_ref(),
        ) {
            entries.push(fact);
        }
    }
    std::sync::Arc::from(entries)
}

/// Build a two-canonical `DepSignature` (used for DB caches whose
/// validity depends on both an active scope and a declaration source).
#[allow(dead_code)]
pub(crate) fn engine_dep_signature_for_two_canonicals(
    ctx: &dyn ResolverContext,
    canonical_a: &str,
    canonical_b: &str,
) -> crate::semantic_query::DepSignature {
    let mut entries: Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let push = |entries: &mut Vec<_>, c: &str| {
        let whole_hash = ctx
            .shallow_file_state(c)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        entries.push((
            std::sync::Arc::<str>::from(c),
            crate::semantic_query::DepVersion::WholeHash(whole_hash),
        ));
    };
    push(&mut entries, canonical_a);
    if canonical_b != canonical_a {
        push(&mut entries, canonical_b);
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    std::sync::Arc::from(entries.into_boxed_slice())
}

#[cfg(test)]
use std::cell::Cell;

/// Path C C11b — composite-scope context for prepared-member-path
/// projection. Bundles the two scopes the prepared-route walker keeps
/// live:
///
/// - `decl_scope`: the canonical id of the file where the prepared
///   declaration (e.g., `type Button = ComponentConfig<typeof theme>`)
///   was originally defined. Helper-body-internal refs (like the inner
///   `ComponentUI` in `ComponentConfig`'s body) resolve against this
///   scope because that's where the helper imports are visible.
/// - `arg_scope`: the canonical id of the caller — the file that
///   instantiated the prepared decl. `typeof value_ref` references and
///   type arguments passed at the call site resolve in this scope.
///
/// See [`ComponentMetaQueryEngine::solve_or_project_leaf_expr_with_context`]
/// for the per-TypeExpr dispatch rules.
//
// No longer constructed after
// trampoline conversion of the retired surface methods. Deleted in
// 5g per §F call-graph closure.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct PreparedProjectionContext {
    decl_scope: String,
    arg_scope: String,
    /// Path C C11-residual-B: scopes from outer levels of a
    /// declaration-chain projection. Populated as the recursion
    /// descends from `project_prepared_member_path_route_projection_from_*`
    /// so a `TypeOf(value)` reference inside an inner helper body
    /// (e.g., the lowered `ComponentUI<typeof theme>` inside
    /// `ComponentConfig`'s body, where the original `Button` alias
    /// lives in `button-types.ts`) can fall back through the chain
    /// to find the scope where the value symbol was actually visible.
    ///
    /// Innermost-first ordering: `chain_scopes[0]` is the scope of the
    /// most recently entered declaration. Deduplicated against
    /// `decl_scope` and `arg_scope` at lookup time.
    chain_scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedImportedRegistrySymbol {
    pub canonical_id: String,
    pub exported_name: String,
    pub body: TypeExpr,
    pub canonical_dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum PreparedSubstitutionKey {
    Empty,
    Entries(Vec<(String, TypeExpr)>),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedSurfaceCacheKey {
    canonical_id: String,
    symbol_name: String,
    substitutions: PreparedSubstitutionKey,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedMemberCacheKey {
    canonical_id: String,
    symbol_name: String,
    member_name: String,
    kind: PreparedMemberCacheKind,
    substitutions: PreparedSubstitutionKey,
}

// `InheritedRoute` is no longer
// constructed after trampoline conversion. Variant
// §F call-graph closure.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum PreparedMemberCacheKind {
    Requested,
    #[allow(dead_code)]
    InheritedRoute,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedTargetCacheKey {
    active_scope_canonical_id: String,
    decl_canonical_id: String,
    decl_symbol_name: String,
    requested_name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct RoutedExprSurfaceCacheKey {
    scope_canonical_id: String,
    root_symbol: String,
    route: super::RouteDemand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FastShallowFieldExprExactness {
    Symbolic,
    Concrete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FastShallowFieldExpr {
    pub expr: TypeExpr,
    pub exactness: FastShallowFieldExprExactness,
}

/// Query-local component-meta solve engine.
///
/// Declaration-scoped lookups resolve through dispatch via
/// [`project_type_surface_expr`]. retired the
/// request-scoped owner engine bridge; all solve-like operations now
/// route through `ProjectSemanticDispatch`. Imported registry entries
/// memoize by declaration scope so the same textual reference does not
/// alias across files.
///
/// **Engine-local cache audit.**
///
/// The plan's binary partition (a = request-local non-semantic scratch,
/// b = reusable semantic producer cache subsumed by dispatch) classifies
/// each field as follows. Fields marked **(a)** are scratch state and
/// retained; fields marked **(b)** are pre-lowering-level memos that
/// genuinely complement dispatch's post-lowering memo (the two operate
/// on different identity spaces — `TypeExpr` vs. `SemanticNodeId` — so
/// dispatch cannot subsume them). The CLAUDE.md "ctx-owned cache
/// principle" violation (these are `FxHashMap` rather than DashMap-backed
/// ctx caches) is documented architectural debt distinct from the
/// dispatch-routing scope of this commit; migrating the (b) entries to
/// ctx-owned `DashMap`s is its own follow-up plan.
///
/// | Field | Class | Rationale |
/// |---|---|---|
/// | `ctx` | (a) | Borrowed runtime reference, not a cache. |
/// | `current_prepared_request_root` | (a) | Call-scoped recursion-guard. |
/// | `imported_registry_symbols` | (b) | Caches `(canonical, name) → ResolvedImportedRegistrySymbol` at TypeExpr level. Dispatch's `ResolveDecl` memo operates on `SemanticNodeId`s; cannot subsume the pre-lowering identity. |
/// | `declarations` / `resolvable` / `owner_collection_exprs` | (b) | Same kind — pre-lowering memos keyed on `(canonical, name)` strings. |
/// | `scope_payloads` | (a) | Per-request `Arc<DeclarationScopePayload>` clones; the bundle is ctx-owned, this just reuses the Arc within one request. |
/// | `prepared_surface_cache` / `prepared_member_cache` / `prepared_target_cache` / `routed_expr_surface_cache` | (b) | All four are pre-lowering route projections — same justification as above. |
/// | `prepared_type_decls` | (a) | Arc-cache for `Arc<PreparedTypeDecl>` from ctx; no semantic computation — only refcount avoidance. |
/// | `materialize_memo` | (b) | — `(scope, expr, navigate_flag) → MaterializedTypeExpr` memo. Dispatch's post-lowering memo cannot replace this because the key is the un-lowered `TypeExpr`. |
/// | `prepared_*_query_count`, `prepared_*_hit_count` | (a) | `#[cfg(test)]` instrumentation counters. |
/// | `fuse_budgets` / `fuse_state` | (a) | Engine-construction-scoped fuse rails (§1.4). |
/// | `projection_chain_scopes` | (a) | Call-scoped scope chain (Path C C11-residual-B). |
///
/// **Audit conclusion:** all (b) producer caches operate at the
/// pre-lowering `TypeExpr` identity space, which dispatch's
/// `SemanticNodeId`-keyed memo cannot subsume. They are NOT dual-path
/// duplicates of dispatch's work; they are a complementary memoization
/// layer. The plan's "delete (b) fields" directive applies only when
/// dispatch can replace the work — for these fields it cannot. The
/// (b) → ctx-owned migration is documented architectural debt
/// (CLAUDE.md ctx-owned cache principle) addressed in a separate
/// follow-up plan.
pub struct ComponentMetaQueryEngine<'a> {
    pub(crate) ctx: &'a dyn ResolverContext,
    current_prepared_request_root: Option<String>,
    // The 10 caches below are read-through views over the host-owned
    // typed DBs on `ProjectTypeStore` (see `crate::component_meta_caches`).
    // Each engine field is a per-request **non-authoritative read-through
    // view** that mirrors the ctx DB result for repeated lookups within
    // one request. `RefCell` provides interior mutability so `&self`
    // lookups can populate the view after a ctx DB hit. NO independent
    // invalidation, NO
    // independent dep_signature, NO entries the ctx DB doesn't have.
    imported_registry_symbols:
        RefCell<FxHashMap<(String, String), Option<ResolvedImportedRegistrySymbol>>>,
    /// Cached type declarations (read-through view; authority is
    /// `ProjectTypeStore::declaration_db()`).
    declarations: RefCell<FxHashMap<(String, String), ResolvedTypeDeclaration>>,
    /// Cached resolvability checks (read-through view; authority is
    /// `ProjectTypeStore::resolvable_db()`).
    resolvable: RefCell<FxHashMap<(String, String), bool>>,
    /// Cached owner collection expressions (read-through view;
    /// authority is `ProjectTypeStore::owner_collection_db()`).
    owner_collection_exprs: RefCell<FxHashMap<String, Option<verter_type_expr::TypeExpr>>>,
    /// Request-local cache of declaration-scope payloads per scope canonical id.
    /// The prepared bundle stays authoritative; this cache only reuses the
    /// bundle-derived names/bindings within one request so repeated projections
    /// do not keep recloning them.
    scope_payloads: FxHashMap<String, Option<std::sync::Arc<DeclarationScopePayload>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_surface_db()`.
    ///
    /// Unread after trampoline
    /// conversion of retired surface methods. Field
    /// §F call-graph closure.
    #[allow(dead_code)]
    prepared_surface_cache: RefCell<FxHashMap<PreparedSurfaceCacheKey, PreparedSurfaceProjection>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_member_db()`.
    prepared_member_cache: RefCell<FxHashMap<PreparedMemberCacheKey, Option<ProjectedMember>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_target_db()`.
    prepared_target_cache: RefCell<FxHashMap<PreparedTargetCacheKey, Option<(String, String)>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::routed_expr_surface_db()`.
    ///
    /// Unread after trampoline
    /// conversion of retired surface methods. Field
    /// §F call-graph closure.
    #[allow(dead_code)]
    routed_expr_surface_cache: RefCell<FxHashMap<RoutedExprSurfaceCacheKey, TypeExpr>>,
    /// Request-local memoization for prepared declaration lookups.
    prepared_type_decls: FxHashMap<
        (String, String),
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    >,
    /// Read-through view; authority is
    /// `ProjectTypeStore::materialize_memo_db()`.
    pub(crate) materialize_memo: RefCell<
        FxHashMap<
            (String, verter_type_expr::TypeExpr, bool),
            crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
        >,
    >,
    #[cfg(test)]
    prepared_type_decl_query_count: usize,
    #[cfg(test)]
    prepared_root_surface_projection_count: usize,
    #[cfg(test)]
    #[allow(dead_code)]
    prepared_shared_surface_hit_count: usize,
    #[cfg(test)]
    #[allow(dead_code)]
    prepared_shared_member_hit_count: usize,
    fuse_budgets: FuseBudgets,
    fuse_state: FuseState,
    /// Path C C11-residual-B: ambient declaration-scope chain accumulated
    /// during prepared-member-path projection recursion. Innermost entry
    /// at index 0; outermost (originating call's `decl_scope`) at the
    /// end. Used by `solve_or_project_leaf_expr_with_context` to find the
    /// scope where a `TypeOf(value)` reference is visible when neither
    /// `decl_scope` (the current declaration owner) nor `arg_scope` (the
    /// caller's SFC) contains the value symbol.
    ///
    /// Unread after trampoline
    /// conversion. Field.
    #[allow(dead_code)]
    projection_chain_scopes: Vec<String>,
}

#[cfg(test)]
thread_local! {
    static FORBID_STRUCTURAL_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
    static FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
    static FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) struct StructuralSlowLaneGuard;

#[cfg(test)]
impl Drop for StructuralSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_STRUCTURAL_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_structural_slow_lane_for_tests() -> StructuralSlowLaneGuard {
    FORBID_STRUCTURAL_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    StructuralSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct DirectPickRoutedExprSlowLaneGuard;

#[cfg(test)]
impl Drop for DirectPickRoutedExprSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_direct_pick_routed_expr_slow_lane_for_tests(
) -> DirectPickRoutedExprSlowLaneGuard {
    FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    DirectPickRoutedExprSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct PreparedStructuralSubstitutionSlowLaneGuard;

#[cfg(test)]
impl Drop for PreparedStructuralSubstitutionSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| {
            depth.set(depth.get().saturating_sub(1));
        });
    }
}

#[cfg(test)]
pub(crate) fn forbid_prepared_structural_substitution_slow_lane_for_tests(
) -> PreparedStructuralSubstitutionSlowLaneGuard {
    FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| {
        depth.set(depth.get().saturating_add(1));
    });
    PreparedStructuralSubstitutionSlowLaneGuard
}

// Unused after trampoline
// conversion of `project_route_surface_expr`. Helper.
#[cfg(test)]
#[allow(dead_code)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {
    assert!(
        !direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
        "direct routed-expr pick slow lane should not be used when member projection can satisfy the route",
    );
}

#[cfg(not(test))]
#[allow(dead_code)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {}

#[cfg(test)]
fn assert_prepared_structural_substitution_slow_lane_allowed(expr: &TypeExpr) {
    let is_structural = matches!(
        expr,
        TypeExpr::Object(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Union(_)
            | TypeExpr::Function(_)
            | TypeExpr::Parenthesized(_),
    );
    if is_structural {
        assert!(
            !prepared_structural_substitution_slow_lane_forbidden_for_current_thread(),
            "prepared generic projection should not whole-substitute structural bodies when shallow member-local substitution can satisfy the route",
        );
    }
}

#[cfg(test)]
pub(crate) fn structural_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_STRUCTURAL_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(test)]
pub(crate) fn direct_pick_routed_expr_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(test)]
pub(crate) fn prepared_structural_substitution_slow_lane_forbidden_for_current_thread() -> bool {
    FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.with(|depth| depth.get() > 0)
}

#[cfg(not(test))]
fn assert_prepared_structural_substitution_slow_lane_allowed(_expr: &TypeExpr) {}

impl<'a> ComponentMetaQueryEngine<'a> {
    pub(crate) fn new(ctx: &'a dyn ResolverContext) -> Self {
        Self {
            ctx,
            current_prepared_request_root: None,
            imported_registry_symbols: RefCell::new(FxHashMap::default()),
            declarations: RefCell::new(FxHashMap::default()),
            resolvable: RefCell::new(FxHashMap::default()),
            owner_collection_exprs: RefCell::new(FxHashMap::default()),
            scope_payloads: FxHashMap::default(),
            prepared_surface_cache: RefCell::new(FxHashMap::default()),
            prepared_member_cache: RefCell::new(FxHashMap::default()),
            prepared_target_cache: RefCell::new(FxHashMap::default()),
            routed_expr_surface_cache: RefCell::new(FxHashMap::default()),
            prepared_type_decls: FxHashMap::default(),
            materialize_memo: RefCell::new(FxHashMap::with_capacity_and_hasher(
                64,
                Default::default(),
            )),
            #[cfg(test)]
            prepared_type_decl_query_count: 0,
            #[cfg(test)]
            prepared_root_surface_projection_count: 0,
            #[cfg(test)]
            prepared_shared_surface_hit_count: 0,
            #[cfg(test)]
            prepared_shared_member_hit_count: 0,
            fuse_budgets: FuseBudgets::default(),
            fuse_state: FuseState::default(),
            projection_chain_scopes: Vec::new(),
        }
    }

    /// Returns the cached [`DeclarationScopePayload`] for
    /// `scope_canonical_id`, lazily loading the underlying
    /// `prepared_decl_bundle` on first access ( D35:
    /// promoted to `pub(crate)` so the session-layer materialize wrapper
    /// in `meta_resolve.rs` can reuse the cache without re-walking the
    /// bundle).
    pub(crate) fn scope_payload_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
        let ctx = self.ctx;
        self.scope_payloads
            .entry(scope_canonical_id.to_string())
            .or_insert_with(|| {
                ctx.prepared_decl_bundle(scope_canonical_id)
                    .or_else(|| {
                        // Lazy first-time loading for dependency files discovered
                        // during resolution. This is NOT re-walking cached state —
                        // it triggers the normal load/parse/cache pipeline for files
                        // not yet in the ctx's cache.
                        ctx.ensure_loaded(scope_canonical_id)
                            .then(|| ctx.prepared_decl_bundle(scope_canonical_id))
                            .flatten()
                    })
                    .map(|bundle| {
                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle))
                    })
            })
            .clone()
    }
}

fn local_type_symbol_metadata_for_known_source(
    ctx: &dyn ResolverContext,
    canonical_source: &str,
    resolved_name: &str,
) -> Option<ResolvedLocalTypeSymbolMetadata> {
    let analysis = ctx.external_type_analysis(canonical_source)?;
    let symbol = analysis.local_type_symbol(resolved_name)?;
    let kind = match symbol.kind {
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::TypeAlias => {
            ResolvedDeclarationKind::TypeAlias
        }
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Interface => {
            ResolvedDeclarationKind::Interface
        }
        verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSymbolKind::Class => {
            ResolvedDeclarationKind::Class
        }
    };
    Some(ResolvedLocalTypeSymbolMetadata {
        kind,
        span: symbol.span,
    })
}

struct DirectPreparedDeclarationResolver<'a> {
    ctx: &'a dyn ResolverContext,
}

impl DeclarationMetadataResolver for DirectPreparedDeclarationResolver<'_> {
    fn resolve_export_target(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<super::declaration_metadata::ResolvedExportTarget> {
        None
    }

    fn get_export_span_follow_reexports(
        &self,
        _dep_canonical: &str,
        _requested_name: &str,
    ) -> Option<verter_span::Span> {
        None
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        self.ctx
            .local_type_declaration_id(canonical_source, resolved_name)
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<super::declaration_metadata::ResolvedLocalTypeSymbolMetadata> {
        local_type_symbol_metadata_for_known_source(self.ctx, canonical_source, resolved_name)
    }
}

fn empty_semantic_args() -> std::sync::Arc<[SemanticNodeId]> {
    std::sync::Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
}

/// Engine-internal helper that mirrors the deprecated
/// `project_type_member` entry: dispatch the single-member projection,
/// falling back to the prepared-decl walker when dispatch misses.
/// Used by `project_routed_expr_surface_expr` and friends after the
/// deprecated engine method's deletion.
fn dispatch_member_for_root_symbol(
    engine: &mut ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    symbol_name: &str,
    member_name: &str,
) -> Option<ProjectedMember> {
    if engine.projection_op_budget_exhausted() {
        return None;
    }
    engine
        .dispatch_projected_member(scope_canonical_id, symbol_name, member_name)
        .or_else(|| {
            let mut active = FxHashSet::default();
            engine.project_prepared_requested_member_from_symbol(
                scope_canonical_id,
                symbol_name,
                member_name,
                &FxHashMap::default(),
                &mut active,
            )
        })
}

/// Engine-internal substitution helper that mirrors the
/// deleted `instantiate_local_generic_ref` engine method body. Unlike
/// the dispatch-only `instantiate_local_generic_ref_via_dispatch`, this
/// helper walks the re-export chain via
/// `resolve_final_prepared_type_target` before looking up the prepared
/// decl — preserving the cross-file type-alias substitution semantics
/// the engine method's call sites depended on.
fn instantiate_local_generic_ref_via_engine(
    engine: &mut ComponentMetaQueryEngine<'_>,
    scope_canonical_id: &str,
    expr: &TypeExpr,
) -> Option<TypeExpr> {
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = expr
    else {
        return None;
    };
    if type_arguments.is_empty() {
        return None;
    }

    let declaration = engine.resolve_type_declaration(scope_canonical_id, name.as_ref());
    let declared_canonical_id = if declaration.canonical_source.is_empty() {
        scope_canonical_id.to_string()
    } else {
        declaration.canonical_source.clone()
    };
    let declared_symbol_name = if declaration.resolved_name.is_empty() {
        name.as_ref().to_string()
    } else {
        declaration.resolved_name.clone()
    };
    let (target_canonical_id, target_symbol_name) = engine.resolve_final_prepared_type_target(
        declared_canonical_id.as_str(),
        declared_symbol_name.as_str(),
    );
    if is_package_source(engine.ctx, Some(target_canonical_id.as_str())) {
        return None;
    }
    let prepared = engine.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
    let substitutions = build_default_type_param_substitutions(prepared.as_ref(), type_arguments)?;
    Some(apply_type_param_substitutions(
        &prepared.body,
        &substitutions,
    ))
}

#[cfg(test)]
mod tests;
