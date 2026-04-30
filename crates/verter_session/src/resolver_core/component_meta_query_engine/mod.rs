//! Declaration-aware component-meta query engine.
//!
//! `ComponentMetaQueryEngine` is a request-scoped cache bag for one
//! `get_component_meta()` request. It owns per-request projection caches
//! and resolves type declarations lazily from the host's prepared-decl
//! bundles. All solve-like operations dispatch through
//! [`ProjectSemanticDispatch`]; D-Cutover §5.8 WIP-W retired the
//! previously embedded `TypeQueryEngine` + `TypeSolverHost` bridge.
//!
//! The per-scope caches provide query-local memoization to avoid
//! re-projecting the same imported type reference within one request.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::hash::Hash;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedMember, ProjectedSurface};

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::semantic_query::SemanticNodeId;
use crate::VerterHost;

// Phase 11b.2 — surface-projection helpers, prepared-substitution
// machinery, and arc cache-key constructors live in the private
// `surface` child module. The `pub(crate) use` block re-exports the
// existing public-API symbols so external `crate::resolver_core::component_meta_query_engine::<name>`
// paths remain stable.
mod helpers;
mod prepared_surface;
mod registry_decl;
mod shallow_preserve;
mod surface;

pub(crate) use surface::{
    arc_prepared_member_cache_key, arc_routed_expr_surface_cache_key,
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
    substitute_function_expr_if_needed, substituted_ref_expr_if_needed, PreparedSurfaceProjection,
};

// Phase 11b.3 — predicate/utility helpers (route-expr surface keys,
// package-canonical predicates, prepared-decl shape predicates,
// registry-symbol resolution with budget) live in the private
// `helpers` child module. All entries are `pub(super)` and used only
// from the engine impl in this file plus the inline test module.
use helpers::{
    is_builtin_name, is_package_source, prepared_decl_keeps_raw_symbolic_non_object_alias,
    prepared_member_body_stays_shallow, projected_surface_member_names,
    string_literal_keys_type_expr, strip_parens_expr, type_expr_references_type_params,
};

pub(crate) const SEMANTIC_MISS: &str = "semanticMiss";
pub(crate) const SEMANTIC_OBJECT_SURFACE: &str = "semanticObjectSurface";
pub(crate) const SEMANTIC_SURFACE_MEMBER: &str = "semanticSurfaceMember";

/// Build a single-fact `DepSignature` for a canonical's current
/// `whole_hash`. Used by Step 3 closure's host-DB read-through call
/// sites — each cache entry's dep_signature mirrors the canonical(s)
/// the entry depends on so [`HostFenceValidator`](crate::host_manage::HostFenceValidator)
/// can revalidate it on warm hit and post-compute.
pub(crate) fn engine_dep_signature_for_canonical(
    host: &VerterHost,
    canonical_id: &str,
) -> crate::semantic_query::DepSignature {
    let whole_hash = host
        .shallow_file_state(canonical_id)
        .map(|state| state.whole_hash)
        .unwrap_or_default();
    let entries = vec![(
        std::sync::Arc::<str>::from(canonical_id),
        crate::semantic_query::DepVersion::WholeHash(whole_hash),
    )];
    std::sync::Arc::from(entries.into_boxed_slice())
}

/// Build a two-canonical `DepSignature` (used for DB caches whose
/// validity depends on both an active scope and a declaration source).
#[allow(dead_code)]
pub(crate) fn engine_dep_signature_for_two_canonicals(
    host: &VerterHost,
    canonical_a: &str,
    canonical_b: &str,
) -> crate::semantic_query::DepSignature {
    let mut entries: Vec<(std::sync::Arc<str>, crate::semantic_query::DepVersion)> = Vec::new();
    let push = |entries: &mut Vec<_>, c: &str| {
        let whole_hash = host
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
// Phase 5c (sub-plan §5 commit 3.7): no longer constructed after
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

// Phase 5c (sub-plan §5 commit 3.7): `InheritedRoute` is no longer
// constructed after trampoline conversion. Variant deleted in 5g per
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
/// [`project_type_surface_expr`]. D-Cutover §5.8 WIP-W retired the
/// request-scoped owner engine bridge; all solve-like operations now
/// route through `ProjectSemanticDispatch`. Imported registry entries
/// memoize by declaration scope so the same textual reference does not
/// alias across files.
///
/// **Engine-local cache audit (plan §3 Step 6.4 / D37).**
///
/// The plan's binary partition (a = request-local non-semantic scratch,
/// b = reusable semantic producer cache subsumed by dispatch) classifies
/// each field as follows. Fields marked **(a)** are scratch state and
/// retained; fields marked **(b)** are pre-lowering-level memos that
/// genuinely complement dispatch's post-lowering memo (the two operate
/// on different identity spaces — `TypeExpr` vs. `SemanticNodeId` — so
/// dispatch cannot subsume them). The CLAUDE.md "host-owned cache
/// principle" violation (these are `FxHashMap` rather than DashMap-backed
/// host caches) is documented architectural debt distinct from the
/// dispatch-routing scope of this commit; migrating the (b) entries to
/// host-owned `DashMap`s is its own follow-up plan.
///
/// | Field | Class | Rationale |
/// |---|---|---|
/// | `host` | (a) | Borrowed runtime reference, not a cache. |
/// | `current_prepared_request_root` | (a) | Call-scoped recursion-guard. |
/// | `imported_registry_symbols` | (b) | Caches `(canonical, name) → ResolvedImportedRegistrySymbol` at TypeExpr level. Dispatch's `ResolveDecl` memo operates on `SemanticNodeId`s; cannot subsume the pre-lowering identity. |
/// | `declarations` / `resolvable` / `owner_collection_exprs` | (b) | Same kind — pre-lowering memos keyed on `(canonical, name)` strings. |
/// | `scope_payloads` | (a) | Per-request `Arc<DeclarationScopePayload>` clones; the bundle is host-owned, this just reuses the Arc within one request. |
/// | `prepared_surface_cache` / `prepared_member_cache` / `prepared_target_cache` / `routed_expr_surface_cache` | (b) | All four are pre-lowering route projections — same justification as above. |
/// | `prepared_type_decls` | (a) | Arc-cache for `Arc<PreparedTypeDecl>` from host; no semantic computation — only refcount avoidance. |
/// | `materialize_memo` | (b) | Plan §3 Step 6.3 — `(scope, expr, navigate_flag) → MaterializedTypeExpr` memo. Dispatch's post-lowering memo cannot replace this because the key is the un-lowered `TypeExpr`. |
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
/// (b) → host-owned migration is documented architectural debt
/// (CLAUDE.md host-owned cache principle) addressed in a separate
/// follow-up plan.
pub struct ComponentMetaQueryEngine<'a> {
    pub(crate) host: &'a VerterHost,
    current_prepared_request_root: Option<String>,
    // Step 3 closure (architectural-debt-closure rev 10) — the 10 caches
    // below were authoritative `FxHashMap` storage prior to this commit.
    // Authority moves to host-owned typed DBs on
    // `ProjectTypeStore` (see `crate::component_meta_caches`); each
    // engine field below is a per-request **non-authoritative
    // read-through view** that mirrors the host DB result for repeated
    // lookups within one request. `RefCell` provides interior
    // mutability so `&self` lookups can populate the view after a host
    // DB hit. Per the D3.2 contract: NO independent invalidation, NO
    // independent dep_signature, NO entries the host DB doesn't have.
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
    owner_collection_exprs:
        RefCell<FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>>,
    /// Request-local cache of declaration-scope payloads per scope canonical id.
    /// The prepared bundle stays authoritative; this cache only reuses the
    /// bundle-derived names/bindings within one request so repeated projections
    /// do not keep recloning them.
    scope_payloads: FxHashMap<String, Option<std::sync::Arc<DeclarationScopePayload>>>,
    /// Read-through view; authority is
    /// `ProjectTypeStore::prepared_surface_db()`.
    ///
    /// Phase 5c (sub-plan §5 commit 3.7): unread after trampoline
    /// conversion of retired surface methods. Field deleted in 5g per
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
    /// Phase 5c (sub-plan §5 commit 3.7): unread after trampoline
    /// conversion of retired surface methods. Field deleted in 5g per
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
            (String, verter_semantic::analysis::type_expr::TypeExpr, bool),
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
    /// Phase 5c (sub-plan §5 commit 3.7): unread after trampoline
    /// conversion. Field deleted in 5g per §F call-graph closure.
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

// Phase 5c (sub-plan §5 commit 3.7): unused after trampoline
// conversion of `project_route_surface_expr`. Helper deleted in 5g.
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
    pub fn new(host: &'a VerterHost) -> Self {
        Self {
            host,
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
    /// `prepared_decl_bundle` on first access (plan §3 Step 6.3 D35:
    /// promoted to `pub(crate)` so the session-layer materialize wrapper
    /// in `meta_resolve.rs` can reuse the cache without re-walking the
    /// bundle).
    pub(crate) fn scope_payload_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
        let host = self.host;
        self.scope_payloads
            .entry(scope_canonical_id.to_string())
            .or_insert_with(|| {
                host.prepared_decl_bundle(scope_canonical_id)
                    .or_else(|| {
                        // Lazy first-time loading for dependency files discovered
                        // during resolution. This is NOT re-walking cached state —
                        // it triggers the normal load/parse/cache pipeline for files
                        // not yet in the host's cache.
                        host.ensure_loaded(scope_canonical_id)
                            .then(|| host.prepared_decl_bundle(scope_canonical_id))
                            .flatten()
                    })
                    .map(|bundle| {
                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle))
                    })
            })
            .clone()
    }
    fn prepared_string_literal_keys(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::with_capacity(types.len());
                for ty in types.iter() {
                    keys.extend(self.prepared_string_literal_keys(
                        scope_canonical_id,
                        prepared,
                        ty,
                        active,
                    )?);
                }
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => {
                self.prepared_string_literal_keys(scope_canonical_id, prepared, inner, active)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                let (target_canonical_id, target_symbol_name) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)?;
                let visit_key = (target_canonical_id.clone(), target_symbol_name.clone());
                if !active.insert(visit_key.clone()) {
                    return None;
                }
                let resolved = self
                    .prepared_type_decl(&target_canonical_id, &target_symbol_name)
                    .and_then(|target_prepared| {
                        self.prepared_string_literal_keys(
                            &target_canonical_id,
                            target_prepared.as_ref(),
                            &target_prepared.body,
                            active,
                        )
                    });
                active.remove(&visit_key);
                resolved
            }
            _ => None,
        }
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn enumerate_route_literal_keys(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<Vec<String>> {
        self.enumerate_route_literal_keys_inner(
            resolution_scope_canonical_id,
            active_scope_canonical_id,
            expr,
            0,
        )
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn enumerate_route_literal_keys_inner(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

        if depth >= 4 {
            return None;
        }

        match expr {
            TypeExpr::Literal(LiteralValue::String(value)) => Some(vec![value.clone()]),
            TypeExpr::Union(types) => {
                let mut keys = Vec::new();
                for ty in types.iter() {
                    keys.extend(self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        ty,
                        depth + 1,
                    )?);
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Parenthesized(inner) => self.enumerate_route_literal_keys_inner(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                depth + 1,
            ),
            TypeExpr::KeyOf(inner) => {
                if let TypeExpr::IndexedAccess { object, index } = inner.as_ref() {
                    if let TypeExpr::Literal(LiteralValue::String(member_name)) = index.as_ref() {
                        if let Some(keys) = self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            object,
                            member_name,
                            depth + 1,
                        ) {
                            return Some(keys);
                        }
                    }
                }

                if let Some(projected_expr) = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        expr,
                    )
                    .filter(|projected| projected != expr)
                {
                    return self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected_expr,
                        depth + 1,
                    );
                }

                let projected_inner = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        inner,
                    )
                    .unwrap_or_else(|| inner.as_ref().clone());
                if let Some(keys) = projected_surface_member_names(&projected_inner) {
                    return Some(keys);
                }

                match &projected_inner {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        // Path C C11-residual-C: accumulate enumerable
                        // arms only. Pre-residual-C the `?` propagation
                        // dropped the entire result when any single arm
                        // could not enumerate keys, even when other arms
                        // had concrete keyspaces. Mirrors the
                        // SemanticNode-level Intersection accumulation
                        // change in `key_names_from_base_node` (Path C
                        // C10) — `keyof (A & B)` returns the union of
                        // enumerable keys across A and B and only fails
                        // when EVERY arm is unresolvable.
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for part in parts.iter() {
                            if let Some(arm_keys) = self.enumerate_route_literal_keys_inner(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &TypeExpr::KeyOf(std::sync::Arc::new(part.clone())),
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(arm_keys);
                            }
                        }
                        if !any_enumerable {
                            return None;
                        }
                        keys.sort();
                        keys.dedup();
                        Some(keys)
                    }
                    _ => None,
                }
            }
            _ => {
                let projected = self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    expr,
                )?;
                if projected == *expr {
                    crate::resolver_core::component_meta_registry::component_meta_registry_string_literal_keys(
                        &projected,
                    )
                } else {
                    self.enumerate_route_literal_keys_inner(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &projected,
                        depth + 1,
                    )
                }
            }
        }
    }

    /// **Step 2 deletion target.** Plan §3 Step 6.4 requires deletion
    /// of this walker — the architectural target is `PathWalker`
    /// (in `project_semantic_dispatch/walk.rs`) as the only path-precise
    /// walker. The Step 11 tombstone command
    /// `! grep -rn "enumerate_member_surface_keys_via_route" crates/ packages/ scripts/`
    /// must return 0 hits.
    ///
    /// **Status (post Step 1.5).** The Step 1.5 dispatch-substitution
    /// parity work (Pick<X, K>['member'], mapped+conditional infer P,
    /// Method-as-Function lowering) closed the substitution-parity gap
    /// that previously blocked this walker's deletion. The walker
    /// remains in service of legacy member-route resolution and
    /// projection-rescue helpers (`expr_needs_projection_rescue`,
    /// `compare_type_expr_improvement`,
    /// `select_imported_materialization_scope`, and the cycle-detection
    /// migration helper `lowered_root_reaches_transitive_cycle`) that
    /// Step 2's caller-class parity matrix is responsible for migrating.
    /// Once those callers retire, this walker and its 13 internal call
    /// sites can ALL be deleted in the same commit (per CLAUDE.md
    /// "Legacy Code Deletion" — no shims).
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn enumerate_member_surface_keys_via_route(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
        member_name: &str,
        depth: usize,
    ) -> Option<Vec<String>> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        // Path C C11-residual-C: depth bumped from 4 to 8 to allow
        // multi-step navigation chains like
        // `(typeof theme & GetComponentAppConfig<AppConfig, "ui", "button">)['variants']['color']`
        // which require: (1) IndexedAccess(Intersection,..) distribute
        // → (2) IndexedAccess(Ref,..) expand alias → (3)
        // IndexedAccess(Conditional,..) distribute → (4)
        // IndexedAccess(IndexedAccess,..) recurse on inner → ...
        if depth >= 8 {
            return None;
        }

        let projected_expr = self
            .solve_or_project_prepared_member_leaf_expr(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                expr,
            )
            .unwrap_or_else(|| expr.clone());
        if matches!(projected_expr, TypeExpr::Unknown { .. }) {
            // Phase 5l: preserve the re-export chain walk that the
            // deleted `instantiate_local_generic_ref` engine method
            // performed via `resolve_final_prepared_type_target`.
            if let Some(expanded) =
                instantiate_local_generic_ref_via_engine(self, resolution_scope_canonical_id, expr)
                    .or_else(|| {
                        instantiate_local_generic_ref_via_engine(
                            self,
                            active_scope_canonical_id,
                            expr,
                        )
                    })
                    .filter(|expanded| expanded != expr)
            {
                return self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &expanded,
                    member_name,
                    depth + 1,
                );
            }
        }

        match &projected_expr {
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == member_name => {
                        Some(property.ty.clone())
                    }
                    ObjectMember::Method(method) if method.name == member_name => Some(
                        TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                    ),
                    _ => None,
                })?;
                let projected_member = self
                    .solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    )
                    .unwrap_or(member_ty);
                projected_surface_member_names(&projected_member)
            }
            TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                // Path C C11-residual-C: accumulate enumerable arms
                // only — see the matching change in
                // `enumerate_route_literal_keys_inner`. `keyof
                // (typeof theme & GetComponentAppConfig<...>)['variants']
                // ['color']` must merge `theme.variants.color`'s keys
                // with the conditional's resolvable arm keys, even when
                // the deferred conditional arm couldn't enumerate.
                let mut keys = Vec::new();
                let mut any_enumerable = false;
                for part in parts.iter() {
                    if let Some(arm_keys) = self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        part,
                        member_name,
                        depth + 1,
                    ) {
                        any_enumerable = true;
                        keys.extend(arm_keys);
                    }
                }
                if !any_enumerable {
                    return None;
                }
                keys.sort();
                keys.dedup();
                Some(keys)
            }
            TypeExpr::Conditional {
                true_type,
                false_type,
                ..
            } => {
                let mut keys = Vec::new();
                if let Some(true_keys) = self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    true_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(true_keys);
                }
                if let Some(false_keys) = self.enumerate_member_surface_keys_via_route(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    false_type,
                    member_name,
                    depth + 1,
                ) {
                    keys.extend(false_keys);
                }
                if keys.is_empty() {
                    None
                } else {
                    keys.sort();
                    keys.dedup();
                    Some(keys)
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                // D-Cutover §5.8: resolve the value root via the
                // dispatch-aligned bare-name resolver + host
                // prepared_value_decl directly. Mirrors `build_typeof`.
                let scope_payload = self.scope_payload_for_scope(active_scope_canonical_id);
                let root_name = value_ref.path.first()?;
                let root_identity =
                    crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                        self.host,
                        active_scope_canonical_id,
                        scope_payload.as_deref(),
                        root_name,
                    )?;
                let prepared_value = self
                    .host
                    .prepared_value_decl(&root_identity.canonical_id, &root_identity.symbol_name)
                    .or_else(|| {
                        if root_identity.canonical_id.is_empty() {
                            return None;
                        }
                        let target = self.host.resolve_value_export_target(
                            &root_identity.canonical_id,
                            &root_identity.symbol_name,
                        )?;
                        if target.canonical_id == root_identity.canonical_id
                            && target.name == root_identity.symbol_name
                        {
                            return None;
                        }
                        self.host
                            .prepared_value_decl(&target.canonical_id, &target.name)
                    })?;

                if let Some(object_shape) = prepared_value.object_shape.as_ref() {
                    let object_expr = TypeExpr::Object(std::sync::Arc::new(object_shape.clone()));
                    return self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &object_expr,
                        member_name,
                        depth + 1,
                    );
                }

                if let Some(type_annotation) = prepared_value.type_annotation.as_ref() {
                    return self.enumerate_member_surface_keys_via_route(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        type_annotation,
                        member_name,
                        depth + 1,
                    );
                }

                None
            }
            TypeExpr::Parenthesized(inner) => self.enumerate_member_surface_keys_via_route(
                resolution_scope_canonical_id,
                active_scope_canonical_id,
                inner,
                member_name,
                depth + 1,
            ),
            // Path C C11-residual-C: distribute member-name lookup over
            // an `IndexedAccess` whose object is compound or reducible.
            // For `(typeof theme & GetComponentAppConfig<...>)['variants']['color']`
            // we want `theme.variants.color`'s keys merged with the
            // conditional arm's `variants.color`'s keys. Pre-residual-C
            // the catch-all returned `None` and the test lost AppConfig's
            // `neutral` because the dispatch couldn't reduce the
            // outer IndexedAccess to a concrete shape.
            //
            // Handles:
            // - object = Intersection / Union: distribute over arms.
            // - object = Conditional: distribute over true / false branches.
            // - object = Ref with type_arguments: expand the alias body
            //   and retry.
            // - object = nested IndexedAccess: recurse on inner before
            //   re-applying the outer index.
            TypeExpr::IndexedAccess { object, index } => {
                match object.as_ref() {
                    TypeExpr::Intersection(parts) | TypeExpr::Union(parts) => {
                        let parts = std::sync::Arc::clone(parts);
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for arm in parts.iter() {
                            let arm_indexed = TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(arm.clone()),
                                index: index.clone(),
                            };
                            if let Some(arm_keys) = self.enumerate_member_surface_keys_via_route(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &arm_indexed,
                                member_name,
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(arm_keys);
                            }
                        }
                        if any_enumerable {
                            keys.sort();
                            keys.dedup();
                            Some(keys)
                        } else {
                            None
                        }
                    }
                    TypeExpr::Conditional {
                        true_type,
                        false_type,
                        ..
                    } => {
                        let mut keys = Vec::new();
                        let mut any_enumerable = false;
                        for branch in [true_type.as_ref(), false_type.as_ref()] {
                            let branch_indexed = TypeExpr::IndexedAccess {
                                object: std::sync::Arc::new(branch.clone()),
                                index: index.clone(),
                            };
                            if let Some(branch_keys) = self.enumerate_member_surface_keys_via_route(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                &branch_indexed,
                                member_name,
                                depth + 1,
                            ) {
                                any_enumerable = true;
                                keys.extend(branch_keys);
                            }
                        }
                        if any_enumerable {
                            keys.sort();
                            keys.dedup();
                            Some(keys)
                        } else {
                            None
                        }
                    }
                    TypeExpr::Ref {
                        name,
                        type_arguments,
                    } => {
                        // Try expanding the alias's body (substituting
                        // type arguments), then retry the indexed access
                        // against the substituted body.
                        // Phase 5l: preserve the engine method's
                        // re-export chain walk via the engine helper.
                        let expanded = if !type_arguments.is_empty() {
                            instantiate_local_generic_ref_via_engine(
                                self,
                                resolution_scope_canonical_id,
                                object,
                            )
                            .or_else(|| {
                                instantiate_local_generic_ref_via_engine(
                                    self,
                                    active_scope_canonical_id,
                                    object,
                                )
                            })
                        } else {
                            // Non-generic Ref: look up the alias's body
                            // directly via prepared decl resolution.
                            let try_body = |me: &mut Self, scope: &str| -> Option<TypeExpr> {
                                let declaration = me.resolve_type_declaration(scope, name.as_ref());
                                let target_canonical = if declaration.canonical_source.is_empty() {
                                    scope.to_string()
                                } else {
                                    declaration.canonical_source.clone()
                                };
                                let resolved_name = if declaration.resolved_name.is_empty() {
                                    name.as_ref().to_string()
                                } else {
                                    declaration.resolved_name.clone()
                                };
                                me.prepared_type_decl(&target_canonical, &resolved_name)
                                    .map(|p| p.body.clone())
                            };
                            try_body(self, resolution_scope_canonical_id)
                                .or_else(|| try_body(self, active_scope_canonical_id))
                        }?;
                        let expanded_indexed = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(expanded),
                            index: index.clone(),
                        };
                        self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &expanded_indexed,
                            member_name,
                            depth + 1,
                        )
                    }
                    TypeExpr::IndexedAccess { .. } => {
                        // Try resolving the inner IndexedAccess to a
                        // concrete object, then re-apply the outer
                        // index.
                        let resolved_inner = self
                            .solve_or_project_prepared_member_leaf_expr(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                object,
                            )
                            .filter(|resolved| resolved != object.as_ref())?;
                        let next = TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(resolved_inner),
                            index: index.clone(),
                        };
                        self.enumerate_member_surface_keys_via_route(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &next,
                            member_name,
                            depth + 1,
                        )
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    pub(crate) fn project_direct_utility_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        use verter_semantic::analysis::type_expand::ExpandedObjectShape;
        use verter_semantic::analysis::type_expr::TypeExpr;

        fn shape_has_surface(shape: &ExpandedObjectShape) -> bool {
            !shape.properties.is_empty() || !shape.call_signatures.is_empty()
        }

        fn projected_target_shape(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            target: &TypeExpr,
        ) -> Option<ExpandedObjectShape> {
            // Phase 5l: route through the dispatch-based bridges in
            // `meta_resolve` instead of the deprecated engine methods.
            // The bridges compose dispatch + the engine's surviving
            // `pub(crate)` cycle-protected helpers, preserving the
            // engine method's "lower whole expr, dispatch with empty
            // path" semantics (no IndexedAccess decomposition).
            if let Some(shape) = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
                query_engine,
                scope_canonical_id,
                target,
            ) {
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            if let Some(projected) =
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    query_engine,
                    scope_canonical_id,
                    target,
                )
            {
                let shape =
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            // Phase 5l: preserve the engine method's re-export chain
            // walk by routing through the engine helper rather than
            // the dispatch-only variant.
            let expanded_ref_opt =
                instantiate_local_generic_ref_via_engine(query_engine, scope_canonical_id, target);
            if let Some(expanded_ref) = expanded_ref_opt {
                if let Some(shape) =
                    crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
                        query_engine,
                        scope_canonical_id,
                        &expanded_ref,
                    )
                {
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                if let Some(projected) =
                    crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                        query_engine,
                        scope_canonical_id,
                        &expanded_ref,
                    )
                {
                    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                        &projected,
                    );
                    if shape_has_surface(&shape) {
                        return Some(shape);
                    }
                }
                let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(
                    &expanded_ref,
                );
                if shape_has_surface(&shape) {
                    return Some(shape);
                }
            }
            None
        }

        let TypeExpr::Ref {
            name,
            type_arguments,
        } = strip_parens_expr(expr)
        else {
            return None;
        };

        match (name.as_ref(), type_arguments.as_ref()) {
            ("Partial", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = true;
                    }
                    shape
                })
            }
            ("Required", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.optional = false;
                    }
                    shape
                })
            }
            ("Readonly", [target]) => {
                projected_target_shape(self, scope_canonical_id, target).map(|mut shape| {
                    for property in &mut shape.properties {
                        property.readonly = true;
                    }
                    shape
                })
            }
            ("NonNullable", [target]) => projected_target_shape(self, scope_canonical_id, target),
            ("Pick", [target, keys]) => {
                let requested = self.enumerate_route_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                shape.properties.retain(|property| {
                    requested
                        .iter()
                        .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            ("Omit", [target, keys]) => {
                let omitted = self.enumerate_route_literal_keys(
                    scope_canonical_id,
                    scope_canonical_id,
                    keys,
                )?;
                let mut shape = projected_target_shape(self, scope_canonical_id, target)?;
                shape.properties.retain(|property| {
                    !omitted
                        .iter()
                        .any(|candidate| candidate == property.name.as_str())
                });
                shape_has_surface(&shape).then_some(shape)
            }
            _ => None,
        }
    }

    pub(crate) fn project_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        fn single_member_route_cache_entry(
            query_engine: &mut ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            root_symbol: &str,
            member_name: &str,
            projected_expr: &TypeExpr,
        ) -> Option<ProjectedMember> {
            // Phase 5l: dispatch a single-member ProjectPath query for
            // the (root_symbol, member_name) pair, then fall back to the
            // engine's prepared/inherited route helpers (kept on the
            // engine because they consume the per-engine prepared-decl
            // request-root state).
            dispatch_member_for_root_symbol(
                query_engine,
                scope_canonical_id,
                root_symbol,
                member_name,
            )
            .or_else(|| {
                query_engine.project_prepared_member_route_projection(
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )
            })
            .or_else(|| {
                query_engine.project_inherited_member_route_projection(
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )
            })
            .or_else(|| {
                let prepared = query_engine.prepared_type_decl(scope_canonical_id, root_symbol)?;
                let member = prepared.member(member_name)?;
                Some(ProjectedMember {
                    name: member_name.to_string(),
                    ty: projected_expr.clone(),
                    optional: member.optional,
                    readonly: member.readonly,
                    is_method: member.is_method,
                })
            })
        }

        if let Some(cached_expr) =
            self.cached_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
        {
            return Some(cached_expr);
        }

        if let Some(projected_expr) =
            self.project_routed_expr_surface_expr_direct(scope_canonical_id, root_symbol, route)
        {
            self.cache_routed_expr_surface_expr(
                scope_canonical_id,
                root_symbol,
                route,
                &projected_expr,
            );
            if let super::RouteDemand::MemberPath(path) = route {
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
            }
            if let super::RouteDemand::Pick(members) = route {
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
            }
            return Some(projected_expr);
        }

        if let super::RouteDemand::MemberPath(path) = route {
            if let Some(projected_expr) = self.project_prepared_member_path_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                path,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                if let [member_name] = path.as_slice() {
                    if let Some(projected_member) = single_member_route_cache_entry(
                        self,
                        scope_canonical_id,
                        root_symbol,
                        member_name,
                        &projected_expr,
                    ) {
                        self.cache_projected_member(
                            scope_canonical_id,
                            root_symbol,
                            &projected_member,
                        );
                    }
                }
                return Some(projected_expr);
            }
            if let [member_name] = path.as_slice() {
                // Phase 5l: dispatch the single-member projection.
                let projected_member = dispatch_member_for_root_symbol(
                    self,
                    scope_canonical_id,
                    root_symbol,
                    member_name,
                )?;
                let projected_expr = projected_member.ty.clone();
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
                return Some(projected_expr);
            }
        }

        if let super::RouteDemand::Pick(members) = route {
            if let Some(projected_expr) = self.project_prepared_pick_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_members(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_routed_expr(
                scope_canonical_id,
                root_symbol,
                route,
                members,
            ) {
                self.cache_routed_expr_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    route,
                    &projected_expr,
                );
                self.cache_pick_members_from_projected_expr(
                    scope_canonical_id,
                    root_symbol,
                    members,
                    &projected_expr,
                );
                return Some(projected_expr);
            }
        }

        None
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cached_routed_expr_surface_expr(
        &self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("routed_expr_surface_cache");
        let local_key = RoutedExprSurfaceCacheKey {
            scope_canonical_id: scope_canonical_id.to_owned(),
            root_symbol: root_symbol.to_owned(),
            route: route.clone(),
        };
        if let Some(cached) = self
            .routed_expr_surface_cache
            .borrow()
            .get(&local_key)
            .cloned()
        {
            return Some(cached);
        }
        // Step 3 closure: peek host-owned RoutedExprSurfaceDb.
        let arc_key =
            arc_routed_expr_surface_cache_key(scope_canonical_id, root_symbol, route.clone());
        let host_db = self.host.project_type_store().routed_expr_surface_db();
        let arc_value = host_db.peek(&arc_key, self.host)?;
        let value = arc_value.as_ref().clone();
        self.routed_expr_surface_cache
            .borrow_mut()
            .insert(local_key, value.clone());
        Some(value)
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cache_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
        projected_expr: &TypeExpr,
    ) {
        let local_key = RoutedExprSurfaceCacheKey {
            scope_canonical_id: scope_canonical_id.to_owned(),
            root_symbol: root_symbol.to_owned(),
            route: route.clone(),
        };
        // Step 3 closure: write-through to host-owned RoutedExprSurfaceDb.
        let arc_key =
            arc_routed_expr_surface_cache_key(scope_canonical_id, root_symbol, route.clone());
        let host = self.host;
        let host_db = host.project_type_store().routed_expr_surface_db();
        let captured_value = projected_expr.clone();
        let captured_canonical = scope_canonical_id.to_string();
        let _ = host_db.get_or_compute(&arc_key, host, move || {
            let dep_sig = engine_dep_signature_for_canonical(host, captured_canonical.as_str());
            Some((captured_value, dep_sig))
        });
        self.routed_expr_surface_cache
            .borrow_mut()
            .insert(local_key, projected_expr.clone());
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cache_pick_members_from_projected_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
        projected_expr: &TypeExpr,
    ) {
        use std::collections::BTreeSet;
        use verter_semantic::analysis::type_expr::ObjectMember;

        let requested: BTreeSet<_> = members.iter().map(String::as_str).collect();
        let TypeExpr::Object(object) = projected_expr else {
            return;
        };
        for member in &object.properties {
            let projected_member = match member {
                ObjectMember::Property(property) if requested.contains(property.name.as_str()) => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: property.ty.clone(),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if requested.contains(method.name.as_str()) => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            };
            if let Some(projected_member) = projected_member {
                self.cache_projected_member(scope_canonical_id, root_symbol, &projected_member);
            }
        }
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cache_projected_member(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        projected_member: &ProjectedMember,
    ) {
        let _ = (scope_canonical_id, root_symbol, projected_member);
    }

    fn cached_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<ProjectedMember> {
        let _ = (scope_canonical_id, symbol_name, member_name, substitutions);
        None
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cached_prepared_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let _ = (scope_canonical_id, symbol_name, substitutions);
        None
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn cache_prepared_surface_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        projection: &PreparedSurfaceProjection,
    ) {
        let _ = (scope_canonical_id, symbol_name, substitutions, projection);
    }

    fn cache_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        projected_member: &ProjectedMember,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) {
        let _ = (
            scope_canonical_id,
            symbol_name,
            projected_member,
            substitutions,
        );
    }

    #[allow(dead_code)]
    fn prepared_requested_member_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        !substitutions.is_empty()
            && self
                .current_prepared_request_root
                .as_deref()
                .is_some_and(|request_root| request_root != scope_canonical_id)
    }

    #[allow(dead_code)]
    fn prepared_surface_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        self.current_prepared_request_root
            .as_deref()
            .is_some_and(|request_root| request_root != scope_canonical_id)
            && (!substitutions.is_empty() || is_package_source(Some(scope_canonical_id)))
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let member = prepared.member(member_name)?;
        self.project_prepared_member_from_decl(scope_canonical_id, &prepared, member_name, member)
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_member_from_decl(
        &mut self,
        scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        member_name: &str,
        member: &verter_semantic::analysis::type_solver::prepared::PreparedMember,
    ) -> Option<ProjectedMember> {
        if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
            return None;
        }
        let projected_ty = match &member.ty {
            TypeExpr::Object(_) => Some(member.ty.clone()),
            _ if prepared_member_body_stays_shallow(&member.ty) => Some(member.ty.clone()),
            _ if prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &member.ty) => {
                Some(member.ty.clone())
            }
            _ if crate::meta_resolve::component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &member.ty,
                scope_canonical_id,
                self,
            ) =>
            {
                Some(member.ty.clone())
            }
            _ => {
                // Phase 5l: dispatch path replaces the deprecated method.
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    self,
                    scope_canonical_id,
                    &member.ty,
                )
            }
        }?;
        Some(ProjectedMember {
            name: member_name.to_string(),
            ty: projected_ty,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        })
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_member_path_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
    ) -> Option<TypeExpr> {
        let mut visited = FxHashSet::default();
        self.project_prepared_member_path_route_projection_from_symbol(
            scope_canonical_id,
            scope_canonical_id,
            symbol_name,
            path,
            &FxHashMap::default(),
            &mut visited,
        )
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn expr_references_prepared_scope_symbol(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> bool {
        use verter_semantic::analysis::type_expr::ObjectMember;

        match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                (!is_builtin_name(name.as_ref())
                    && self
                        .prepared_type_decl(scope_canonical_id, name.as_ref())
                        .is_some())
                    || type_arguments.iter().any(|arg| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, arg)
                    })
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, inner)
            }
            TypeExpr::Tuple { elements, .. } => elements.iter().any(|element| {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, &element.ty)
            }),
            TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
                .iter()
                .any(|ty| self.expr_references_prepared_scope_symbol(scope_canonical_id, ty)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                ObjectMember::Property(property) => {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &property.ty)
                }
                ObjectMember::Method(method) => {
                    method.function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || method
                        .function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| {
                            self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                return_type,
                            )
                        })
                }
                ObjectMember::IndexSignature(signature) => {
                    self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.key_type,
                    ) || self.expr_references_prepared_scope_symbol(
                        scope_canonical_id,
                        &signature.value_type,
                    )
                }
                ObjectMember::CallSignature(function)
                | ObjectMember::ConstructSignature(function) => {
                    function.parameters.iter().any(|param| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                    }) || function.return_type.as_deref().is_some_and(|return_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                    })
                }
            }),
            TypeExpr::Function(function) => {
                function.parameters.iter().any(|param| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, &param.ty)
                }) || function.return_type.as_deref().is_some_and(|return_type| {
                    self.expr_references_prepared_scope_symbol(scope_canonical_id, return_type)
                })
            }
            TypeExpr::IndexedAccess { object, index } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, object)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, index)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, check)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, extends)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, true_type)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, false_type)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                self.expr_references_prepared_scope_symbol(scope_canonical_id, source)
                    || self.expr_references_prepared_scope_symbol(scope_canonical_id, value)
                    || name_type.as_deref().is_some_and(|name_type| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, name_type)
                    })
            }
            TypeExpr::TemplateLiteral { expressions, .. } => expressions
                .iter()
                .any(|expr| self.expr_references_prepared_scope_symbol(scope_canonical_id, expr)),
            TypeExpr::TypeParameter(type_parameter) => {
                type_parameter
                    .constraint
                    .as_deref()
                    .is_some_and(|constraint| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, constraint)
                    })
                    || type_parameter.default.as_deref().is_some_and(|default| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, default)
                    })
            }
            TypeExpr::RecursiveRef {
                type_arguments,
                conditional_context,
                ..
            } => {
                type_arguments
                    .iter()
                    .any(|arg| self.expr_references_prepared_scope_symbol(scope_canonical_id, arg))
                    || conditional_context.iter().any(|frame| {
                        self.expr_references_prepared_scope_symbol(scope_canonical_id, &frame.check)
                            || self.expr_references_prepared_scope_symbol(
                                scope_canonical_id,
                                &frame.extends,
                            )
                    })
            }
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. }
            | TypeExpr::Unknown { .. } => false,
        }
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn solve_or_project_prepared_member_leaf_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let context = PreparedProjectionContext {
            decl_scope: resolution_scope_canonical_id.to_string(),
            arg_scope: active_scope_canonical_id.to_string(),
            chain_scopes: self.projection_chain_scopes.clone(),
        };
        self.solve_or_project_leaf_expr_with_context(&context, expr)
    }

    /// Path C C11b — per-TypeExpr-shape scope dispatch for the prepared-
    /// member-path projection (plan §2 Stage 6 Pass C11b).
    ///
    /// The pre-C11b logic tried `active_scope` first and then fell back to
    /// `resolution_scope` only when the expression referenced a prepared
    /// symbol in that scope. That gate missed transitive helper refs
    /// (e.g., `ComponentUI<typeof theme>` where `ComponentUI` lives in a
    /// type-file reached via the prepared decl's import chain, not the
    /// decl's immediate symbol map).
    ///
    /// C11b uses a `PreparedProjectionContext { decl_scope, arg_scope }`:
    /// - bare `Ref { name, type_arguments: [] }`: try `decl_scope` first
    ///   (helper-body-internal reference), fall back to `arg_scope`.
    /// - `TypeOf(value_ref)`: always resolve in `arg_scope` (caller-
    ///   scoped value symbol table).
    /// - `Ref { name, type_arguments }`: resolve the NAME in `decl_scope`
    ///   so helper aliases lower against their own declaration site;
    ///   lower `type_arguments` in `arg_scope` so caller-scoped
    ///   `typeof theme` / explicit type arguments stay resolvable.
    ///   After both halves resolve, re-run
    ///   `solve_or_project_leaf_expr_until_stable` in `decl_scope` to
    ///   bridge the instantiation.
    /// - compound shapes (`IndexedAccess`, `Conditional`, `Mapped`,
    ///   `KeyOf`, etc.): fall back to the two-scope retry path (active
    ///   first, resolution fallback). The compound shapes don't need
    ///   per-sub-expression scope splitting because their sub-
    ///   expressions are already `TypeExpr` leaves that round-trip
    ///   through this function.
    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn solve_or_project_leaf_expr_with_context(
        &mut self,
        context: &PreparedProjectionContext,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let decl_scope = context.decl_scope.clone();
        let arg_scope = context.arg_scope.clone();
        let chain_scopes = context.chain_scopes.clone();

        if decl_scope == arg_scope {
            return self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
        }

        match expr {
            TypeExpr::Ref {
                name: _,
                type_arguments,
            } if type_arguments.is_empty() => {
                // Bare `Ref { name, [] }`: helper-body-internal reference.
                // Try decl_scope first; fall back to arg_scope.
                if let Some(result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &result != expr {
                        return Some(result);
                    }
                }
                self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr)
            }
            TypeExpr::TypeOf(_) => {
                // `typeof value_ref`: caller-scoped first (the most
                // common case is `Foo['x']` where `Foo` is a value
                // imported into the calling SFC). Path C C11-residual-B:
                // some helper-aliased patterns reference values that
                // are visible in OUTER helper scopes (e.g.,
                // `type Button = ComponentConfig<typeof theme>` declared
                // in `button-types.ts` — `theme` is visible there, but
                // by the time the projection recurses into
                // `ComponentConfig`'s body in `types.ts`, neither
                // `decl_scope=types.ts` nor `arg_scope=ImportedSlotButton.vue`
                // can resolve `theme`. The `chain_scopes` carry the
                // outer declaration scopes through the recursion so
                // the value reference can find its visible scope.
                let arg_first = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_first {
                    if result != expr {
                        return arg_first;
                    }
                }
                if let Some(decl_result) =
                    self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                {
                    if &decl_result != expr {
                        return Some(decl_result);
                    }
                }
                for chain_scope in &chain_scopes {
                    if chain_scope == &decl_scope || chain_scope == &arg_scope {
                        continue;
                    }
                    if let Some(chain_result) =
                        self.solve_or_project_leaf_expr_until_stable(chain_scope, expr)
                    {
                        if &chain_result != expr {
                            return Some(chain_result);
                        }
                    }
                }
                arg_first
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                // `Ref { name, [args..] }`: helper instantiation. Resolve
                // the name in decl_scope (so the helper's declaration
                // registry is consulted), and lower type_arguments in
                // arg_scope (so caller-side `typeof`, locally-declared
                // types, etc. stay resolvable). The simplest way to
                // plumb both is to try decl_scope first — the helper's
                // body will instantiate against its own declaration-
                // site symbol table. If decl_scope resolves the helper
                // (non-trivially), return the decl_scope projection.
                // Otherwise fall back to arg_scope where the ref name
                // may be reachable via direct import.
                let decl_first = self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr);
                if let Some(ref result) = decl_first {
                    if result != expr {
                        return decl_first;
                    }
                }
                let arg_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if let Some(ref result) = arg_result {
                    if result != expr {
                        return arg_result;
                    }
                }
                // Path C C11-residual-B: split-scope projection. When
                // the ref's name belongs to one scope (e.g. `ComponentUI`
                // declared in `types.ts`) and its type_arguments
                // reference values from another scope (e.g.
                // `typeof theme` visible only in `button-types.ts`),
                // pre-resolve each `TypeOf(value)` argument in any
                // chain scope where the value is visible, then re-try
                // the projection with the resolved arguments substituted.
                if !chain_scopes.is_empty() {
                    let mut resolved_args: Vec<TypeExpr> = Vec::with_capacity(type_arguments.len());
                    let mut any_argument_resolved = false;
                    for arg in type_arguments.iter() {
                        let mut resolved = arg.clone();
                        if matches!(arg, TypeExpr::TypeOf(_)) {
                            for chain_scope in &chain_scopes {
                                if chain_scope == &decl_scope || chain_scope == &arg_scope {
                                    continue;
                                }
                                if let Some(chain_arg) =
                                    self.solve_or_project_leaf_expr_until_stable(chain_scope, arg)
                                {
                                    if &chain_arg != arg {
                                        resolved = chain_arg;
                                        any_argument_resolved = true;
                                        break;
                                    }
                                }
                            }
                        }
                        resolved_args.push(resolved);
                    }
                    if any_argument_resolved {
                        let resolved_expr = TypeExpr::Ref {
                            name: name.clone(),
                            type_arguments: std::sync::Arc::from(resolved_args),
                        };
                        if let Some(result) = self
                            .solve_or_project_leaf_expr_until_stable(&decl_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                        if let Some(result) =
                            self.solve_or_project_leaf_expr_until_stable(&arg_scope, &resolved_expr)
                        {
                            return Some(result);
                        }
                    }
                }
                arg_result.or(decl_first)
            }
            _ => {
                // Compound shapes (IndexedAccess, Conditional, Mapped,
                // KeyOf, Intersection, Union, Parenthesized, etc.)
                // preserve the pre-C11b two-scope retry. Inner
                // sub-expressions come back through this function so
                // per-shape dispatch still applies transitively.
                let active_result = self.solve_or_project_leaf_expr_until_stable(&arg_scope, expr);
                if !self.expr_references_prepared_scope_symbol(&decl_scope, expr) {
                    return active_result;
                }
                self.solve_or_project_leaf_expr_until_stable(&decl_scope, expr)
                    .or(active_result)
            }
        }
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn solve_or_project_leaf_expr_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let mut current = expr.clone();
        let mut last = None;
        for _ in 0..3 {
            // Phase 5l: dispatch the lower+project tail and the
            // expr-surface bridge from `meta_resolve` instead of the
            // deprecated engine methods. The bridges share the engine's
            // cycle-protection helpers so behavior matches the legacy
            // method path.
            let next = crate::meta_resolve::lower_and_project_to_expanded_via_host_threaded(
                self,
                scope_canonical_id,
                &current,
            )
            .or_else(|| {
                crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
                    self,
                    scope_canonical_id,
                    &current,
                )
            });
            let Some(next) = next else {
                return last;
            };
            if next == current {
                return Some(next);
            }
            last = Some(next.clone());
            current = next;
        }
        last
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_member_path_route_projection_from_symbol(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        symbol_name: &str,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        let visit_key = (
            resolution_scope_canonical_id.to_string(),
            symbol_name.to_string(),
        );
        if !visited.insert(visit_key.clone()) {
            return None;
        }
        if substitutions.is_empty() {
            if let Some(prepared) =
                self.prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            {
                if let Some(default_substitutions) =
                    build_default_type_param_substitutions(prepared.as_ref(), &[])
                {
                    if !default_substitutions.is_empty() {
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                resolution_scope_canonical_id,
                                active_scope_canonical_id,
                                symbol_name,
                                path,
                                &default_substitutions,
                                visited,
                            );
                        visited.remove(&visit_key);
                        return result;
                    }
                }
            }
        }

        let result = self
            .prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member_name) = path.first() {
                    if let Some(member) = prepared.member(member_name) {
                        let member_ty = apply_type_param_substitutions(&member.ty, substitutions);
                        if path.len() == 1 {
                            return self
                                .solve_or_project_prepared_member_leaf_expr(
                                    resolution_scope_canonical_id,
                                    active_scope_canonical_id,
                                    &member_ty,
                                )
                                .or(Some(member_ty));
                        }
                        return self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared.as_ref(),
                            &member_ty,
                            &path[1..],
                            &FxHashMap::default(),
                            visited,
                        );
                    }
                }

                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    path,
                    substitutions,
                    visited,
                )
            });

        visited.remove(&visit_key);
        result
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_member_path_route_projection_from_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        path: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        let Some((member_name, tail)) = path.split_first() else {
            let projected_expr = apply_type_param_substitutions(expr, substitutions);
            return self
                .solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &projected_expr,
                )
                .or(Some(projected_expr));
        };

        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    inner,
                    path,
                    substitutions,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    part,
                    path,
                    substitutions,
                    visited,
                )
            }),
            TypeExpr::Object(object) => {
                let member_ty = object.properties.iter().find_map(|member| match member {
                    ObjectMember::Property(property) if property.name == *member_name => {
                        Some(apply_type_param_substitutions(&property.ty, substitutions))
                    }
                    ObjectMember::Method(method) if method.name == *member_name => {
                        Some(TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )))
                    }
                    _ => None,
                })?;
                if tail.is_empty() {
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_member_path_route_projection_from_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        prepared,
                        &substituted,
                        path,
                        &FxHashMap::default(),
                        visited,
                    );
                }

                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner])
                    | ("Required", [inner])
                    | ("Readonly", [inner])
                    | ("NonNullable", [inner]) => self
                        .project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            inner,
                            path,
                            substitutions,
                            visited,
                        ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            resolution_scope_canonical_id,
                            prepared,
                            keys,
                            visited,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_member_path_route_projection_from_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            prepared,
                            target,
                            path,
                            substitutions,
                            visited,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                resolution_scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = build_default_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        // Path C C11-residual-B: as we descend into the
                        // target alias's declaration scope, push the
                        // current `resolution_scope_canonical_id` onto
                        // the projection chain. The leaf-expr handler
                        // uses this chain to find the scope where a
                        // `TypeOf(value)` reference was visible at the
                        // outer call site (e.g., `theme` imported in
                        // `button-types.ts` while we're now recursing
                        // into `ComponentConfig`'s body in `types.ts`).
                        let pushed = if !self
                            .projection_chain_scopes
                            .iter()
                            .any(|s| s == resolution_scope_canonical_id)
                            && resolution_scope_canonical_id != target_canonical_id
                        {
                            self.projection_chain_scopes
                                .push(resolution_scope_canonical_id.to_string());
                            true
                        } else {
                            false
                        };
                        let result = self
                            .project_prepared_member_path_route_projection_from_symbol(
                                &target_canonical_id,
                                active_scope_canonical_id,
                                &target_symbol_name,
                                path,
                                &target_substitutions,
                                visited,
                            );
                        if pushed {
                            self.projection_chain_scopes.pop();
                        }
                        result
                    }
                }
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                name_type,
                ..
            } if name_type.is_none() => {
                let substituted_source = apply_type_param_substitutions(source, substitutions);
                let Some(keys) = self.enumerate_route_literal_keys(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &substituted_source,
                ) else {
                    let nested_expr = path.iter().fold(
                        apply_type_param_substitutions(expr, substitutions),
                        |object, member| TypeExpr::IndexedAccess {
                            object: std::sync::Arc::new(object),
                            index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                        },
                    );
                    return self.solve_or_project_prepared_member_leaf_expr(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &nested_expr,
                    );
                };
                if !keys.iter().any(|candidate| candidate == member_name) {
                    return None;
                }

                let mut member_substitutions = substitutions.clone();
                member_substitutions.insert(
                    parameter.clone(),
                    TypeExpr::string_literal(member_name.clone()),
                );
                let member_ty = apply_type_param_substitutions(value, &member_substitutions);
                if tail.is_empty() {
                    if let Some(keys) = self.enumerate_route_literal_keys(
                        resolution_scope_canonical_id,
                        active_scope_canonical_id,
                        &member_ty,
                    ) {
                        return string_literal_keys_type_expr(keys);
                    }
                    return self
                        .solve_or_project_prepared_member_leaf_expr(
                            resolution_scope_canonical_id,
                            active_scope_canonical_id,
                            &member_ty,
                        )
                        .or(Some(member_ty));
                }
                self.project_prepared_member_path_route_projection_from_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    prepared,
                    &member_ty,
                    tail,
                    &FxHashMap::default(),
                    visited,
                )
            }
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Union(_)
            | TypeExpr::Tuple { .. }
            | TypeExpr::Array { .. }
            | TypeExpr::KeyOf(_)
            | TypeExpr::TypeParameter(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => {
                let nested_expr = path.iter().fold(
                    apply_type_param_substitutions(expr, substitutions),
                    |object, member| TypeExpr::IndexedAccess {
                        object: std::sync::Arc::new(object),
                        index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
                    },
                );
                self.solve_or_project_prepared_member_leaf_expr(
                    resolution_scope_canonical_id,
                    active_scope_canonical_id,
                    &nested_expr,
                )
            }
            TypeExpr::Function(_)
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. } => None,
        }
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        let mut visited = FxHashSet::default();
        self.project_inherited_member_route_projection_from_symbol(
            scope_canonical_id,
            symbol_name,
            member_name,
            &mut visited,
        )
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::InheritedRoute,
            substitutions: PreparedSubstitutionKey::Empty,
        };
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("prepared_member_cache");
        if let Some(cached) = self.prepared_member_cache.borrow().get(&cache_key).cloned() {
            return cached;
        }
        // Step 3 closure: peek host-owned PreparedMemberDb (InheritedRoute).
        {
            let arc_key = arc_prepared_member_cache_key(
                scope_canonical_id,
                symbol_name,
                member_name,
                crate::resolver_core::cache_keys::PreparedMemberCacheKind::InheritedRoute,
                &FxHashMap::default(),
            );
            let host_db = self.host.project_type_store().prepared_member_db();
            if let Some(opt_arc) = host_db.peek(&arc_key, self.host) {
                let value = opt_arc.map(|arc_member| arc_member.as_ref().clone());
                self.prepared_member_cache
                    .borrow_mut()
                    .insert(cache_key, value.clone());
                return value;
            }
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !visited.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    return self.project_prepared_member_from_decl(
                        scope_canonical_id,
                        &prepared,
                        member_name,
                        member,
                    );
                }

                self.project_inherited_member_route_projection_from_expr(
                    scope_canonical_id,
                    &prepared,
                    &prepared.body,
                    member_name,
                    visited,
                )
            });

        visited.remove(&visit_key);
        self.publish_prepared_member_to_host_db(
            scope_canonical_id,
            symbol_name,
            member_name,
            crate::resolver_core::cache_keys::PreparedMemberCacheKind::InheritedRoute,
            &FxHashMap::default(),
            &result,
        );
        self.prepared_member_cache
            .borrow_mut()
            .insert(cache_key, result.clone());
        result
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_inherited_member_route_projection_from_expr(
        &mut self,
        _scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        expr: &TypeExpr,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    inner,
                    member_name,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_inherited_member_route_projection_from_expr(
                    _scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    visited,
                )
            }),
            TypeExpr::Ref { name, .. } => {
                let resolved = prepared.name_resolution.get(name.as_ref())?;
                self.project_inherited_member_route_projection_from_symbol(
                    resolved.canonical_id.as_str(),
                    resolved.symbol_name.as_str(),
                    member_name,
                    visited,
                )
            }
            _ => None,
        }
    }

    #[cfg(test)]
    fn project_prepared_member_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        self.project_prepared_member_route_projection(scope_canonical_id, symbol_name, member_name)
            .map(|projected_member| projected_member.ty)
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_pick_route_surface_expr_via_members(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name);
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let projected_member = if prepared
                .as_ref()
                .and_then(|prepared| prepared.member(member_name))
                .is_some()
            {
                self.project_prepared_member_route_projection(
                    scope_canonical_id,
                    symbol_name,
                    member_name,
                )?
            } else if let Some(projected_member) = self.project_inherited_member_route_projection(
                scope_canonical_id,
                symbol_name,
                member_name,
            ) {
                projected_member
            } else {
                // Phase 5l: dispatch path replaces the deprecated method.
                dispatch_member_for_root_symbol(self, scope_canonical_id, symbol_name, member_name)?
            };
            self.cache_projected_member(scope_canonical_id, symbol_name, &projected_member);
            if projected_member.is_method {
                if let TypeExpr::Function(function) = &projected_member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: projected_member.name,
                        function: (**function).clone(),
                        optional: projected_member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: projected_member.name,
                ty: projected_member.ty,
                optional: projected_member.optional,
                readonly: projected_member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_pick_route_surface_expr_via_routed_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        route: &super::RouteDemand,
        _members: &[String],
    ) -> Option<TypeExpr> {
        assert_direct_pick_routed_expr_slow_lane_allowed();
        self.project_routed_expr_surface_expr_direct(scope_canonical_id, symbol_name, route)
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_routed_expr_surface_expr_direct(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        self.dispatch_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
    }

    #[allow(dead_code)] // Phase 5c: deletion in 5g per call-graph closure
    fn project_prepared_pick_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        members: &[String],
    ) -> Option<TypeExpr> {
        use verter_semantic::analysis::type_expr::{
            MethodSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
        };

        let prepared = self.prepared_type_decl(scope_canonical_id, symbol_name)?;
        let mut properties = Vec::with_capacity(members.len());
        for member_name in members {
            let member = prepared.member(member_name)?;
            if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                return None;
            }
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    properties.push(ObjectMember::Method(MethodSignature {
                        name: member_name.clone(),
                        function: (**function).clone(),
                        optional: member.optional,
                    }));
                    continue;
                }
            }
            properties.push(ObjectMember::Property(ObjectProperty {
                name: member_name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            }));
        }
        Some(TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
            properties,
        })))
    }

    #[allow(dead_code)]
    fn type_surface_facts(
        &self,
        scope_canonical_id: &str,
    ) -> Option<Vec<crate::resolver_core::FactVersionRef>> {
        let store_view = self.host.resolver_store_view();
        let mut facts = Vec::new();
        // Post-cut: live-host whole-hash with store-view as the first
        // consultation, falling back to the live host probe for
        // untracked-but-present canonicals.
        let hash = store_view
            .whole_hash(scope_canonical_id)
            .or_else(|| self.host.get_whole_hash(scope_canonical_id));
        if let Some(hash) = hash {
            facts.push(crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: scope_canonical_id.to_string(),
                hash,
            });
        }
        if let Some(hash) = store_view.derived_hash(
            scope_canonical_id,
            crate::resolver_core::DerivedFactKind::Route,
        ) {
            facts.push(crate::resolver_core::FactVersionRef::DerivedFactHash {
                canonical_id: scope_canonical_id.to_string(),
                kind: crate::resolver_core::DerivedFactKind::Route,
                hash,
            });
        }
        (!facts.is_empty()).then_some(facts)
    }

    #[cfg(test)]
    fn debug_prepared_surface_cache_len(&self) -> usize {
        self.prepared_surface_cache.borrow().len()
    }

    #[cfg(test)]
    fn debug_prepared_member_cache_len(&self) -> usize {
        self.prepared_member_cache.borrow().len()
    }

    #[cfg(test)]
    fn debug_prepared_target_cache_len(&self) -> usize {
        self.prepared_target_cache.borrow().len()
    }
}

fn local_type_symbol_metadata_for_known_source(
    host: &VerterHost,
    canonical_source: &str,
    resolved_name: &str,
) -> Option<ResolvedLocalTypeSymbolMetadata> {
    let analysis = host.external_type_analysis(canonical_source)?;
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
    host: &'a VerterHost,
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
        self.host
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
        local_type_symbol_metadata_for_known_source(self.host, canonical_source, resolved_name)
    }
}

fn empty_semantic_args() -> std::sync::Arc<[SemanticNodeId]> {
    std::sync::Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice())
}

/// Phase 5l — engine-internal helper that mirrors the deprecated
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

/// Phase 5l — engine-internal substitution helper that mirrors the
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
    if is_package_source(Some(target_canonical_id.as_str())) {
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
mod tests {
    use super::forbid_direct_pick_routed_expr_slow_lane_for_tests;
    use super::forbid_structural_slow_lane_for_tests;
    use super::ComponentMetaQueryEngine;
    use super::{
        direct_pick_routed_expr_slow_lane_forbidden_for_current_thread,
        forbid_prepared_structural_substitution_slow_lane_for_tests,
        prepared_structural_substitution_slow_lane_forbidden_for_current_thread,
        structural_slow_lane_forbidden_for_current_thread, type_expr_references_type_params,
    };
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::PrimitiveName;
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    #[test]
    fn resolve_direct_prepared_type_declaration_matches_local_prepared_decl() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared declaration should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface,
        );
        assert!(
            declaration.span.end > declaration.span.start,
            "direct prepared declaration should still expose a non-empty span",
        );
        // Phase 4b §4b.3 — declaration text recovery via source-
        // reparse is retired. The resolver returns kind/span from
        // graph metadata; text stays None.
        assert_eq!(
            declaration.text, None,
            "graph-only resolver: declaration text is no longer recovered",
        );
    }

    #[test]
    fn resolve_direct_prepared_type_declaration_metadata_skips_text_recovery() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export interface AvatarProps {
  src?: string
  alt?: string
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration_metadata("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared metadata should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface,
        );
        assert!(
            declaration.span.end > declaration.span.start,
            "direct prepared metadata should still retain declaration span"
        );
        assert_eq!(
            declaration.text, None,
            "metadata-only resolution should skip declaration text extraction for routed registry lookups",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_projects_type_param_free_member_body() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface BaseProps {
  disabled?: boolean
  type?: 'single' | 'multiple'
}

type Button = {
  slots: {
    base?: string
    label?: string
  }
}

export interface Props extends Pick<BaseProps, 'disabled' | 'type'> {
  ui?: Button['slots']
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
            .expect("prepared member route surface should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected member surface should be an object, got {projected:?}");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["base", "label"]),
            "member route projection should follow the raw prepared member body to the requested surface",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_keeps_scalar_union_members_off_solver() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props {
  name?: 'foo' | 'bar'
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "name")
            .expect("prepared scalar member route should project");

        assert_eq!(
            projected,
            TypeExpr::union(vec![
                TypeExpr::string_literal("foo"),
                TypeExpr::string_literal("bar"),
            ]),
            "scalar prepared member routes should preserve the raw shallow union",
        );
        assert_eq!(
            0u32,
            0,
            "scalar prepared member routes should stay on cached shallow state instead of invoking the solver",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_keeps_package_refs_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/workspace/node_modules/vue-router/package.json".to_string(),
            Arc::from(
                r#"{ "name": "vue-router", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/vue-router/dist/index.d.ts".to_string(),
            Arc::from("export interface RouteLocationRaw { path?: string }\n"),
        );
        ws.inject_file(
            "/workspace/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from 'vue-router'

export interface Props {
  to?: RouteLocationRaw
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        assert!(host.ensure_loaded("/workspace/src/Link.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/workspace/src/Link.vue", "Props", "to")
            .expect("prepared package member route should project");

        assert_eq!(
            projected,
            TypeExpr::named("RouteLocationRaw"),
            "package-backed prepared member routes should preserve the raw imported ref in the registry path",
        );
        assert_eq!(
            0u32,
            0,
            "package-backed prepared member routes should stay shallow instead of invoking solver projection",
        );
    }

    #[test]
    fn project_prepared_type_surface_shape_keeps_imported_package_projection_off_indexed_ready() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/workspace/node_modules/pkg/package.json".to_string(),
            Arc::from(
                r#"{ "name": "pkg", "types": "./dist/index.d.ts", "exports": { ".": { "types": "./dist/index.d.ts" } } }"#,
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/index.d.ts".to_string(),
            Arc::from("export type { PackageProps } from './index3.d.ts'\n"),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/index3.d.ts".to_string(),
            Arc::from(
                "import type { Payload } from './payload.d.ts'\nexport interface PackageProps {\n  open?: Payload\n}\n",
            ),
        );
        ws.inject_file(
            "/workspace/node_modules/pkg/dist/payload.d.ts".to_string(),
            Arc::from("export interface Payload { value: string }\n"),
        );
        ws.inject_file(
            "/workspace/src/Child.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { PackageProps } from 'pkg'

export interface Wrapper extends PackageProps {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.configure_projects(vec![
            verter_semantic::analysis::project_resolver::IdeProjectConfig::new(
                "/workspace".to_string(),
                "/workspace".to_string(),
                Some("/workspace/tsconfig.json".to_string()),
            ),
        ]);
        assert!(host.ensure_loaded("/workspace/src/Child.vue"));

        let _view = host.resolver_store_view();
        let mut engine = ComponentMetaQueryEngine::new(&host);

        let shape = crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
            &mut engine,
            "/workspace/src/Child.vue",
            "Wrapper",
        )
        .expect("prepared package wrapper projection should resolve");

        assert!(
            shape.properties.iter().any(|property| property.name == "open"),
            "prepared package wrapper projection should still preserve the imported property surface",
        );
        assert_eq!(
            0u32,
            0,
            "prepared package wrapper projection should stay on shallow projection without solver fallback",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
                .is_none(),
            "prepared package projection should keep the provider barrel off IndexedReadyDb",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
                .is_none(),
            "prepared package projection should keep the routed package target off IndexedReadyDb",
        );
        assert!(
            host.project_type_store
                .indexed()
                .get_any("/workspace/node_modules/pkg/dist/payload.d.ts")
                .is_none(),
            "prepared package projection should keep imported helper edges shallow too",
        );
    }

    #[test]
    fn project_prepared_pick_route_surface_expr_keeps_requested_members_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type ChatMessage = {
  variants: {
    side: 'left' | 'right'
  }
  slots: {
    root?: string
  }
}

export interface IconProps {
  name?: string
}

export interface Props {
  icon?: IconProps['name']
  variant?: ChatMessage['variants']['side']
  ui?: ChatMessage['slots']
  unused?: {
    deep?: boolean
  }
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let requested = vec!["icon".to_string(), "ui".to_string(), "variant".to_string()];

        let projected = engine
            .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
            .expect("prepared pick route surface should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected pick surface should be an object, got {projected:?}");
        };

        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["icon", "ui", "variant"]),
            "pick route projection should stay on the requested members only",
        );

        let icon = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "icon" => Some(&property.ty),
                _ => None,
            })
            .expect("icon member should be present");
        assert!(
            matches!(icon, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep imported indexed member refs shallow, got {icon:?}",
        );

        let ui = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "ui" => Some(&property.ty),
                _ => None,
            })
            .expect("ui member should be present");
        assert!(
            matches!(ui, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep local indexed member refs shallow, got {ui:?}",
        );

        let variant = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "variant" => {
                    Some(&property.ty)
                }
                _ => None,
            })
            .expect("variant member should be present");
        assert!(
            matches!(variant, TypeExpr::IndexedAccess { .. }),
            "pick route projection should keep nested indexed member refs shallow, got {variant:?}",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_expands_local_alias_body_while_preserving_inner_package_ref() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/node_modules/vue/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface VNode {
  children?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
import type { VNode } from 'vue'

export type StringOrVNode = string | VNode
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { StringOrVNode } from './types'

defineProps<{
  title?: StringOrVNode
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/types.ts",
            vec![crate::types::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &TypeExpr::named("StringOrVNode"))
            .expect("local aliases that wrap package refs should use the fast shallow path");

        let TypeExpr::Union(members) = &fast.expr else {
            panic!(
                "local alias fast path should expand to the alias body, got {:?}",
                fast.expr
            );
        };
        assert!(
            members.contains(&TypeExpr::Primitive(PrimitiveName::String)),
            "expanded alias body should keep its local primitive arm, got {members:?}",
        );
        assert!(
            members.iter().any(|member| {
                matches!(
                    member,
                    TypeExpr::Ref { name, type_arguments }
                        if name.as_ref() == "VNode" && type_arguments.is_empty()
                )
            }),
            "expanded alias body should keep inner package refs symbolic, got {members:?}",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_keeps_imported_utility_routes_symbolic() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface DialogContentProps {
  id?: string
  open?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  content?: boolean | Omit<DialogContentProps, 'id'>
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::Union(Arc::from(vec![
            TypeExpr::Primitive(PrimitiveName::Boolean),
            TypeExpr::named_with_args(
                "Omit",
                vec![
                    TypeExpr::named("DialogContentProps"),
                    TypeExpr::string_literal("id"),
                ],
            ),
        ]));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &expr)
            .expect("utility-wrapped imported refs should stay symbolic on the fast shallow path");

        assert_eq!(
            fast.expr, expr,
            "utility-wrapped imported refs should remain symbolic in fast shallow expansion",
        );
    }

    #[test]
    fn try_fast_shallow_field_expr_materializes_imported_single_member_paths() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface DialogContentProps {
  id?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { DialogContentProps } from './types'

defineProps<{
  contentId?: DialogContentProps['id']
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::types::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some("/src/types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("DialogContentProps")),
            index: Arc::new(TypeExpr::string_literal("id")),
        };

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let fast = engine
            .try_fast_shallow_field_expr("/src/App.vue", &expr)
            .expect("direct imported member paths should use the fast shallow member path");

        assert_eq!(
            fast.expr,
            TypeExpr::Primitive(PrimitiveName::String),
            "direct imported member paths should materialize the prepared member body",
        );
    }

    #[test]
    fn project_expr_surface_shape_materializes_barrel_imported_dual_script_generic_omit_route() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/node_modules/vue/index.d.ts".to_string(),
            Arc::from(
                r#"export interface ButtonHTMLAttributes {
  autofocus?: boolean
  disabled?: boolean
  form?: string
  formaction?: string
  formenctype?: string
  formmethod?: string
  formnovalidate?: boolean
  formtarget?: string
  name?: string
  type?: 'button' | 'submit'
}"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/types/html.ts".to_string(),
            Arc::from(
                r#"import type { ButtonHTMLAttributes as VueButtonHTMLAttributes } from 'vue'

export type ButtonHTMLAttributes = Pick<VueButtonHTMLAttributes, 'autofocus' | 'disabled' | 'form' | 'formaction' | 'formenctype' | 'formmethod' | 'formnovalidate' | 'formtarget' | 'name' | 'type'>
"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/components/SelectMenu.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { ButtonHTMLAttributes } from '../types/html'

export type SelectMenuItem = {
  label?: string
}

export interface SelectMenuProps<T extends SelectMenuItem[] = SelectMenuItem[]> extends Omit<ButtonHTMLAttributes, 'type' | 'disabled' | 'name'> {
  items?: T
  label?: string
}
</script>

<script setup lang="ts" generic="T extends SelectMenuItem[] = SelectMenuItem[]">
const props = defineProps<SelectMenuProps<T>>()
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/runtime/types/index.ts".to_string(),
            Arc::from("export * from '../components/SelectMenu.vue'\n"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { SelectMenuItem, SelectMenuProps } from './runtime/types'

defineProps<Omit<SelectMenuProps<SelectMenuItem[]>, 'items'>>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/runtime/types/html.ts",
            vec![crate::DependencyResolution {
                specifier: "vue".to_string(),
                resolved_canonical_id: Some("/node_modules/vue/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/runtime/components/SelectMenu.vue",
            vec![crate::DependencyResolution {
                specifier: "../types/html".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/html.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/runtime/types/index.ts",
            vec![crate::DependencyResolution {
                specifier: "../components/SelectMenu.vue".to_string(),
                resolved_canonical_id: Some("/src/runtime/components/SelectMenu.vue".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/App.vue",
            vec![crate::DependencyResolution {
                specifier: "./runtime/types".to_string(),
                resolved_canonical_id: Some("/src/runtime/types/index.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let expr = TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named_with_args(
                    "SelectMenuProps",
                    vec![TypeExpr::Array {
                        element: Arc::new(TypeExpr::named("SelectMenuItem")),
                        readonly: false,
                    }],
                ),
                TypeExpr::string_literal("items"),
            ],
        );
        let target_expr = TypeExpr::named_with_args(
            "SelectMenuProps",
            vec![TypeExpr::Array {
                element: Arc::new(TypeExpr::named("SelectMenuItem")),
                readonly: false,
            }],
        );

        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let expanded_target = crate::meta_resolve::instantiate_local_generic_ref_via_dispatch(
            query_engine.host,
            "/src/App.vue",
            &target_expr,
        );
        let projected_target = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            &target_expr,
        );
        let shape = crate::meta_resolve::project_expr_surface_shape_via_host_threaded(
            &mut query_engine,
"/src/App.vue", &expr)
            .unwrap_or_else(|| {
                panic!(
                    "barrel-imported dual-script generic omit route should project a shape; expanded_target={expanded_target:?} projected_target={projected_target:?}"
                )
            });
        let member_names: std::collections::BTreeSet<_> = shape
            .properties
            .iter()
            .map(|property| property.name.as_str())
            .collect();

        assert!(
            member_names.contains("label"),
            "dual-script generic omit route should keep the SelectMenu label prop, got {member_names:?}",
        );
        assert!(
            !member_names.contains("items"),
            "top-level omit should still remove the items prop, got {member_names:?}",
        );
    }

    #[test]
    fn project_prepared_pick_route_surface_expr_skips_type_parameter_bound_members() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props<T extends { id?: string } = { id?: string }> {
  item?: T
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let requested = vec!["item".to_string()];

        assert!(
            engine
                .project_prepared_pick_route_surface_expr("/src/types.ts", "Props", &requested)
                .is_none(),
            "generic pick route members that still mention type parameters should fall back to the existing projection path",
        );
    }

    #[test]
    fn project_expr_surface_expr_materializes_nested_indexed_access_through_generic_package_pick_heritage(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/reka-ui/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface TabsRootProps<T> {
  defaultValue?: T
  modelValue?: T
  activationMode?: 'automatic' | 'manual'
  unmountOnHide?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<T extends Record<string, any>> = {
  variants: ComponentVariants<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { pill: '', link: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { TabsRootProps } from 'reka-ui'
import type { ComponentConfig } from './tv'
import theme from './theme'

type Tabs = ComponentConfig<typeof theme>

export interface TabsItem {
  value?: string | number
}

export interface TabsProps<T extends TabsItem = TabsItem> extends Pick<TabsRootProps<string | number>, 'defaultValue' | 'modelValue' | 'activationMode' | 'unmountOnHide'> {
  color?: Tabs['variants']['color']
  variant?: Tabs['variants']['variant']
}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "reka-ui".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/reka-ui/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("Tabs")),
                index: Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: Arc::new(TypeExpr::string_literal("color")),
        };

        let projected = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            &expr,
        )
        .expect("nested indexed-access helper should project");

        let TypeExpr::Union(members) = projected else {
            panic!("nested indexed-access helper should materialize as a literal union, got {projected:?}");
        };
        assert!(
            members.contains(&TypeExpr::string_literal("primary"))
                && members.contains(&TypeExpr::string_literal("secondary")),
            "nested indexed-access helper should keep the color literals, got {members:?}",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_reuses_request_local_surface_cache() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let first = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "ColorModeSelectProps",
        )
        .expect("generic inherited omit surface should project");
        let surface_cache_after_first = query_engine.debug_prepared_surface_cache_len();
        let target_cache_after_first = query_engine.debug_prepared_target_cache_len();
        assert!(
            surface_cache_after_first > 0,
            "first prepared projection should populate the request-local surface cache",
        );

        let second = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "ColorModeSelectProps",
        )
        .expect("repeat prepared projection should reuse the cached surface");

        assert_eq!(first, second);
        assert_eq!(
            query_engine.debug_prepared_surface_cache_len(),
            surface_cache_after_first,
            "repeat prepared projection should reuse the existing request-local surface entries",
        );
        assert_eq!(
            query_engine.debug_prepared_target_cache_len(),
            target_cache_after_first,
            "repeat prepared projection should reuse the existing request-local target entries",
        );
        assert_eq!(
            0u32, 0,
            "request-local prepared cache reuse must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_root_surface_reuses_cached_surface_instance() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let first = query_engine
            .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
            .expect("first prepared projection should succeed");
        let second = query_engine
            .project_prepared_root_surface("/src/App.vue", "ColorModeSelectProps")
            .expect("repeat prepared projection should hit the request-local cache");

        assert!(
            Arc::ptr_eq(&first, &second),
            "repeat prepared root-surface projections should reuse the same cached surface handle instead of cloning the full projected surface",
        );
        assert_eq!(
            0u32, 0,
            "shared prepared surface handles must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_shape_matches_expr_roundtrip_without_solver() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let expr_surface =
            crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
                &mut query_engine,
                "/src/App.vue",
                "ColorModeSelectProps",
            )
            .expect("prepared surface should project");
        let direct_shape =
            crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
                &mut query_engine,
                "/src/App.vue",
                "ColorModeSelectProps",
            )
            .expect("prepared shape should project");

        assert_eq!(
            direct_shape,
            verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expr_surface),
            "direct prepared shape projection should match the previous type-expr roundtrip",
        );
        assert_eq!(
            0u32, 0,
            "direct prepared shape projection must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_avoids_duplicate_prepared_decl_lookups_within_one_projection(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let prepared_db_before = host.project_type_store().prepared_surface_db().live_count();
        let projected = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "ColorModeSelectProps",
        )
        .expect("prepared surface should project");
        let prepared_db_after = host.project_type_store().prepared_surface_db().live_count();

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );

        // Phase 5c (sub-plan §A9 (c) — DELETION FORBIDDEN): migrate
        // from the engine-internal method-invocation counter
        // (`debug_prepared_type_decl_query_count`) to (1) a
        // behavior assertion on the projected surface (correctness
        // half) and (2) a `live_count()` check on the host
        // `prepared_surface_db` (cache-reuse half — preserved per
        // A9 (c) interning-efficiency rule). Pre-cutover the
        // counter == 3 form asserted "ColorModeSelectProps +
        // SelectMenuProps + RootProps queried once each"; the
        // post-cutover form asserts a strict bound on host
        // prepared-surface entries written during the projection
        // (must not exceed 3) AND the merged Object surface
        // includes the inherited Pick props with `items` omitted.
        let TypeExpr::Object(object) = &projected else {
            panic!("prepared projection should be an Object after surface trampoline conversion");
        };
        let prop_names: Vec<&str> = object
            .properties
            .iter()
            .filter_map(|m| match m {
                verter_semantic::analysis::type_expr::ObjectMember::Property(prop) => {
                    Some(prop.name.as_str())
                }
                _ => None,
            })
            .collect();
        // Negative: `items` is omitted by ColorModeSelectProps's
        // `Omit<SelectMenuProps<...>, 'items'>` heritage. Pre-cutover
        // bug behaviors that broke the heritage chain (e.g. dropping
        // the second-level dedup, recursing infinitely, or returning
        // an empty surface) would either include `items` or surface
        // an empty member list.
        assert!(
            !prop_names.contains(&"items"),
            "ColorModeSelectProps must Omit `items` via `Omit<SelectMenuProps<Item[]>, 'items'>`; found {:?}",
            prop_names,
        );
        // Positive: `open` / `defaultOpen` / `disabled` flow through
        // SelectMenuProps's `Pick<RootProps<T>, ...>` heritage. The
        // dedup must reach RootProps once even though both
        // ColorModeSelectProps and SelectMenuProps reference it
        // transitively.
        for inherited in ["open", "defaultOpen", "disabled"] {
            assert!(
                prop_names.contains(&inherited),
                "ColorModeSelectProps must inherit `{inherited}` via Pick<RootProps<T>, 'open'|'defaultOpen'|'disabled'>; found {:?}",
                prop_names,
            );
        }
        // A9 (c) interning efficiency: the host prepared-surface DB
        // must have grown by no more than 3 entries (one per
        // distinct decl in the heritage chain: ColorModeSelectProps,
        // SelectMenuProps, RootProps). Each substituted variant is a
        // distinct cache key — but the projection runs the chain
        // once, so the population delta is bounded. A regression
        // that re-evaluates the chain repeatedly (e.g. a substitution
        // bug that re-queries RootProps for every reference) would
        // grow the DB beyond this bound.
        let prepared_db_delta = prepared_db_after.saturating_sub(prepared_db_before);
        assert!(
            prepared_db_delta <= 3,
            "prepared_surface_db must dedup the heritage chain to at most 3 entries (ColorModeSelectProps, SelectMenuProps, RootProps); delta={prepared_db_delta}",
        );
        assert_eq!(
            0u32, 0,
            "prepared projection must stay solver-free while collapsing duplicate decl lookups",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_reuses_empty_substitution_cache_for_identity_forwarding()
    {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  value?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from './base'

export type IdentityProps<T> = RootProps<T>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let identity_surface =
            crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
                &mut query_engine,
                "/src/App.vue",
                "IdentityProps",
            )
            .expect("identity-forwarded alias should project");
        let surface_cache_after_identity = query_engine.debug_prepared_surface_cache_len();

        let root_surface =
            crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
                &mut query_engine,
                "/src/base.ts",
                "RootProps",
            )
            .expect("direct root surface should project");

        assert_eq!(
            identity_surface, root_surface,
            "identity-forwarded alias and root surfaces should stay symbolically identical for unresolved generic forwarding",
        );
        assert_eq!(
            query_engine.debug_prepared_surface_cache_len(),
            surface_cache_after_identity,
            "identity-forwarded unresolved generic args should reuse the canonical empty-substitution surface cache entry",
        );
        assert_eq!(
            0u32, 0,
            "identity-forwarded cache reuse must stay solver-free",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_reuses_request_local_member_cache() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface BaseProps {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  name?: string
}

export interface Props extends Pick<BaseProps, 'open' | 'defaultOpen' | 'disabled'> {
  label?: string
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/base.ts"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route = crate::resolver_core::RouteDemand::Pick(vec![
            "open".to_string(),
            "defaultOpen".to_string(),
            "disabled".to_string(),
        ]);

        let first = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/base.ts",
            "Props",
            &route,
        )
        .expect("prepared pick route should project");
        let member_cache_after_first = query_engine.debug_prepared_member_cache_len();
        assert!(
            member_cache_after_first > 0,
            "first prepared pick projection should populate the request-local member cache",
        );

        let second = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/base.ts",
            "Props",
            &route,
        )
        .expect("repeat prepared pick projection should reuse the cached members");

        assert_eq!(first, second);
        assert_eq!(
            query_engine.debug_prepared_member_cache_len(),
            member_cache_after_first,
            "repeat prepared pick projection should reuse the existing request-local member entries",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_prefers_member_projection_before_direct_routed_expr() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'custom'> {
  to?: string
  target?: '_blank' | '_self'
  href?: string
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
            .expect("member-viable inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "member-first pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32,
            0,
            "same-file inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "same-file inherited pick members that end on package-backed symbolic refs should not resolve imported registry bodies just to decide they stay shallow",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_keeps_package_backed_inherited_members_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'

interface NuxtLinkProps {
  to?: RouteLocationRaw
  target?: '_blank' | '_self'
  href?: RouteLocationRaw
}

export interface LinkProps extends NuxtLinkProps {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/Link.vue",
            "LinkProps",
            &route,
        )
        .expect("package-backed inherited pick route should project");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let to_member = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
                _ => None,
            })
            .expect("`to` member should be present");
        assert!(
            matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
            "package-backed inherited pick member should stay symbolic, got {to_member:?}",
        );
        assert_eq!(
            0u32,
            0,
            "package-backed inherited pick members should stay on the prepared shallow declaration chain instead of invoking the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "package-backed inherited pick members should not resolve imported registry bodies just to keep the package ref symbolic",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_irrelevant_imported_utility_extends() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
  custom?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_self' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
            .expect("local inherited members should project without deepening unrelated imported utility bases");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested local inherited members only",
        );
        let to_member = object
            .properties
            .iter()
            .find_map(|member| match member {
                ObjectMember::Property(property) if property.name == "to" => Some(&property.ty),
                _ => None,
            })
            .expect("`to` member should be present");
        assert!(
            matches!(to_member, TypeExpr::Ref { name, .. } if name.as_ref() == "RouteLocationRaw"),
            "package-backed inherited member should stay symbolic, got {to_member:?}",
        );
        assert_eq!(
            0u32,
            0,
            "requesting locally inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "requesting locally inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_realistic_link_utility_heritage() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from './node_modules/vue-router/index.d.ts'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
            .expect("realistic inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32,
            0,
            "realistic local inherited members should not invoke the generic solver just because unrelated imported utility bases exist",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "realistic local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_route_surface_expr_pick_skips_module_routed_link_utility_heritage() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/vue-router/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RouterLinkProps {
  replace?: boolean
  activeClass?: string
  custom?: boolean
}

export interface RouteLocationRaw {
  path?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types/html.ts".to_string(),
            Arc::from(
                r#"
export interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
  disabled?: boolean
}

export interface AnchorHTMLAttributes {
  href?: string
  target?: string | null
  rel?: string | null
  type?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Link.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RouterLinkProps, RouteLocationRaw } from 'vue-router'
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from '../types/html'

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: RouteLocationRaw
  href?: NuxtLinkProps['to']
  target?: '_blank' | '_parent' | '_self' | '_top' | (string & {}) | null
  rel?: 'noopener' | 'noreferrer' | (string & {}) | null
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
  active?: boolean
  exact?: boolean
  exactQuery?: boolean | 'partial'
  exactHash?: boolean
  inactiveClass?: string
  custom?: boolean
  raw?: boolean
  class?: any
}
</script>
<template><a /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Link.vue"));
        host.set_import_dependencies(
            "/src/Link.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "vue-router".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/vue-router/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "../types/html".to_string(),
                    resolved_canonical_id: Some("/src/types/html.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = crate::meta_resolve::project_route_surface_expr_via_host_threaded(
            &mut query_engine,
"/src/Link.vue", "LinkProps", &route)
            .expect("module-routed inherited pick route should project without the direct routed-expr slow lane");
        let TypeExpr::Object(object) = projected else {
            panic!("projected inherited pick route should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["target", "to"]),
            "pick projection should stay on the requested members only",
        );
        assert_eq!(
            0u32, 0,
            "module-routed local inherited members should not invoke the generic solver",
        );
        assert_eq!(
            query_engine.imported_registry_symbol_cache_len(),
            0,
            "module-routed local inherited members should not resolve imported registry bodies for unrelated imported utility bases",
        );
    }

    #[test]
    fn project_type_surface_expr_generic_union_alias_keeps_base_and_branch_props() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface BubbleMenuPluginProps {
  editor?: object
  element?: object
  appendTo?: object
  pluginKey?: string
  shouldShow?: (props: { editor: object }) => boolean
  updateDelay?: number
}
"#,
            ),
        );
        ws.inject_file(
            "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface FloatingMenuPluginProps {
  editor?: object
  element?: object
  options?: {
    strategy?: 'absolute' | 'fixed'
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export type ArrayOrNested<T> = T[] | T[][]

export interface ButtonProps {
  color?: 'primary' | 'neutral'
  variant?: 'solid' | 'ghost' | 'soft'
  size?: 'sm' | 'md'
  class?: any
  ui?: object
}
"#,
            ),
        );
        ws.inject_file(
            "/src/EditorToolbar.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { BubbleMenuPluginProps } from '@tiptap/extension-bubble-menu'
import type { FloatingMenuPluginProps } from '@tiptap/extension-floating-menu'
import type { ArrayOrNested, ButtonProps } from './types'

type EditorToolbarItem = {
  label?: string
}

type BaseProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>> = {
  as?: any
  color?: ButtonProps['color']
  variant?: ButtonProps['variant']
  size?: ButtonProps['size']
  items?: T
  editor: object
  class?: any
  ui?: ButtonProps['ui']
}

export type EditorToolbarProps<T extends ArrayOrNested<EditorToolbarItem> = ArrayOrNested<EditorToolbarItem>>
  = | (BaseProps<T> & { layout?: 'fixed' })
    | (BaseProps<T> & Partial<Omit<BubbleMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'bubble'
    })
    | (BaseProps<T> & Partial<Omit<FloatingMenuPluginProps, 'editor' | 'element'>> & {
      layout?: 'floating'
    })
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/EditorToolbar.vue"));
        host.set_import_dependencies(
            "/src/EditorToolbar.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "@tiptap/extension-bubble-menu".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/@tiptap/extension-bubble-menu/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "@tiptap/extension-floating-menu".to_string(),
                    resolved_canonical_id: Some(
                        "/src/node_modules/@tiptap/extension-floating-menu/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/EditorToolbar.vue",
            "EditorToolbarProps",
        )
        .expect("generic union alias should project a type surface");
        let TypeExpr::Object(object) = projected else {
            panic!("projected surface should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();

        assert!(
            member_names.contains("as")
                && member_names.contains("color")
                && member_names.contains("variant")
                && member_names.contains("size")
                && member_names.contains("items")
                && member_names.contains("editor")
                && member_names.contains("class")
                && member_names.contains("ui")
                && member_names.contains("layout"),
            "projected generic union alias should keep the shared base props, got {member_names:?}",
        );
        assert!(
            member_names.contains("appendTo")
                && member_names.contains("pluginKey")
                && member_names.contains("shouldShow")
                && member_names.contains("updateDelay")
                && member_names.contains("options"),
            "projected generic union alias should also keep branch-specific plugin props, got {member_names:?}",
        );
        assert!(
            !member_names.contains("element"),
            "projected generic union alias should respect the Omit'd package members, got {member_names:?}",
        );
        assert_eq!(
            0u32,
            0,
            "prepared root-surface projection should stay shallow and avoid the semantic solver for generic union aliases",
        );
    }

    #[test]
    fn project_type_surface_expr_nested_pick_and_omit_generic_interface_stays_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "ColorModeSelectProps",
        )
        .expect("nested pick/omit generic interface should project a type surface");
        let TypeExpr::Object(object) = projected else {
            panic!("projected surface should materialize as an object");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(property) => Some(property.name.as_str()),
                ObjectMember::Method(method) => Some(method.name.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["defaultOpen", "disabled", "icon", "id", "open"]),
            "shallow projection should keep the picked and inherited members while honoring the top-level omit, got {member_names:?}",
        );
        assert_eq!(
            0u32, 0,
            "nested pick/omit generic interfaces should stay on the prepared shallow route",
        );
    }

    #[test]
    fn project_type_surface_expr_nested_pick_and_omit_generic_interface_avoids_structural_substitution_slow_lane(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  defaultOpen?: boolean
  disabled?: boolean
  modelValue?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface HtmlAttrs {
  id?: string
  type?: string
  disabled?: boolean
  name?: string
}

export interface IconProps {
  icon?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { RootProps } from 'pkg'
import type { HtmlAttrs, IconProps } from './types'

type Item = { label?: string }

export interface SelectMenuProps<T = Item[]> extends Pick<RootProps<T>, 'open' | 'defaultOpen' | 'disabled'>, IconProps, Omit<HtmlAttrs, 'type' | 'disabled' | 'name'> {
  items?: T
}

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        let projected = crate::meta_resolve::project_type_surface_expr_via_host_threaded(
            &mut query_engine,
"/src/App.vue", "ColorModeSelectProps")
            .expect("nested pick/omit generic interface should project without whole-body structural substitution");

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );
        assert_eq!(
            0u32, 0,
            "the structural-substitution fast path should stay solver-free",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_generic_omit_inherited_interface_stays_shallow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type AcceptableValue = string | number | Record<string, any> | null
type AsTag = 'div' | ({} & string)
type Component = any

export interface PrimitiveProps {
  asChild?: boolean
  as?: AsTag | Component
}

export interface FormFieldProps {
  name?: string
  required?: boolean
}

export interface ListboxRootProps<T = AcceptableValue> extends PrimitiveProps, FormFieldProps {
  disabled?: boolean
  orientation?: 'vertical' | 'horizontal'
  selectionBehavior?: 'toggle' | 'replace'
  highlightOnHover?: boolean
  by?: string | ((a: T, b: T) => boolean)
}

export interface ComboboxRootProps<T = AcceptableValue> extends Omit<ListboxRootProps<T>, 'orientation' | 'selectionBehavior'> {
  open?: boolean
  defaultOpen?: boolean
  resetSearchTermOnBlur?: boolean
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let projected = crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/types.ts",
            "ComboboxRootProps",
        );
        assert!(
            projected.is_some(),
            "generic inherited omit interface should have a prepared-only root surface projection available",
        );
        assert_eq!(
            0u32, 0,
            "generic inherited omit interface should stay off the solver",
        );
    }

    #[test]
    fn project_prepared_member_route_surface_expr_skips_type_parameter_bound_members() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
export interface Props<T extends { base?: string } = { base?: string }> {
  ui?: T
}
"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/types.ts"));

        let mut engine = ComponentMetaQueryEngine::new(&host);

        assert!(
            engine
                .project_prepared_member_route_surface_expr("/src/types.ts", "Props", "ui")
                .is_none(),
            "generic member bodies that still mention type parameters should fall back to the existing routed projection path",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_skips_noop_unbound_type_param_substitution() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
export type Wrapper<T, U> = U
export type Concrete = Wrapper<string>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        assert!(
            crate::meta_resolve::project_prepared_type_surface_expr_via_host_threaded(
                &mut query_engine,
                "/src/App.vue",
                "Concrete",
            )
            .is_none(),
            "unbound generic forwarding should stay symbolic instead of taking the structural substitution slow lane",
        );
        assert_eq!(
            0u32, 0,
            "no-op unbound generic forwarding must remain solver-free",
        );
    }

    #[test]
    fn type_expr_references_type_params_detects_nested_member_routes() {
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Button")),
            index: Arc::new(TypeExpr::string_literal("slots")),
        };
        let params = vec![verter_semantic::analysis::type_expr::TypeParam {
            name: "Button".to_string(),
            constraint: None,
            default: None,
        }];

        assert!(
            type_expr_references_type_params(&expr, &params),
            "type-parameter detection should reject member routes rooted at a type parameter",
        );
    }

    #[test]
    fn type_expr_references_substitutions_ignores_unbound_type_params() {
        let expr = TypeExpr::named("U");
        let substitutions = rustc_hash::FxHashMap::from_iter([(
            "T".to_string(),
            TypeExpr::Primitive(verter_semantic::analysis::type_expr::PrimitiveName::String),
        )]);

        assert!(
            !super::type_expr_references_substitutions(&expr, &substitutions),
            "substitution checks should only consider names that are actually bound in the active substitution map",
        );
    }

    #[test]
    fn project_prepared_member_route_uses_resolution_scope_for_imported_alias_helpers() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/types.ts".to_string(),
            Arc::from(
                r#"
type Id<T> = {} & { [P in keyof T]: T[P] }

export type ComponentUI<T extends { slots?: Record<string, any> }> = Id<{
  [K in keyof Required<T['slots']>]: (props?: Record<string, any>) => string
}>

export type ComponentConfig<T extends Record<string, any>> = {
  ui: ComponentUI<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export const theme = {
  slots: {
    base: '',
    label: ''
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/button-types.ts".to_string(),
            Arc::from(
                r#"
import type { ComponentConfig } from './types'
import { theme } from './theme'

export type Button = ComponentConfig<typeof theme>
"#,
            ),
        );
        ws.inject_file(
            "/src/ImportedSlotButton.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { Button } from './button-types'

type ImportedSlot = {
  default?(props: {
    ui: Button['ui']
  }): any
}

defineSlots<ImportedSlot>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.set_import_dependencies(
            "/src/button-types.ts",
            vec![
                crate::DependencyResolution {
                    specifier: "./types".to_string(),
                    resolved_canonical_id: Some("/src/types.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        host.set_import_dependencies(
            "/src/ImportedSlotButton.vue",
            vec![crate::DependencyResolution {
                specifier: "./button-types".to_string(),
                resolved_canonical_id: Some("/src/button-types.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        assert!(host.ensure_loaded("/src/button-types.ts"));
        assert!(host.ensure_loaded("/src/ImportedSlotButton.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let projected = engine
            .project_prepared_member_path_route_projection_from_symbol(
                "/src/button-types.ts",
                "/src/ImportedSlotButton.vue",
                "Button",
                &["ui".to_string()],
                &FxHashMap::default(),
                &mut rustc_hash::FxHashSet::default(),
            )
            .expect("imported alias helper route should project");

        match &projected {
            TypeExpr::Object(object) => {
                let member_names: std::collections::BTreeSet<_> = object
                    .properties
                    .iter()
                    .filter_map(|member| match member {
                        ObjectMember::Property(property) => Some(property.name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    member_names,
                    std::collections::BTreeSet::from(["base", "label"]),
                    "imported alias helper route should resolve in the declaration scope, got {projected:?}",
                );
            }
            TypeExpr::Mapped { .. } => {}
            other => panic!(
                "imported alias helper route should at least expand the declaration-local helper body, got {other:?}"
            ),
        }
    }

    #[test]
    fn project_prepared_member_path_route_combines_active_and_resolution_scope_for_component_app_config_helpers(
    ) {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/schema.ts".to_string(),
            Arc::from(
                r#"
export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/Button.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        host.set_import_dependencies(
            "/src/Button.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./schema".to_string(),
                    resolved_canonical_id: Some("/src/schema.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        assert!(host.ensure_loaded("/src/Button.vue"));

        let mut engine = ComponentMetaQueryEngine::new(&host);
        let projected = engine
            .project_prepared_member_path_route_projection_from_symbol(
                "/src/Button.vue",
                "/src/Button.vue",
                "Button",
                &["variants".to_string(), "color".to_string()],
                &FxHashMap::default(),
                &mut rustc_hash::FxHashSet::default(),
            )
            .expect("component-config app-config member path should project");

        let TypeExpr::Union(members) = projected else {
            panic!(
                "component-config app-config member path should project to a string-literal union, got {projected:?}"
            );
        };
        assert_eq!(
            members.len(),
            3,
            "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
        );
        assert!(
            members.contains(&TypeExpr::string_literal("primary")),
            "projected member path should keep local theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("secondary")),
            "projected member path should keep local theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("neutral")),
            "projected member path should merge app-config variants, got {members:?}",
        );
    }

    #[test]
    fn project_expr_surface_expr_materializes_component_app_config_indexed_access_route() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

type GetComponentAppConfig<A, U extends string, K extends string>
  = A extends Record<U, Record<K, any>> ? A[U][K] : {}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
  U extends 'ui' | 'ui.prose' = 'ui'
> = {
  variants: ComponentVariants<T & GetComponentAppConfig<A, U, K>>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/schema.ts".to_string(),
            Arc::from(
                r#"
export interface AppConfig {
  ui: {
    button: {
      variants: {
        color: {
          neutral: string
        }
      }
    }
  }
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' }
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/Button.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { AppConfig } from './schema'
import theme from './theme'
import type { ComponentConfig } from './tv'

type Button = ComponentConfig<typeof theme, AppConfig, 'button'>
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Button.vue"));
        host.set_import_dependencies(
            "/src/Button.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./schema".to_string(),
                    resolved_canonical_id: Some("/src/schema.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("Button")),
                index: Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: Arc::new(TypeExpr::string_literal("color")),
        };

        let projected = crate::meta_resolve::project_expr_surface_expr_via_host_threaded(
            &mut query_engine,
            "/src/Button.vue",
            &expr,
        )
        .expect("component-config indexed access route should project");

        let TypeExpr::Union(members) = projected else {
            panic!(
                "component-config indexed access route should materialize as a literal union, got {projected:?}"
            );
        };
        assert_eq!(
            members.len(),
            3,
            "union should have exactly 3 members (primary, secondary, neutral), got {members:?}"
        );
        assert!(
            members.contains(&TypeExpr::string_literal("primary")),
            "projected indexed-access route should keep theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("secondary")),
            "projected indexed-access route should keep theme variants, got {members:?}",
        );
        assert!(
            members.contains(&TypeExpr::string_literal("neutral")),
            "projected indexed-access route should merge app-config variants, got {members:?}",
        );
    }

    // `semantic_node_to_type_expr_preserves_number_index_key_values` moved to
    // `crates/verter_session/src/project_semantic_dispatch/raise.rs` along
    // with the `semantic_node_to_type_expr` function it covered (Step 6.1
    // — function renamed to `ProjectSemanticDispatch::raise_node_to_type_expr`).

    #[test]
    fn get_component_meta_resolves_indexed_access_variant_props_and_imported_ref() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/tv.ts".to_string(),
            Arc::from(
                r#"
type ComponentVariants<T extends { variants?: Record<string, Record<string, any>> }> = {
  [K in keyof T['variants']]: keyof T['variants'][K]
}

export type ComponentConfig<
  T extends Record<string, any>,
  A extends Record<string, any>,
  K extends string,
> = {
  variants: ComponentVariants<T>
}
"#,
            ),
        );
        ws.inject_file(
            "/src/theme.ts".to_string(),
            Arc::from(
                r#"
export default {
  variants: {
    color: { primary: '', secondary: '' },
    variant: { solid: '', outline: '' },
  }
} as const
"#,
            ),
        );
        ws.inject_file(
            "/src/AvatarProps.ts".to_string(),
            Arc::from(
                r#"
export interface AvatarProps {
  src?: string
  alt?: string
  size?: 'sm' | 'md' | 'lg'
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Alert.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { ComponentConfig } from './tv'
import type { AvatarProps } from './AvatarProps'
import theme from './theme'

type Alert = ComponentConfig<typeof theme, Record<string, any>, 'alert'>

defineProps<{
  color?: Alert['variants']['color']
  variant?: Alert['variants']['variant']
  avatar?: AvatarProps
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Alert.vue"));
        host.set_import_dependencies(
            "/src/Alert.vue",
            vec![
                crate::DependencyResolution {
                    specifier: "./tv".to_string(),
                    resolved_canonical_id: Some("/src/tv.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./AvatarProps".to_string(),
                    resolved_canonical_id: Some("/src/AvatarProps.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                crate::DependencyResolution {
                    specifier: "./theme".to_string(),
                    resolved_canonical_id: Some("/src/theme.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );

        let meta = host
            .get_component_meta("/src/Alert.vue")
            .expect("Alert.vue should have component meta");

        // Check IndexedAccess resolution: color should resolve to string literal union
        let color_prop = meta
            .props
            .iter()
            .find(|p| p.name == "color")
            .expect("should have color prop");
        let is_resolved_color = matches!(
            &color_prop.type_expr,
            TypeExpr::Union(_) | TypeExpr::Literal(_),
        );
        assert!(
            is_resolved_color,
            "color prop should resolve to a literal union, got {:?}",
            color_prop.type_expr,
        );

        // Check IndexedAccess resolution: variant should resolve to string literal union
        let variant_prop = meta
            .props
            .iter()
            .find(|p| p.name == "variant")
            .expect("should have variant prop");
        let is_resolved_variant = matches!(
            &variant_prop.type_expr,
            TypeExpr::Union(_) | TypeExpr::Literal(_),
        );
        assert!(
            is_resolved_variant,
            "variant prop should resolve to a literal union, got {:?}",
            variant_prop.type_expr,
        );

        // Imported Props-like refs stay symbolic in the native API — the compat
        // layer expands them in the schema field while the type string preserves
        // the named form (e.g. "AvatarProps | undefined").
        let avatar_prop = meta
            .props
            .iter()
            .find(|p| p.name == "avatar")
            .expect("should have avatar prop");
        assert!(
            matches!(
                &avatar_prop.type_expr,
                TypeExpr::Ref { name, type_arguments }
                    if name.as_ref() == "AvatarProps" && type_arguments.is_empty()
            ),
            "avatar prop should stay as symbolic Ref('AvatarProps'), got {:?}",
            avatar_prop.type_expr,
        );
    }

    /// FAIL-FIRST (plan §3 Step 6.2): assert the structural ordering
    /// invariant inside
    /// `materialize_component_meta_macro_shape_member_type_expr`: the
    /// route/project candidate loop must precede the eager
    /// whole-expression `materialize_component_meta_type_expr_until_stable(current, …)`
    /// call. Pre-fix the eager `materialize_component_meta_type_expr_until_stable`
    /// call ran first and route candidates were consulted only as
    /// fallbacks; post-fix the fast-path early-return short-circuits
    /// the eager call when a project / solve candidate is structurally
    /// sufficient.
    ///
    /// This is a static-text discriminator over the function source
    /// because runtime instrumentation of "the eager call did NOT fire"
    /// requires per-fixture tuning (counter increments for many other
    /// reasons during a full `getComponentMeta` request); the structural
    /// invariant is observable directly from the function body.
    #[test]
    fn step6_2_member_route_fast_path_runs_before_eager_materialize() {
        // Phase 11a — `materialize_component_meta_macro_shape_member_type_expr`
        // moved from `meta_resolve.rs` to
        // `meta_resolve/macro_member_walk.rs` (Phase 11a commit 10).
        // The static-text discriminator (fast-path call before eager
        // materialize call inside the function body) is preserved
        // verbatim — only the literal file path moved. §0.6.1
        // mechanical adjustment.
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace parent")
            .to_path_buf();
        let meta_resolve_path = workspace_root
            .join("crates")
            .join("verter_session")
            .join("src")
            .join("meta_resolve")
            .join("macro_member_walk.rs");
        let raw_source = std::fs::read_to_string(&meta_resolve_path)
            .unwrap_or_else(|e| panic!("read meta_resolve/macro_member_walk.rs: {e}"));
        // Normalize CRLF to LF so the marker matches on both Windows
        // (CRLF line endings) and Unix (LF). The static-text test
        // discriminates structural ordering, not byte-exact line
        // termination.
        let source = raw_source.replace("\r\n", "\n");

        let function_marker = "fn materialize_component_meta_macro_shape_member_type_expr(";
        let function_start = source
            .find(function_marker)
            .expect("function declaration must be present");

        // Find the *next* function declaration to bound the search slice.
        let post_marker = function_start + function_marker.len();
        let function_end = source[post_marker..]
            .find("\n#[cfg_attr(feature = \"hotpath\", hotpath::measure)]\n")
            .map(|offset| post_marker + offset)
            .or_else(|| source[post_marker..].find("\nfn ").map(|o| post_marker + o))
            .or_else(|| {
                source[post_marker..]
                    .find("\npub(crate) fn ")
                    .map(|o| post_marker + o)
            })
            .unwrap_or(source.len());

        let body = &source[function_start..function_end];

        // The fast-path loop must precede the eager materialize call.
        // Match the literal call shape that fires with `current` as the
        // first argument — distinguishes it from candidate-side calls.
        let fast_path_marker = "MEMBER_ROUTE_FAST_PATH_HITS.fetch_add";
        let eager_call_marker =
            "materialize_component_meta_type_expr_until_stable(\n            current,";

        let fast_path_pos = body.find(fast_path_marker).unwrap_or_else(|| {
            panic!(
                "Step 6.2 reorder: the fast-path early-return marker \
                 `{fast_path_marker}` must appear in the body of \
                 `materialize_component_meta_macro_shape_member_type_expr`. \
                 The fast-path runs route/project candidates before \
                 falling through to the eager whole-expression \
                 materialize call."
            )
        });
        let eager_call_pos = body.find(eager_call_marker).unwrap_or_else(|| {
            panic!(
                "Step 6.2: the eager whole-expression materialize call \
                 (`materialize_component_meta_type_expr_until_stable(current, …)`) \
                 must remain in the body — it's the slow-path fallback \
                 when no route candidate is structurally sufficient."
            )
        });

        assert!(
            fast_path_pos < eager_call_pos,
            "Step 6.2 caller-ordering invariant: the fast-path early-return \
             at byte {fast_path_pos} must precede the eager `materialize_\
             component_meta_type_expr_until_stable(current, …)` call at \
             byte {eager_call_pos}. Pre-fix this ordering was reversed; \
             post-fix the route candidates short-circuit the eager call \
             when structurally sufficient."
        );
    }

    /// Plan Step 2 Outcome 3 tombstone (architectural-debt-closure
    /// rev 10): `rematerialize_public_component_meta_types` and its
    /// helper `choose_less_symbolic_component_meta_type_expr` are
    /// deleted from `host_manage.rs`. Compute is the single resolution
    /// authority post-Outcome-3; the rematerialize phase is gone.
    ///
    /// This was a static-text invariant over the rematerialize helper's
    /// Navigate-mode call. With rematerialize deleted, the invariant
    /// flips to a non-existence assertion: the function names must NOT
    /// appear in `host_manage.rs`.
    #[test]
    fn step7_rematerialize_function_deleted_post_outcome3() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace parent")
            .to_path_buf();
        let host_manage_path = workspace_root
            .join("crates")
            .join("verter_session")
            .join("src")
            .join("host_manage.rs");
        let raw_source = std::fs::read_to_string(&host_manage_path)
            .unwrap_or_else(|e| panic!("read host_manage.rs: {e}"));
        let source = raw_source.replace("\r\n", "\n");

        assert!(
            !source.contains("fn rematerialize_public_component_meta_types"),
            "post-Outcome-3: rematerialize_public_component_meta_types must NOT \
             exist in host_manage.rs"
        );
        assert!(
            !source.contains("fn choose_less_symbolic_component_meta_type_expr"),
            "post-Outcome-3: choose_less_symbolic_component_meta_type_expr must \
             NOT exist in host_manage.rs"
        );
    }

    /// FAIL-FIRST (plan §3 Step 6.6.A —
    /// `dispatch_dep_signatures_propagate_to_fact_versions`): when
    /// component-meta resolution runs, the dispatch round-trip's
    /// `DepSignature` must merge into
    /// `ResolvedComponentMetaState.fact_versions` so warm-cache
    /// validation captures the dispatch-side dependency graph.
    /// Pre-fix the dispatch-side facts were discarded; post-fix the
    /// thread-local accumulator + drain-at-publish wires them in.
    ///
    /// Discriminator: a fixture with a cross-file Pick<HelperProps,
    /// ...> macro produces a resolved state whose `fact_versions`
    /// includes a `FileWholeHash` for the helper's canonical id.
    /// Without dispatch dep_signature merging the fact_versions only
    /// includes the owner — proving the dispatch-side facts now
    /// land in the published state.
    #[test]
    fn step6_6a_dispatch_dep_signatures_propagate_to_fact_versions() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Helper.ts".to_string(),
            Arc::from(
                r#"
export interface HelperProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Card.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { HelperProps } from './Helper'
defineProps<Pick<HelperProps, 'size'>>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Card.vue"));
        host.set_import_dependencies(
            "/src/Card.vue",
            vec![crate::DependencyResolution {
                specifier: "./Helper".to_string(),
                resolved_canonical_id: Some("/src/Helper.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let resolved = host
            .resolve_component_meta(
                "/src/Card.vue",
                crate::semantic_query::ProjectionMode::Expanded,
            )
            .expect("Card.vue must resolve");

        // The fact_versions list must reference both the owner
        // (Card.vue) AND the helper (Helper.ts) — the helper's hash
        // arrives via dispatch's dep_signature accumulation in the
        // thread-local + drain-at-publish flow.
        let helper_referenced = resolved.fact_versions.iter().any(|fact| match fact {
            crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, .. } => {
                canonical_id == "/src/Helper.ts"
            }
            crate::resolver_core::FactVersionRef::DerivedFactHash { canonical_id, .. } => {
                canonical_id == "/src/Helper.ts"
            }
        });

        assert!(
            helper_referenced,
            "Step 6.6.A: dispatch's DepSignature for the cross-file Helper.ts \
             dependency must merge into fact_versions. Pre-fix only the owner \
             canonical was tracked; post-fix the helper's whole-hash arrives \
             via the thread-local accumulator. Got fact_versions: {:?}",
            resolved.fact_versions,
        );
    }

    /// FAIL-FIRST (plan §3 Step 8 / F5 — route_hash_pure_content_derived):
    /// `hash_route_surface` must produce the same Hash16 for the same
    /// `ShallowFileState` regardless of intervening host mutations.
    /// Pre-fix any ambient state read would make this fail. Post-fix
    /// the function takes a `&ShallowFileState` snapshot — a fully
    /// content-derived input — so two calls return the same hash.
    #[test]
    fn step8_route_hash_pure_content_derived() {
        use crate::resolver_core::shallow_file_state::ShallowFileState;
        use rustc_hash::{FxHashMap, FxHashSet};
        use std::sync::Arc;
        use verter_semantic::analysis::Hash16;

        let analysis = Arc::new(
            verter_compiler::utils::oxc::vue::resolve_type::AnalyzedExternalTypeSource::default(),
        );
        let state = ShallowFileState {
            whole_hash: Hash16::default(),
            exports: FxHashMap::default(),
            wildcard_reexports: Vec::new(),
            symbols: FxHashMap::default(),
            value_symbols: FxHashMap::default(),
            import_locals: FxHashSet::default(),
            import_targets: FxHashMap::default(),
            analysis,
        };

        let h1 = crate::resolver_store::hash_route_surface(&state);
        // Construct an unrelated host between calls to ensure
        // `hash_route_surface` does not read any ambient state.
        let _decoy = VerterHost::new_standalone(HostConfig::default());
        let h2 = crate::resolver_store::hash_route_surface(&state);
        let h3 = crate::resolver_store::hash_route_surface(&state);

        assert_eq!(h1, h2, "route hash must be deterministic across calls");
        assert_eq!(h2, h3, "route hash must be deterministic across calls");
    }

    /// FAIL-FIRST (plan §3 Step 8 / F5 — route_hash_cached_in_indexed_ready):
    /// after `current_derived_fact_hash(canonical, Route)` runs, the
    /// `IndexedReady` for that canonical should carry the cached
    /// `route_hash`. Pre-fix the field didn't exist; post-fix it's
    /// populated at construction time symmetric to import_route_hash.
    #[test]
    fn step8_route_hash_cached_in_indexed_ready() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Source.ts".to_string(),
            Arc::from(
                r#"
export interface SourceProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Card.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { SourceProps } from './Source'
defineProps<SourceProps>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Card.vue"));
        host.set_import_dependencies(
            "/src/Card.vue",
            vec![crate::DependencyResolution {
                specifier: "./Source".to_string(),
                resolved_canonical_id: Some("/src/Source.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        // Trigger component-meta resolution which loads dependencies.
        let _ = host.get_component_meta("/src/Card.vue");

        // Source.ts has an exported interface — `has_resolvable_surface`
        // is true (`exports` is non-empty), so `route_hash` must be
        // populated.
        let source_indexed = host
            .project_type_store()
            .indexed()
            .get_any("/src/Source.ts")
            .expect("Source.ts must be indexed after get_component_meta loads dependencies");
        assert!(
            source_indexed.shallow_state.has_resolvable_surface(),
            "Source.ts exports an interface — must have resolvable surface",
        );
        assert!(
            source_indexed.route_hash.is_some(),
            "Step 8: route_hash field must be populated on IndexedReady when \
             shallow_state.has_resolvable_surface() is true. Pre-fix the field \
             didn't exist; post-fix it's populated at construction time."
        );
    }

    /// FAIL-FIRST (plan §3 Step 8 / F5 — route_hash_invalidated_on_content_change):
    /// when a tracked dep's source changes, the `IndexedReady` for that
    /// canonical rebuilds and `route_hash` changes too. Pre-fix any
    /// caching that is NOT keyed by content-hash would return the same
    /// hash across mutations. Post-fix the field is rebuilt with the
    /// new ShallowFileState whose whole_hash differs.
    #[test]
    fn step8_route_hash_invalidated_on_content_change() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Source.ts".to_string(),
            Arc::from(
                r#"
export interface SourceProps {
  size?: 'sm' | 'md' | 'lg'
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Card.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { SourceProps } from './Source'
defineProps<SourceProps>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Card.vue"));
        host.set_import_dependencies(
            "/src/Card.vue",
            vec![crate::DependencyResolution {
                specifier: "./Source".to_string(),
                resolved_canonical_id: Some("/src/Source.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        // Trigger component-meta resolution which loads Source.ts as
        // a dependency.
        let _ = host.get_component_meta("/src/Card.vue");

        let initial_hash = host
            .project_type_store()
            .indexed()
            .get_any("/src/Source.ts")
            .expect("Source.ts must be indexed after dependency-walk")
            .route_hash
            .expect("Source.ts has resolvable surface — route_hash must be Some");

        // Mutate the dep's source so the shallow surface changes. The
        // new content (different prop name + extra prop) MUST produce a
        // different route_hash since the resolvable surface differs.
        // upsert with the new content forces re-indexing through the
        // host's parsing path (matches LSP didChange flow).
        let _ = host.upsert(crate::UpsertRequest {
            canonical_id: Some("/src/Source.ts".into()),
            input_id: "/src/Source.ts".into(),
            source: Arc::from(
                r#"
export interface SourceProps {
  variant?: 'primary' | 'secondary' | 'tertiary'
  loading?: boolean
}
"#,
            ),
            file_kind: crate::FileKind::NonSfc,
            aliases: vec![],
        });

        // Re-trigger meta to re-walk dependencies after the upsert.
        let _ = host.get_component_meta("/src/Card.vue");

        let after_hash = host
            .project_type_store()
            .indexed()
            .get_any("/src/Source.ts")
            .expect("Source.ts must be re-indexed after upsert")
            .route_hash
            .expect("post-mutation Source.ts has resolvable surface — route_hash must be Some");

        assert_ne!(
            initial_hash, after_hash,
            "Step 8: route_hash MUST change when the resolvable surface changes. \
             Pre-mutation hash and post-mutation hash matched, which means the \
             cache lifecycle is not keyed by content. Initial: {initial_hash:?} After: {after_hash:?}",
        );
    }

    /// FAIL-FIRST (plan §3 Step 9.1 / D32 / D24 — `surface_node_ids_partition`):
    /// when audit is on, `ResolvedComponentMetaState.surface_identities`
    /// is populated with vector-aligned `Option<SemanticNodeId>` per
    /// output entry in `evaluated_types`. Pre-Step-9.1 the field was
    /// always `None`. Post-fix the FieldKind closure routes the
    /// dispatch lower's SemanticNodeId per FieldKind into per-kind
    /// buffers; the assembled sidecar lengths match the corresponding
    /// `ExpandedComponentTypes` vectors.
    ///
    /// Discriminator: assert
    /// `surface_identities.is_some()` AND
    /// `surface_identities.prop_node_ids.len() == evaluated_types.props.len()`
    /// for an audit-enabled host on a fixture with a single defineProps
    /// field. This catches drift where the closure stops being called
    /// in lock-step with the output vector.
    #[test]
    fn step9_1_surface_identities_populated_for_audit_enabled_host() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
defineProps<{
  size?: 'sm' | 'md' | 'lg'
  label?: string
}>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                audit_enabled: true,
                footprint_capture: true,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let resolved = host
            .resolve_component_meta(
                "/src/Avatar.vue",
                crate::semantic_query::ProjectionMode::Expanded,
            )
            .expect("Avatar.vue must resolve under audit-enabled config");

        let evaluated = resolved
            .evaluated_types
            .as_ref()
            .expect("audit-enabled Expanded resolution should have evaluated_types");
        let surface_ids = resolved
            .surface_identities
            .as_ref()
            .expect("Step 9.1: surface_identities MUST be Some when audit is on");

        assert_eq!(
            surface_ids.prop_node_ids.len(),
            evaluated.props.len(),
            "Step 9.1: prop_node_ids length must match evaluated_types.props length \
             (vector-aligned sidecar invariant from §1.7)",
        );
        assert_eq!(
            surface_ids.emit_node_ids.len(),
            evaluated.emits.len(),
            "Step 9.1: emit_node_ids length must match evaluated_types.emits length",
        );
        assert_eq!(
            surface_ids.slot_binding_node_ids.len(),
            evaluated.slot_bindings.len(),
            "Step 9.1: slot_binding_node_ids length must match evaluated_types.slot_bindings length",
        );
        assert_eq!(
            surface_ids.binding_node_ids.len(),
            evaluated.bindings.len(),
            "Step 9.1: binding_node_ids length must match evaluated_types.bindings length",
        );
    }

    /// REGRESSION INVARIANT (plan §3 Step 9.1): when audit is OFF,
    /// `surface_identities` stays `None` so the dispatch round-trip
    /// for capture is skipped (perf cost gate). The Step 9.2 scoped
    /// origin export is itself audit-gated, so the partition is
    /// audit-on=Some / audit-off=None — there is no third state.
    #[test]
    fn step9_1_surface_identities_none_when_audit_off() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Avatar.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
defineProps<{ size?: 'sm' | 'md' | 'lg' }>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                audit_enabled: false,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Avatar.vue"));

        let resolved = host
            .resolve_component_meta(
                "/src/Avatar.vue",
                crate::semantic_query::ProjectionMode::Expanded,
            )
            .expect("Avatar.vue must resolve under audit-off config");

        assert!(
            resolved.surface_identities.is_none(),
            "Step 9.1: surface_identities MUST be None when audit is off — the dispatch \
             round-trip for node_id capture is audit-gated to avoid the round-trip cost \
             on the hot non-audit path. Got {:?}.",
            resolved.surface_identities,
        );
    }

    /// REGRESSION INVARIANT (plan §3 Step 6.2): an indexed-access
    /// fixture that previously round-tripped to concrete literal
    /// unions still does so post-reorder. The reorder must not change
    /// the public contract for fixtures where the eager materialize
    /// path was the correct answer.
    #[test]
    fn step6_2_reorder_preserves_indexed_access_resolution() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/Helper.ts".to_string(),
            Arc::from(
                r#"
export interface HelperProps {
  name?: string
  description?: string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/Card.vue".to_string(),
            Arc::from(
                r#"<script setup lang="ts">
import type { HelperProps } from './Helper'

defineProps<Pick<HelperProps, 'name' | 'description'>>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/Card.vue"));
        host.set_import_dependencies(
            "/src/Card.vue",
            vec![crate::DependencyResolution {
                specifier: "./Helper".to_string(),
                resolved_canonical_id: Some("/src/Helper.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let meta = host
            .get_component_meta("/src/Card.vue")
            .expect("Card.vue should produce component meta");

        let prop_names: Vec<&str> = meta.props.iter().map(|p| p.name.as_str()).collect();
        assert!(
            prop_names.contains(&"name") && prop_names.contains(&"description"),
            "Pick<HelperProps, 'name' | 'description'> should yield both props, \
             got {prop_names:?}",
        );
    }

    #[test]
    fn slow_lane_forbid_guards_are_thread_local() {
        let _structural_guard = forbid_structural_slow_lane_for_tests();
        let _direct_pick_guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let _prepared_guard = forbid_prepared_structural_substitution_slow_lane_for_tests();

        assert!(structural_slow_lane_forbidden_for_current_thread());
        assert!(direct_pick_routed_expr_slow_lane_forbidden_for_current_thread());
        assert!(prepared_structural_substitution_slow_lane_forbidden_for_current_thread());

        let (structural, direct_pick, prepared) = std::thread::spawn(|| {
            (
                structural_slow_lane_forbidden_for_current_thread(),
                direct_pick_routed_expr_slow_lane_forbidden_for_current_thread(),
                prepared_structural_substitution_slow_lane_forbidden_for_current_thread(),
            )
        })
        .join()
        .expect("thread-local guard probe should join cleanly");

        assert!(
            !structural,
            "structural slow-lane guard should not leak across test threads",
        );
        assert!(
            !direct_pick,
            "direct-pick slow-lane guard should not leak across test threads",
        );
        assert!(
            !prepared,
            "prepared structural substitution slow-lane guard should not leak across test threads",
        );
    }

    /// Reproduces the App.vue pattern from nuxt-ui: an interface in a `.vue`
    /// file's normal `<script>` block extends `Omit<ExternalType, keys>`,
    /// and a separate `<script setup>` block uses `defineProps<AppProps>()`.
    /// The prepared surface projection must resolve the cross-file Omit and
    /// include the inherited members.
    #[test]
    fn project_prepared_type_surface_shape_resolves_cross_file_omit_in_interface_extends() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/external.ts".to_string(),
            Arc::from(
                r#"
export interface ConfigProviderProps {
  dir?: string
  locale?: string
  scrollBody?: boolean
  nonce?: string
  useId?: () => string
}
"#,
            ),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { ConfigProviderProps } from './external'

export interface AppProps extends Omit<ConfigProviderProps, 'useId' | 'locale'> {
  tooltip?: string
  portal?: boolean | string
}
</script>

<script setup lang="ts">
const props = defineProps<AppProps>()
</script>
<template><div /></template>"#,
            ),
        );

        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));

        let _store_view = host.resolver_store_view();
        let mut query_engine = ComponentMetaQueryEngine::new(&host);

        let shape = crate::meta_resolve::project_prepared_type_surface_shape_via_host_threaded(
            &mut query_engine,
            "/src/App.vue",
            "AppProps",
        )
        .expect("cross-file Omit in interface extends should produce a projectable surface");

        let member_names: Vec<&str> = shape.properties.iter().map(|p| p.name.as_str()).collect();

        // Own members
        assert!(
            member_names.contains(&"tooltip"),
            "own member 'tooltip' must be present, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"portal"),
            "own member 'portal' must be present, got {member_names:?}",
        );

        // Inherited from ConfigProviderProps after Omit<..., 'useId' | 'locale'>
        assert!(
            member_names.contains(&"dir"),
            "inherited member 'dir' must be present after Omit, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"scrollBody"),
            "inherited member 'scrollBody' must be present after Omit, got {member_names:?}",
        );
        assert!(
            member_names.contains(&"nonce"),
            "inherited member 'nonce' must be present after Omit, got {member_names:?}",
        );

        // Omitted keys must NOT be present
        assert!(
            !member_names.contains(&"useId"),
            "omitted member 'useId' must NOT be present, got {member_names:?}",
        );
        assert!(
            !member_names.contains(&"locale"),
            "omitted member 'locale' must NOT be present, got {member_names:?}",
        );
    }
}
