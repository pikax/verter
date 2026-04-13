//! Declaration-aware component-meta query engine.
//!
//! `ComponentMetaQueryEngine` wraps a single request-scoped `TypeQueryEngine`
//! that is shared across owner solves and all declaration-scoped solves in one
//! `get_component_meta()` request. Declaration-scoped solves use
//! `TypeQueryEngine::solve_scoped()` to reuse the shared arena, caches, and
//! root_identity memoization while resolving through a file-scoped host.
//!
//! The `scoped_cache` provides query-local memoization by
//! `(scope_canonical_id, symbol_name)` to avoid re-solving the same imported
//! type reference within one request.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::query_engine::{
    ProjectedMember, ProjectedSurface, TypeQueryEngine,
};
use verter_semantic::analysis::type_solver::result::SolverResult;

use super::declaration_metadata::{
    DeclarationMetadataResolver, ResolvedDeclarationKind, ResolvedLocalTypeSymbolMetadata,
    ResolvedTypeDeclaration,
};
use crate::resolver_core::solver_host::{DeclarationScopePayload, SessionSolverHost};
use crate::resolver_core::{FuseBudgets, FuseState};
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
pub struct ResolvedImportedRegistrySymbol {
    pub canonical_id: String,
    pub exported_name: String,
    pub body: TypeExpr,
    pub canonical_dependencies: BTreeSet<String>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ScopedSolveKey {
    scope_canonical_id: String,
    symbol_name: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct MaterializedMemberSurfaceKey {
    scope_canonical_id: String,
    target: MaterializedMemberSurfaceTarget,
    nested_surface: bool,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
enum MaterializedMemberSurfaceTarget {
    Symbol(String),
    RoutedMember {
        root_symbol: String,
        route: super::RouteDemand,
    },
    Structural(TypeExpr),
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

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
enum PreparedMemberCacheKind {
    Requested,
    InheritedRoute,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct PreparedTargetCacheKey {
    active_scope_canonical_id: String,
    decl_canonical_id: String,
    decl_symbol_name: String,
    requested_name: String,
}

#[derive(Debug, Clone)]
struct ScopedSolveEntry {
    result: SolverResult<TypeExpr>,
}

/// Query-local component-meta solve engine.
///
/// The owner engine is the single request-scoped mutable solver owner.
/// Declaration-scoped solves reuse the same engine via `solve_scoped()`,
/// sharing arena nodes, caches, and root_identity memoization across all
/// scoped queries in one request. Imported registry entries additionally
/// memoize by declaration scope so the same textual reference does not
/// alias across files.
pub struct ComponentMetaQueryEngine<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    owner_engine: TypeQueryEngine<'a>,
    current_prepared_request_root: Option<String>,
    scoped_cache: FxHashMap<ScopedSolveKey, ScopedSolveEntry>,
    imported_registry_symbols: FxHashMap<(String, String), Option<ResolvedImportedRegistrySymbol>>,
    /// Cached type declarations.
    declarations: FxHashMap<(String, String), ResolvedTypeDeclaration>,
    /// Cached resolvability checks.
    resolvable: FxHashMap<(String, String), bool>,
    /// Cached owner collection expressions.
    owner_collection_exprs:
        FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>,
    /// Request-local cache of declaration-scope payloads per scope canonical id.
    /// The prepared bundle stays authoritative; this cache only reuses the
    /// bundle-derived names/bindings within one request so repeated projections
    /// do not keep recloning them.
    scope_payloads: FxHashMap<String, Option<std::sync::Arc<DeclarationScopePayload>>>,
    /// Request-local cache for named-ref member surface materialization.
    /// This sits above the DB-backed projection caches so repeated registry
    /// enrichment can reuse the fully materialized nested surface for the same
    /// imported named ref within one request.
    materialized_member_surfaces: FxHashMap<MaterializedMemberSurfaceKey, TypeExpr>,
    /// Request-local memoization for prepared shallow surface projection.
    prepared_surface_cache: FxHashMap<PreparedSurfaceCacheKey, PreparedSurfaceProjection>,
    /// Request-local memoization for prepared member projection.
    prepared_member_cache: FxHashMap<PreparedMemberCacheKey, Option<ProjectedMember>>,
    /// Request-local memoization for prepared imported target normalization.
    prepared_target_cache: FxHashMap<PreparedTargetCacheKey, Option<(String, String)>>,
    /// Request-local memoization for prepared declaration lookups.
    prepared_type_decls: FxHashMap<
        (String, String),
        Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>>,
    >,
    #[cfg(test)]
    prepared_type_decl_query_count: usize,
    #[cfg(test)]
    prepared_root_surface_projection_count: usize,
    #[cfg(test)]
    prepared_shared_surface_hit_count: usize,
    #[cfg(test)]
    prepared_shared_member_hit_count: usize,
    fuse_budgets: FuseBudgets,
    fuse_state: FuseState,
}

#[cfg(test)]
static FORBID_STRUCTURAL_SLOW_LANE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
static FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub(crate) struct StructuralSlowLaneGuard;

#[cfg(test)]
impl Drop for StructuralSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_STRUCTURAL_SLOW_LANE.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) fn forbid_structural_slow_lane_for_tests() -> StructuralSlowLaneGuard {
    FORBID_STRUCTURAL_SLOW_LANE.store(true, Ordering::SeqCst);
    StructuralSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct DirectPickRoutedExprSlowLaneGuard;

#[cfg(test)]
impl Drop for DirectPickRoutedExprSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) fn forbid_direct_pick_routed_expr_slow_lane_for_tests(
) -> DirectPickRoutedExprSlowLaneGuard {
    FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.store(true, Ordering::SeqCst);
    DirectPickRoutedExprSlowLaneGuard
}

#[cfg(test)]
pub(crate) struct PreparedStructuralSubstitutionSlowLaneGuard;

#[cfg(test)]
impl Drop for PreparedStructuralSubstitutionSlowLaneGuard {
    fn drop(&mut self) {
        FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) fn forbid_prepared_structural_substitution_slow_lane_for_tests(
) -> PreparedStructuralSubstitutionSlowLaneGuard {
    FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.store(true, Ordering::SeqCst);
    PreparedStructuralSubstitutionSlowLaneGuard
}

#[cfg(test)]
fn assert_structural_slow_lane_allowed() {
    assert!(
        !FORBID_STRUCTURAL_SLOW_LANE.load(Ordering::SeqCst),
        "component-meta structural slow lane should not be used on the DB-backed production path",
    );
}

#[cfg(test)]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {
    assert!(
        !FORBID_DIRECT_PICK_ROUTED_EXPR_SLOW_LANE.load(Ordering::SeqCst),
        "direct routed-expr pick slow lane should not be used when member projection can satisfy the route",
    );
}

#[cfg(not(test))]
fn assert_structural_slow_lane_allowed() {}

#[cfg(not(test))]
fn assert_direct_pick_routed_expr_slow_lane_allowed() {}

#[cfg(test)]
fn assert_prepared_structural_substitution_slow_lane_allowed(expr: &TypeExpr) {
    let is_structural = matches!(
        expr,
        TypeExpr::Object(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Union(_)
            | TypeExpr::Function(_)
            | TypeExpr::Parenthesized(_)
    );
    if is_structural {
        assert!(
            !FORBID_PREPARED_STRUCTURAL_SUBSTITUTION_SLOW_LANE.load(Ordering::SeqCst),
            "prepared generic projection should not whole-substitute structural bodies when shallow member-local substitution can satisfy the route",
        );
    }
}

#[cfg(not(test))]
fn assert_prepared_structural_substitution_slow_lane_allowed(_expr: &TypeExpr) {}

impl<'a> ComponentMetaQueryEngine<'a> {
    pub fn new(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_solver_host: &'a dyn TypeSolverHost,
    ) -> Self {
        Self {
            host,
            store_view,
            owner_engine: TypeQueryEngine::new(owner_solver_host),
            current_prepared_request_root: None,
            scoped_cache: FxHashMap::default(),
            imported_registry_symbols: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
            scope_payloads: FxHashMap::default(),
            materialized_member_surfaces: FxHashMap::default(),
            prepared_surface_cache: FxHashMap::default(),
            prepared_member_cache: FxHashMap::default(),
            prepared_target_cache: FxHashMap::default(),
            prepared_type_decls: FxHashMap::default(),
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
        }
    }

    pub fn from_owner_engine(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_engine: TypeQueryEngine<'a>,
    ) -> Self {
        Self {
            host,
            store_view,
            owner_engine,
            current_prepared_request_root: None,
            scoped_cache: FxHashMap::default(),
            imported_registry_symbols: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
            scope_payloads: FxHashMap::default(),
            materialized_member_surfaces: FxHashMap::default(),
            prepared_surface_cache: FxHashMap::default(),
            prepared_member_cache: FxHashMap::default(),
            prepared_target_cache: FxHashMap::default(),
            prepared_type_decls: FxHashMap::default(),
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
        }
    }

    pub fn owner_engine_mut(&mut self) -> &mut TypeQueryEngine<'a> {
        &mut self.owner_engine
    }

    pub fn into_owner_engine(self) -> TypeQueryEngine<'a> {
        self.owner_engine
    }

    fn scope_payload_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> Option<std::sync::Arc<DeclarationScopePayload>> {
        let host = self.host;
        let store_view = self.store_view;
        self.scope_payloads
            .entry(scope_canonical_id.to_string())
            .or_insert_with(|| {
                host.prepared_decl_bundle_in_view(scope_canonical_id, store_view)
                    .map(|bundle| {
                        std::sync::Arc::new(DeclarationScopePayload::from_bundle(&bundle))
                    })
            })
            .clone()
    }

    /// Create a `SessionSolverHost` for the given declaration scope, reusing a
    /// previously-fetched declaration-scope payload when available.
    fn solver_host_for_scope(&mut self, scope_canonical_id: &str) -> SessionSolverHost<'a> {
        if let Some(scope_payload) = self.scope_payload_for_scope(scope_canonical_id) {
            SessionSolverHost::from_scope_payload(
                self.host,
                self.store_view,
                scope_canonical_id,
                scope_payload,
            )
        } else {
            SessionSolverHost::new(self.host, self.store_view)
        }
    }

    #[cfg(test)]
    pub(crate) fn debug_solver_host_for_scope(
        &mut self,
        scope_canonical_id: &str,
    ) -> SessionSolverHost<'a> {
        self.solver_host_for_scope(scope_canonical_id)
    }

    pub fn solve_owner_named(&mut self, requested_name: &str) -> Option<TypeExpr> {
        assert_structural_slow_lane_allowed();
        let expr = TypeExpr::named(requested_name);
        let result = self.owner_engine.solve(&expr);
        filter_identity_ref(&result, requested_name)
    }

    pub fn resolve_imported_registry_symbol(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<ResolvedImportedRegistrySymbol> {
        let key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = self.imported_registry_symbols.get(&key) {
            return cached.clone();
        }
        let resolved = resolve_imported_registry_symbol_with_budget(
            self.host,
            canonical_id,
            exported_name,
            self.store_view,
            || self.allow_wildcard_route(),
        );
        self.imported_registry_symbols.insert(key, resolved.clone());
        resolved
    }

    pub fn resolve_direct_prepared_type_declaration(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        if self
            .prepared_type_decl(canonical_source, resolved_name)
            .is_none()
        {
            return None;
        }
        let metadata = local_type_symbol_metadata_for_known_source(
            self.host,
            canonical_source,
            resolved_name,
            self.store_view,
        )?;
        let resolver = DirectPreparedDeclarationResolver {
            host: self.host,
            store_view: self.store_view,
        };
        Some(crate::resolver_core::resolve_local_type_declaration(
            &resolver,
            canonical_source,
            resolved_name,
            metadata.span,
        ))
    }

    pub fn resolve_direct_prepared_type_declaration_metadata(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        if self
            .prepared_type_decl(canonical_source, resolved_name)
            .is_none()
        {
            return None;
        }
        let metadata = local_type_symbol_metadata_for_known_source(
            self.host,
            canonical_source,
            resolved_name,
            self.store_view,
        )?;
        Some(ResolvedTypeDeclaration {
            requested_name: resolved_name.to_string(),
            declaration_id: self.host.local_type_declaration_id_in_view(
                canonical_source,
                resolved_name,
                self.store_view,
            ),
            resolved_name: resolved_name.to_string(),
            canonical_source: canonical_source.to_string(),
            span: metadata.span,
            kind: metadata.kind,
            text: None,
        })
    }

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        if let Some(cached) = self.declarations.get(&key) {
            return cached.clone();
        }
        let declaration = self
            .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
            .unwrap_or_else(|| {
                crate::meta_resolve::resolve_type_declaration_in_view(
                    self.host,
                    canonical_source,
                    requested_name,
                    self.store_view,
                )
            });
        self.declarations.insert(key, declaration.clone());
        declaration
    }

    /// Check if a registry ref can resolve, cached per query.
    pub fn can_resolve_registry_symbol(
        &mut self,
        owner_canonical: &str,
        exported_name: &str,
        source_hint: Option<&str>,
    ) -> bool {
        if is_builtin_name(exported_name) {
            return false;
        }
        let source_key = source_hint
            .filter(|s| !s.is_empty())
            .unwrap_or(owner_canonical);
        let key = (source_key.to_string(), exported_name.to_string());
        if let Some(cached) = self.resolvable.get(&key) {
            return *cached;
        }
        let resolved = if self.prepared_type_decl(source_key, exported_name).is_some() {
            true
        } else {
            self.resolve_imported_registry_symbol(source_key, exported_name)
                .is_some()
        };
        self.resolvable.insert(key, resolved);
        resolved
    }

    /// Get the owner's collection expression for a name, cached per query.
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
        if let Some(cached) = self.owner_collection_exprs.get(name) {
            return cached.clone();
        }

        let body = self
            .prepared_type_decl(owner_canonical, name)
            .map(|prepared| prepared.body.clone());
        self.owner_collection_exprs
            .insert(name.to_string(), body.clone());
        body
    }

    pub fn named_decl_body(&mut self, canonical_id: &str, name: &str) -> Option<TypeExpr> {
        self.prepared_type_decl(canonical_id, name)
            .map(|prepared| prepared.body.clone())
    }

    pub fn solve_scoped(
        &mut self,
        scope_canonical_id: &str,
        requested_name: &str,
    ) -> Option<TypeExpr> {
        assert_structural_slow_lane_allowed();
        let key = ScopedSolveKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            symbol_name: requested_name.to_string(),
        };

        if let Some(entry) = self.scoped_cache.get(&key) {
            return filter_identity_ref(&entry.result, requested_name);
        }

        if is_package_source(Some(scope_canonical_id)) {
            let body = self
                .prepared_type_decl(scope_canonical_id, requested_name)
                .map(|prepared| prepared.body.clone());
            if let Some(ref body) = body {
                self.scoped_cache.insert(
                    key,
                    ScopedSolveEntry {
                        result: SolverResult::exact_concrete(body.clone()),
                    },
                );
            }
            return body;
        }

        if let Some(prepared) = self.prepared_type_decl(scope_canonical_id, requested_name) {
            if is_direct_surface_no_deps(&prepared) {
                let body = prepared.body.clone();
                self.scoped_cache.insert(
                    key,
                    ScopedSolveEntry {
                        result: SolverResult::exact_concrete(body.clone()),
                    },
                );
                return Some(body);
            }
        }

        self.host
            .shallow_file_state_in_view(scope_canonical_id, self.store_view)?;

        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        let type_ref = TypeExpr::named(requested_name);
        let (result, _trace) =
            self.owner_engine
                .solve_scoped(&solver_host, scope_canonical_id, &type_ref);
        // Steps and solve_count are tracked by the shared owner_engine.
        let filtered = filter_identity_ref(&result, requested_name);
        self.scoped_cache.insert(key, ScopedSolveEntry { result });
        filtered
    }

    pub fn scoped_cache_len(&self) -> usize {
        self.scoped_cache.len()
    }

    pub fn cached_materialized_member_surface(
        &self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        nested_surface: bool,
    ) -> Option<TypeExpr> {
        materialized_member_surface_key(scope_canonical_id, expr, nested_surface)
            .and_then(|key| self.materialized_member_surfaces.get(&key).cloned())
    }

    pub fn store_materialized_member_surface(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
        nested_surface: bool,
        materialized: TypeExpr,
    ) {
        let Some(key) = materialized_member_surface_key(scope_canonical_id, expr, nested_surface)
        else {
            return;
        };
        self.materialized_member_surfaces.insert(key, materialized);
    }

    pub fn total_steps(&self) -> u64 {
        self.owner_engine.total_steps()
    }

    pub fn enter_member_surface(&mut self) -> bool {
        self.fuse_state.push_member_recursion();
        !self
            .fuse_state
            .check_member_recursion_depth(&self.fuse_budgets)
    }

    pub fn exit_member_surface(&mut self) {
        self.fuse_state.pop_member_recursion();
    }

    pub fn allow_structural_slow_lane(&mut self) -> bool {
        !self
            .fuse_state
            .check_structural_slow_lane(&self.fuse_budgets)
    }

    /// Check wildcard route fanout budget. Returns `true` if within budget.
    pub fn allow_wildcard_route(&mut self) -> bool {
        !self
            .fuse_state
            .check_wildcard_route_fanout(&self.fuse_budgets)
    }

    /// Check imported-root fanout budget. Returns `true` if within budget.
    pub fn allow_imported_root(&mut self) -> bool {
        !self
            .fuse_state
            .check_imported_root_fanout(&self.fuse_budgets)
    }

    /// Check registry deepening fanout budget. Returns `true` if within budget.
    pub fn allow_registry_deepening(&mut self) -> bool {
        !self
            .fuse_state
            .check_registry_deepening_fanout(&self.fuse_budgets)
    }

    /// Check union/member explosion budget. Returns `true` if within budget.
    pub fn allow_union_member(&mut self) -> bool {
        !self
            .fuse_state
            .check_union_member_explosion(&self.fuse_budgets)
    }

    /// Reset union member counter for per-member branch counting.
    pub fn reset_union_members(&mut self) {
        self.fuse_state.reset_union_members();
    }

    /// Whether any fuse has tripped.
    pub fn has_fuse_tripped(&self) -> bool {
        self.fuse_state.has_tripped()
    }

    /// Get fuse trip details for provenance/tracing.
    pub fn fuse_trips(&self) -> &[super::FuseTrip] {
        &self.fuse_state.trips
    }

    pub fn solve_count(&self) -> u32 {
        self.owner_engine.solve_count()
    }

    #[cfg(test)]
    pub(crate) fn imported_registry_symbol_cache_len(&self) -> usize {
        self.imported_registry_symbols.len()
    }

    #[cfg(test)]
    pub(crate) fn materialized_member_surface_cache_len(&self) -> usize {
        self.materialized_member_surfaces.len()
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_type_decl_query_count(&self) -> usize {
        self.prepared_type_decl_query_count
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_root_surface_projection_count(&self) -> usize {
        self.prepared_root_surface_projection_count
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_shared_surface_hit_count(&self) -> usize {
        self.prepared_shared_surface_hit_count
    }

    #[cfg(test)]
    pub(crate) fn debug_prepared_shared_member_hit_count(&self) -> usize {
        self.prepared_shared_member_hit_count
    }

    fn prepared_type_decl(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>> {
        let key = (canonical_id.to_string(), symbol_name.to_string());
        if let Some(cached) = self.prepared_type_decls.get(&key) {
            return cached.clone();
        }

        #[cfg(test)]
        {
            self.prepared_type_decl_query_count += 1;
        }

        let resolved =
            self.host
                .prepared_type_decl_in_view(canonical_id, symbol_name, self.store_view);
        self.prepared_type_decls.insert(key, resolved.clone());
        resolved
    }

    pub fn trace_summary(
        &self,
    ) -> &verter_semantic::analysis::type_solver::query_engine::SolverTraceSummary {
        &self.owner_engine.trace_summary
    }

    pub(crate) fn host(&self) -> &VerterHost {
        self.host
    }

    pub(crate) fn store_view(&self) -> Option<&HostStoreView> {
        self.store_view
    }

    // -------------------------------------------------------------------
    // WS3: Projection-based surface extraction
    // -------------------------------------------------------------------

    /// Project the full surface of a type expression in a declaration scope.
    ///
    /// This is the projection-based alternative to `solve_scoped` + manual
    /// surface extraction. It builds a `SubjectKey::Decl` for the type,
    /// interns it, and calls `project_surface` to get all members and call
    /// signatures without full structural normalization.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedSurface> {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }

        let surface_key = TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        };
        let op_key = TypeSurfaceOpKey::Surface(surface_key);
        let cached_scope_payload = self.scope_payload_for_scope(scope_canonical_id);
        if let Some(store_view) = self.store_view {
            let host = self.host;
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            let cached = host
                .resolver
                .runtime
                .type_surfaces
                .get_or_project_with_facts(op_key, store_view, || {
                    if host
                        .prepared_type_decl_in_view(
                            scope_canonical_id,
                            symbol_name,
                            Some(store_view),
                        )
                        .is_none()
                    {
                        return Some((TypeSurfaceOpResult::Miss, facts.clone()));
                    }
                    if let Some(surface) =
                        self.project_prepared_root_surface(scope_canonical_id, symbol_name)
                    {
                        return Some((
                            TypeSurfaceOpResult::Surface(projected_surface_unwrap_or_clone(
                                surface,
                            )),
                            facts.clone(),
                        ));
                    }
                    let subject_key = SubjectKey::Decl {
                        canonical_id: scope_canonical_id.to_string(),
                        symbol_name: symbol_name.to_string(),
                        args_hash: 0,
                        conditional_ctx_hash: 0,
                    };
                    let subject_id = self.owner_engine.intern_subject(subject_key);
                    let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
                        SessionSolverHost::from_scope_payload(
                            host,
                            Some(store_view),
                            scope_canonical_id,
                            scope_payload.clone(),
                        )
                    } else {
                        SessionSolverHost::new(host, Some(store_view))
                    };
                    self.owner_engine
                        .project_surface(subject_id, &solver_host, scope_canonical_id)
                        .map(|surface| (TypeSurfaceOpResult::Surface(surface), facts.clone()))
                })?;
            return cached.as_surface().cloned();
        }

        if let Some(surface) = self.project_prepared_root_surface(scope_canonical_id, symbol_name) {
            return Some(projected_surface_unwrap_or_clone(surface));
        }

        let owned_view = self.host.resolver_store_view();
        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
            SessionSolverHost::from_scope_payload(
                self.host,
                Some(&owned_view),
                scope_canonical_id,
                scope_payload.clone(),
            )
        } else {
            SessionSolverHost::new(self.host, Some(&owned_view))
        };
        self.owner_engine
            .project_surface(subject_id, &solver_host, scope_canonical_id)
    }

    pub fn project_type_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        self.project_type_surface(scope_canonical_id, symbol_name)
            .and_then(|surface| projected_surface_to_type_expr(&surface))
    }

    pub fn project_type_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        self.project_type_surface(scope_canonical_id, symbol_name)
            .map(|surface| projected_surface_to_expanded_shape(&surface))
    }

    pub fn project_prepared_type_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        self.cached_prepared_root_surface(scope_canonical_id, symbol_name)
            .and_then(|surface| projected_surface_to_type_expr(&surface))
    }

    pub fn project_prepared_type_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        self.cached_prepared_root_surface(scope_canonical_id, symbol_name)
            .map(|surface| projected_surface_to_expanded_shape(&surface))
    }

    fn cached_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };

        let Some(store_view) = self.store_view else {
            return self
                .project_prepared_root_surface(scope_canonical_id, symbol_name)
                .map(projected_surface_unwrap_or_clone);
        };

        let host = self.host;
        let op_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        });
        let facts = self
            .type_surface_facts(scope_canonical_id)
            .unwrap_or_default();
        let cached = host
            .resolver
            .runtime
            .type_surfaces
            .get_or_project_with_facts(op_key, store_view, || {
                if host
                    .prepared_type_decl_in_view(scope_canonical_id, symbol_name, Some(store_view))
                    .is_none()
                {
                    return Some((TypeSurfaceOpResult::Miss, facts.clone()));
                }
                self.project_prepared_root_surface(scope_canonical_id, symbol_name)
                    .map(|surface| {
                        (
                            TypeSurfaceOpResult::Surface(projected_surface_unwrap_or_clone(
                                surface,
                            )),
                            facts.clone(),
                        )
                    })
                    .or_else(|| Some((TypeSurfaceOpResult::Miss, facts.clone())))
            })?;
        cached.as_surface().cloned()
    }

    fn project_prepared_root_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        let previous_root = self
            .current_prepared_request_root
            .replace(scope_canonical_id.to_string());
        let result = self.project_prepared_root_surface_inner(scope_canonical_id, symbol_name);
        self.current_prepared_request_root = previous_root;
        result
    }

    fn project_prepared_root_surface_inner(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        #[cfg(test)]
        {
            self.prepared_root_surface_projection_count += 1;
        }
        let mut active = FxHashSet::default();
        match self.project_prepared_surface_from_symbol(
            scope_canonical_id,
            symbol_name,
            &FxHashMap::default(),
            &mut active,
        ) {
            PreparedSurfaceProjection::Surface(surface)
                if !projected_surface_is_empty(&surface) =>
            {
                Some(surface)
            }
            _ => None,
        }
    }

    fn project_prepared_surface_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let cache_key = PreparedSurfaceCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            substitutions: prepared_substitution_key(substitutions),
        };
        if let Some(cached) = self.prepared_surface_cache.get(&cache_key) {
            return cached.clone();
        }

        if let Some(cached) =
            self.cached_prepared_surface(scope_canonical_id, symbol_name, substitutions)
        {
            let cached = PreparedSurfaceProjection::Surface(cached);
            self.prepared_surface_cache
                .insert(cache_key.clone(), cached.clone());
            return cached;
        }

        let key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(key.clone()) {
            return PreparedSurfaceProjection::Unsupported;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .map(|prepared| {
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    substitutions,
                    active,
                )
            })
            .unwrap_or(PreparedSurfaceProjection::Unsupported);

        active.remove(&key);
        self.cache_prepared_surface_projection(
            scope_canonical_id,
            symbol_name,
            substitutions,
            &result,
        );
        self.prepared_surface_cache
            .insert(cache_key, result.clone());
        result
    }

    fn project_prepared_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                substitutions,
                active,
            ),
            TypeExpr::Object(object) => PreparedSurfaceProjection::Surface(std::sync::Arc::new(
                projected_surface_from_object_expr_with_substitutions(
                    object,
                    &prepared.type_parameters,
                    substitutions,
                ),
            )),
            TypeExpr::Function(function) => PreparedSurfaceProjection::Surface(
                std::sync::Arc::new(projected_surface_from_function_expr_with_substitutions(
                    function,
                    &prepared.type_parameters,
                    substitutions,
                )),
            ),
            TypeExpr::Intersection(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_intersection(surfaces)
            }
            TypeExpr::Union(parts) => {
                let mut surfaces = Vec::with_capacity(parts.len());
                for part in parts.iter() {
                    match self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        part,
                        substitutions,
                        active,
                    ) {
                        PreparedSurfaceProjection::Surface(surface) => surfaces.push(surface),
                        PreparedSurfaceProjection::Empty => {}
                        PreparedSurfaceProjection::Unsupported => {
                            return PreparedSurfaceProjection::Unsupported;
                        }
                    }
                }
                projected_surface_from_parts_union(surfaces)
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        &FxHashMap::default(),
                        active,
                    );
                }
                self.project_prepared_surface_from_ref(
                    scope_canonical_id,
                    prepared,
                    name.as_ref(),
                    type_arguments.as_ref(),
                    active,
                )
            }
            TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::TypeParameter(_)
            | TypeExpr::KeyOf(_)
            | TypeExpr::Rest(_)
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::Infer { .. } => PreparedSurfaceProjection::Empty,
            TypeExpr::IndexedAccess { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Mapped { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::TypeOf(_) => PreparedSurfaceProjection::Unsupported,
        }
    }

    fn project_prepared_surface_from_ref(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
        type_arguments: &[TypeExpr],
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        match (name, type_arguments) {
            ("Partial", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = true,
            ),
            ("Required", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.optional = false,
            ),
            ("Readonly", [inner]) => apply_surface_member_modifier(
                self.project_prepared_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    &FxHashMap::default(),
                    active,
                ),
                |member| member.readonly = true,
            ),
            ("NonNullable", [inner]) => self.project_prepared_surface_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                &FxHashMap::default(),
                active,
            ),
            ("Pick", [target, keys]) => {
                let Some(requested) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_requested_member_surface_from_expr(
                    scope_canonical_id,
                    prepared,
                    target,
                    &requested,
                    &FxHashMap::default(),
                    active,
                )
            }
            ("Omit", [target, keys]) => {
                let Some(omitted) =
                    self.prepared_string_literal_keys(scope_canonical_id, prepared, keys, active)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                apply_surface_member_filter(
                    self.project_prepared_surface_from_expr(
                        scope_canonical_id,
                        prepared,
                        target,
                        &FxHashMap::default(),
                        active,
                    ),
                    move |member_name| !omitted.iter().any(|candidate| candidate == member_name),
                )
            }
            _ if matches!(name, "Array" | "ReadonlyArray" | "Promise") => {
                PreparedSurfaceProjection::Empty
            }
            _ if is_builtin_name(name) => PreparedSurfaceProjection::Unsupported,
            _ => {
                let Some((target_canonical_id, target_symbol_name)) =
                    self.resolve_prepared_surface_target(scope_canonical_id, prepared, name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_prepared) =
                    self.prepared_type_decl(&target_canonical_id, &target_symbol_name)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                let Some(target_substitutions) =
                    prepared_type_param_substitutions(target_prepared.as_ref(), type_arguments)
                else {
                    return PreparedSurfaceProjection::Unsupported;
                };
                self.project_prepared_surface_from_symbol(
                    &target_canonical_id,
                    &target_symbol_name,
                    &target_substitutions,
                    active,
                )
            }
        }
    }

    fn project_prepared_requested_member_surface_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        requested: &[String],
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> PreparedSurfaceProjection {
        let mut members = Vec::with_capacity(requested.len());
        for member_name in requested {
            let Some(projected_member) = self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                expr,
                member_name,
                substitutions,
                active,
            ) else {
                return PreparedSurfaceProjection::Unsupported;
            };
            members.push(projected_member);
        }

        PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
            members,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            has_index_signature: false,
        }))
    }

    fn project_prepared_requested_member_from_symbol(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        let cache_key = PreparedMemberCacheKey {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            member_name: member_name.to_string(),
            kind: PreparedMemberCacheKind::Requested,
            substitutions: prepared_substitution_key(substitutions),
        };
        if let Some(cached) = self.prepared_member_cache.get(&cache_key) {
            return cached.clone();
        }

        if let Some(cached) = self.cached_prepared_requested_member(
            scope_canonical_id,
            symbol_name,
            member_name,
            substitutions,
        ) {
            self.prepared_member_cache
                .insert(cache_key, Some(cached.clone()));
            return Some(cached);
        }

        let visit_key = (scope_canonical_id.to_string(), symbol_name.to_string());
        if !active.insert(visit_key.clone()) {
            return None;
        }

        let result = self
            .prepared_type_decl(scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member) = prepared.member(member_name) {
                    let projected_member = ProjectedMember {
                        name: member_name.to_string(),
                        ty: substitute_type_expr_if_needed(&member.ty, substitutions),
                        optional: member.optional,
                        readonly: member.readonly,
                        is_method: member.is_method,
                    };
                    if !type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
                        self.cache_prepared_requested_member(
                            scope_canonical_id,
                            symbol_name,
                            &projected_member,
                            substitutions,
                        );
                    }
                    return Some(projected_member);
                }

                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared.as_ref(),
                    &prepared.body,
                    member_name,
                    substitutions,
                    active,
                )
            });

        active.remove(&visit_key);
        self.prepared_member_cache.insert(cache_key, result.clone());
        result
    }

    fn project_prepared_requested_member_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        expr: &TypeExpr,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        active: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        use verter_semantic::analysis::type_expr::ObjectMember;

        match expr {
            TypeExpr::Parenthesized(inner) => self.project_prepared_requested_member_from_expr(
                scope_canonical_id,
                prepared,
                inner,
                member_name,
                substitutions,
                active,
            ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_prepared_requested_member_from_expr(
                    scope_canonical_id,
                    prepared,
                    part,
                    member_name,
                    substitutions,
                    active,
                )
            }),
            TypeExpr::Object(object) => object.properties.iter().find_map(|member| match member {
                ObjectMember::Property(property) if property.name == member_name => {
                    Some(ProjectedMember {
                        name: property.name.clone(),
                        ty: substitute_type_expr_if_needed(&property.ty, substitutions),
                        optional: property.optional,
                        readonly: property.readonly,
                        is_method: false,
                    })
                }
                ObjectMember::Method(method) if method.name == member_name => {
                    Some(ProjectedMember {
                        name: method.name.clone(),
                        ty: TypeExpr::Function(std::sync::Arc::new(
                            substitute_function_expr_if_needed(&method.function, substitutions),
                        )),
                        optional: method.optional,
                        readonly: false,
                        is_method: true,
                    })
                }
                _ => None,
            }),
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                if let Some(substituted) =
                    substituted_ref_expr_if_needed(expr, name.as_ref(), substitutions)
                {
                    return self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        &substituted,
                        member_name,
                        &FxHashMap::default(),
                        active,
                    );
                }
                match (name.as_ref(), type_arguments.as_ref()) {
                    ("Partial", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = true;
                            member
                        }),
                    ("Required", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.optional = false;
                            member
                        }),
                    ("Readonly", [inner]) => self
                        .project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            inner,
                            member_name,
                            substitutions,
                            active,
                        )
                        .map(|mut member| {
                            member.readonly = true;
                            member
                        }),
                    ("NonNullable", [inner]) => self.project_prepared_requested_member_from_expr(
                        scope_canonical_id,
                        prepared,
                        inner,
                        member_name,
                        substitutions,
                        active,
                    ),
                    ("Pick", [target, keys]) => {
                        let requested = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if !requested.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    ("Omit", [target, keys]) => {
                        let omitted = self.prepared_string_literal_keys(
                            scope_canonical_id,
                            prepared,
                            keys,
                            active,
                        )?;
                        if omitted.iter().any(|candidate| candidate == member_name) {
                            return None;
                        }
                        self.project_prepared_requested_member_from_expr(
                            scope_canonical_id,
                            prepared,
                            target,
                            member_name,
                            substitutions,
                            active,
                        )
                    }
                    _ if matches!(name.as_ref(), "Array" | "ReadonlyArray" | "Promise") => None,
                    _ if is_builtin_name(name.as_ref()) => None,
                    _ => {
                        let (target_canonical_id, target_symbol_name) = self
                            .resolve_prepared_surface_target(
                                scope_canonical_id,
                                prepared,
                                name.as_ref(),
                            )?;
                        let target_prepared =
                            self.prepared_type_decl(&target_canonical_id, &target_symbol_name)?;
                        let target_substitutions = prepared_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        self.project_prepared_requested_member_from_symbol(
                            &target_canonical_id,
                            &target_symbol_name,
                            member_name,
                            &target_substitutions,
                            active,
                        )
                    }
                }
            }
            _ => None,
        }
    }

    fn resolve_prepared_surface_target(
        &mut self,
        scope_canonical_id: &str,
        prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
        name: &str,
    ) -> Option<(String, String)> {
        let cache_key = PreparedTargetCacheKey {
            active_scope_canonical_id: scope_canonical_id.to_string(),
            decl_canonical_id: prepared.root_identity.canonical_id.clone(),
            decl_symbol_name: prepared.root_identity.symbol_name.clone(),
            requested_name: name.to_string(),
        };
        if let Some(cached) = self.prepared_target_cache.get(&cache_key) {
            return cached.clone();
        }

        let resolve_prepared_target =
            |this: &mut Self, canonical_source: String, resolved_name: String| {
                let mut canonical_source = if canonical_source.is_empty() {
                    scope_canonical_id.to_string()
                } else {
                    canonical_source
                };
                let mut resolved_name = if resolved_name.is_empty() {
                    name.to_string()
                } else {
                    resolved_name
                };

                if canonical_source != scope_canonical_id {
                    if let Some((routed_source, routed_name)) =
                        this.host.resolve_named_type_export_target_shallow_in_view(
                            canonical_source.as_str(),
                            resolved_name.as_str(),
                            this.store_view,
                        )
                    {
                        if this
                            .prepared_type_decl(routed_source.as_str(), routed_name.as_str())
                            .is_some()
                        {
                            canonical_source = routed_source;
                            resolved_name = routed_name;
                        }
                    }
                }

                this.prepared_type_decl(&canonical_source, &resolved_name)
                    .map(|_| (canonical_source, resolved_name))
            };

        let resolved = prepared
            .name_resolution
            .get(name)
            .and_then(|resolved| {
                resolve_prepared_target(
                    self,
                    resolved.canonical_id.clone(),
                    resolved.symbol_name.clone(),
                )
            })
            .or_else(|| {
                let declaration = self.resolve_type_declaration(scope_canonical_id, name);
                resolve_prepared_target(
                    self,
                    declaration.canonical_source,
                    declaration.resolved_name,
                )
            });
        self.prepared_target_cache
            .insert(cache_key, resolved.clone());
        resolved
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

    /// Project a single member from a type expression in a declaration scope.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedMember> {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }

        let surface_key = TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        };
        let op_key = TypeSurfaceOpKey::Member {
            subject: surface_key,
            member_name: member_name.to_owned(),
        };
        let cached_scope_payload = self.scope_payload_for_scope(scope_canonical_id);
        if let Some(store_view) = self.store_view {
            let host = self.host;
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            let owner_engine = &mut self.owner_engine;
            let cached = host
                .resolver
                .runtime
                .type_surfaces
                .get_or_project_with_facts(op_key, store_view, || {
                    if host
                        .prepared_type_decl_in_view(
                            scope_canonical_id,
                            symbol_name,
                            Some(store_view),
                        )
                        .is_none()
                    {
                        return Some((TypeSurfaceOpResult::Miss, facts.clone()));
                    }
                    let subject_key = SubjectKey::Decl {
                        canonical_id: scope_canonical_id.to_string(),
                        symbol_name: symbol_name.to_string(),
                        args_hash: 0,
                        conditional_ctx_hash: 0,
                    };
                    let subject_id = owner_engine.intern_subject(subject_key);
                    let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
                        SessionSolverHost::from_scope_payload(
                            host,
                            Some(store_view),
                            scope_canonical_id,
                            scope_payload.clone(),
                        )
                    } else {
                        SessionSolverHost::new(host, Some(store_view))
                    };
                    owner_engine
                        .project_member(subject_id, member_name, &solver_host, scope_canonical_id)
                        .map(|member| (TypeSurfaceOpResult::Member(member), facts.clone()))
                })?;
            return cached.as_member().cloned();
        }

        let owned_view = self.host.resolver_store_view();
        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
            SessionSolverHost::from_scope_payload(
                self.host,
                Some(&owned_view),
                scope_canonical_id,
                scope_payload.clone(),
            )
        } else {
            SessionSolverHost::new(self.host, Some(&owned_view))
        };
        self.owner_engine
            .project_member(subject_id, member_name, &solver_host, scope_canonical_id)
    }

    /// Project the keyspace (member names) from a type expression in a
    /// declaration scope.
    ///
    /// Results are write-through: stable projections are published to
    /// `TypeSurfaceDb` and reused by later requests.
    pub fn project_type_keyspace(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedKeyspace> {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }

        let surface_key = TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        };
        let op_key = TypeSurfaceOpKey::Keyspace(surface_key);
        let cached_scope_payload = self.scope_payload_for_scope(scope_canonical_id);
        if let Some(store_view) = self.store_view {
            let host = self.host;
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            let owner_engine = &mut self.owner_engine;
            let cached = host
                .resolver
                .runtime
                .type_surfaces
                .get_or_project_with_facts(op_key, store_view, || {
                    if host
                        .prepared_type_decl_in_view(
                            scope_canonical_id,
                            symbol_name,
                            Some(store_view),
                        )
                        .is_none()
                    {
                        return Some((TypeSurfaceOpResult::Miss, facts.clone()));
                    }
                    let subject_key = SubjectKey::Decl {
                        canonical_id: scope_canonical_id.to_string(),
                        symbol_name: symbol_name.to_string(),
                        args_hash: 0,
                        conditional_ctx_hash: 0,
                    };
                    let subject_id = owner_engine.intern_subject(subject_key);
                    let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
                        SessionSolverHost::from_scope_payload(
                            host,
                            Some(store_view),
                            scope_canonical_id,
                            scope_payload.clone(),
                        )
                    } else {
                        SessionSolverHost::new(host, Some(store_view))
                    };
                    owner_engine
                        .project_keyspace(subject_id, &solver_host, scope_canonical_id)
                        .map(|keyspace| (TypeSurfaceOpResult::Keyspace(keyspace), facts.clone()))
                })?;
            return cached.as_keyspace().cloned();
        }

        let owned_view = self.host.resolver_store_view();
        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
            SessionSolverHost::from_scope_payload(
                self.host,
                Some(&owned_view),
                scope_canonical_id,
                scope_payload.clone(),
            )
        } else {
            SessionSolverHost::new(self.host, Some(&owned_view))
        };
        self.owner_engine
            .project_keyspace(subject_id, &solver_host, scope_canonical_id)
    }

    pub fn project_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        if let Some((root_symbol, route)) =
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .or_else(|| {
                super::component_meta_registry::component_meta_registry_public_utility_route(expr)
            })
        {
            if let Some(projected) =
                self.project_route_surface_expr(scope_canonical_id, &root_symbol, &route)
            {
                return Some(projected);
            }
        }
        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        self.owner_engine
            .project_expr_surface_as_type_expr(&solver_host, scope_canonical_id, expr)
    }

    pub fn solve_expr_type_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        let (result, _) = self
            .owner_engine
            .solve_scoped(&solver_host, scope_canonical_id, expr);
        (result.value != *expr).then_some(result.value)
    }

    pub fn expand_local_generic_ref_expr(
        &mut self,
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

        let declaration = self.resolve_type_declaration(scope_canonical_id, name.as_ref());
        let target_canonical_id = if declaration.canonical_source.is_empty() {
            scope_canonical_id.to_string()
        } else {
            declaration.canonical_source.clone()
        };
        if is_package_source(Some(target_canonical_id.as_str())) {
            return None;
        }
        let target_symbol_name = if declaration.resolved_name.is_empty() {
            name.as_ref()
        } else {
            declaration.resolved_name.as_str()
        };
        let prepared = self.prepared_type_decl(&target_canonical_id, target_symbol_name)?;
        let substitutions = prepared_type_param_substitutions(prepared.as_ref(), type_arguments)?;
        Some(substitute_type_expr_if_needed(
            &prepared.body,
            &substitutions,
        ))
    }

    pub fn project_expr_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        if self
            .fuse_state
            .check_projection_op_count(&self.fuse_budgets)
        {
            return None;
        }
        if let Some((root_symbol, route)) =
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .or_else(|| {
                super::component_meta_registry::component_meta_registry_public_utility_route(expr)
            })
        {
            if let Some(projected) =
                self.project_routed_expr_surface_expr(scope_canonical_id, &root_symbol, &route)
            {
                return Some(
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected),
                );
            }
        }
        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        self.owner_engine
            .project_expr_surface(&solver_host, scope_canonical_id, expr)
            .map(|surface| projected_surface_to_expanded_shape(&surface))
    }

    pub fn project_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        self.project_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
    }

    fn project_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        if let Some(cached_expr) =
            self.cached_routed_expr_surface_expr(scope_canonical_id, root_symbol, route)
        {
            return Some(cached_expr);
        }

        if let super::RouteDemand::MemberPath(path) = route {
            if let Some(projected_expr) = self.project_prepared_member_path_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                path,
            ) {
                if let Some(store_view) = self.store_view {
                    self.cache_routed_expr_surface_expr(
                        scope_canonical_id,
                        root_symbol,
                        route,
                        &projected_expr,
                        store_view,
                    );
                    if let [member_name] = path.as_slice() {
                        if let Some(projected_member) = self
                            .project_prepared_member_route_projection(
                                scope_canonical_id,
                                root_symbol,
                                member_name,
                            )
                        {
                            self.cache_projected_member(
                                scope_canonical_id,
                                root_symbol,
                                &projected_member,
                                store_view,
                            );
                        }
                    }
                }
                return Some(projected_expr);
            }
            if let [member_name] = path.as_slice() {
                let projected_member =
                    self.project_type_member(scope_canonical_id, root_symbol, member_name)?;
                let projected_expr = projected_member.ty.clone();
                if let Some(store_view) = self.store_view {
                    self.cache_routed_expr_surface_expr(
                        scope_canonical_id,
                        root_symbol,
                        route,
                        &projected_expr,
                        store_view,
                    );
                }
                return Some(projected_expr);
            }
        }

        if let super::RouteDemand::Pick(members) = route {
            if let Some(projected_expr) = self.project_prepared_pick_route_surface_expr(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                if let Some(store_view) = self.store_view {
                    self.cache_routed_expr_surface_expr(
                        scope_canonical_id,
                        root_symbol,
                        route,
                        &projected_expr,
                        store_view,
                    );
                }
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_members(
                scope_canonical_id,
                root_symbol,
                members,
            ) {
                if let Some(store_view) = self.store_view {
                    self.cache_routed_expr_surface_expr(
                        scope_canonical_id,
                        root_symbol,
                        route,
                        &projected_expr,
                        store_view,
                    );
                }
                return Some(projected_expr);
            }
            if let Some(projected_expr) = self.project_pick_route_surface_expr_via_routed_expr(
                scope_canonical_id,
                root_symbol,
                route,
                members,
            ) {
                if let Some(store_view) = self.store_view {
                    self.cache_routed_expr_surface_expr(
                        scope_canonical_id,
                        root_symbol,
                        route,
                        &projected_expr,
                        store_view,
                    );
                    self.cache_pick_members_from_projected_expr(
                        scope_canonical_id,
                        root_symbol,
                        members,
                        &projected_expr,
                        store_view,
                    );
                }
                return Some(projected_expr);
            }
        }

        self.project_routed_expr_surface_expr_direct(scope_canonical_id, root_symbol, route)
    }

    fn cached_routed_expr_surface_expr(
        &self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        use crate::resolver_core::type_surface_db::{TypeSurfaceKey, TypeSurfaceOpKey};

        let store_view = self.store_view?;
        let op_key = TypeSurfaceOpKey::RoutedExpr {
            subject: TypeSurfaceKey {
                canonical_owner: scope_canonical_id.to_owned(),
                symbol_name: root_symbol.to_owned(),
                instantiation_hash: 0,
                context_hash: 0,
            },
            route: route.clone(),
        };
        self.host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .and_then(|cached| cached.as_expr().cloned())
    }

    fn cache_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
        projected_expr: &TypeExpr,
        store_view: &HostStoreView,
    ) {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };

        let op_key = TypeSurfaceOpKey::RoutedExpr {
            subject: TypeSurfaceKey {
                canonical_owner: scope_canonical_id.to_owned(),
                symbol_name: root_symbol.to_owned(),
                instantiation_hash: 0,
                context_hash: 0,
            },
            route: route.clone(),
        };
        if self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .is_none()
        {
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            self.host.resolver.runtime.type_surfaces.publish_with_facts(
                op_key,
                TypeSurfaceOpResult::Expr(projected_expr.clone()),
                facts,
            );
        }
    }

    fn cache_pick_members_from_projected_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
        projected_expr: &TypeExpr,
        store_view: &HostStoreView,
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
                self.cache_projected_member(
                    scope_canonical_id,
                    root_symbol,
                    &projected_member,
                    store_view,
                );
            }
        }
    }

    fn cache_projected_member(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        projected_member: &ProjectedMember,
        store_view: &HostStoreView,
    ) {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };

        let op_key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: scope_canonical_id.to_owned(),
                symbol_name: root_symbol.to_owned(),
                instantiation_hash: 0,
                context_hash: 0,
            },
            member_name: projected_member.name.clone(),
        };
        if self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .is_none()
        {
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            self.host.resolver.runtime.type_surfaces.publish_with_facts(
                op_key,
                TypeSurfaceOpResult::Member(projected_member.clone()),
                facts,
            );
        }
    }

    fn cached_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<ProjectedMember> {
        if !self.prepared_requested_member_shared_cache_enabled(scope_canonical_id, substitutions) {
            return None;
        }
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey};

        let store_view = self.store_view?;
        let op_key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: scope_canonical_id.to_owned(),
                symbol_name: symbol_name.to_owned(),
                instantiation_hash: prepared_substitution_instantiation_hash(substitutions),
                context_hash: 0,
            },
            member_name: member_name.to_owned(),
        };
        let cached = self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .and_then(|cached| cached.as_member().cloned());
        #[cfg(test)]
        if cached.is_some() {
            self.prepared_shared_member_hit_count += 1;
        }
        cached
    }

    fn cached_prepared_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> Option<std::sync::Arc<ProjectedSurface>> {
        if !self.prepared_surface_shared_cache_enabled(scope_canonical_id, substitutions) {
            return None;
        }
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey};

        let store_view = self.store_view?;
        let op_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: prepared_substitution_instantiation_hash(substitutions),
            context_hash: 0,
        });
        let cached = self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .and_then(|cached| cached.as_surface().cloned())
            .map(std::sync::Arc::new);
        #[cfg(test)]
        if cached.is_some() {
            self.prepared_shared_surface_hit_count += 1;
        }
        cached
    }

    fn cache_prepared_surface_projection(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
        projection: &PreparedSurfaceProjection,
    ) {
        if !self.prepared_surface_shared_cache_enabled(scope_canonical_id, substitutions) {
            return;
        }
        let PreparedSurfaceProjection::Surface(surface) = projection else {
            return;
        };
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};

        let Some(store_view) = self.store_view else {
            return;
        };
        let op_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: symbol_name.to_owned(),
            instantiation_hash: prepared_substitution_instantiation_hash(substitutions),
            context_hash: 0,
        });
        if self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .is_none()
        {
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            self.host.resolver.runtime.type_surfaces.publish_with_facts(
                op_key,
                TypeSurfaceOpResult::Surface(projected_surface_unwrap_or_clone(surface.clone())),
                facts,
            );
        }
    }

    fn cache_prepared_requested_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        projected_member: &ProjectedMember,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) {
        if !self.prepared_requested_member_shared_cache_enabled(scope_canonical_id, substitutions) {
            return;
        }
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};

        let Some(store_view) = self.store_view else {
            return;
        };
        let op_key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: scope_canonical_id.to_owned(),
                symbol_name: symbol_name.to_owned(),
                instantiation_hash: prepared_substitution_instantiation_hash(substitutions),
                context_hash: 0,
            },
            member_name: projected_member.name.clone(),
        };
        if self
            .host
            .resolver
            .runtime
            .type_surfaces
            .get(&op_key, store_view)
            .is_none()
        {
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            self.host.resolver.runtime.type_surfaces.publish_with_facts(
                op_key,
                TypeSurfaceOpResult::Member(projected_member.clone()),
                facts,
            );
        }
    }

    fn prepared_requested_member_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        !substitutions.is_empty()
            && self.store_view.is_some()
            && self
                .current_prepared_request_root
                .as_deref()
                .is_some_and(|request_root| request_root != scope_canonical_id)
    }

    fn prepared_surface_shared_cache_enabled(
        &self,
        scope_canonical_id: &str,
        substitutions: &FxHashMap<String, TypeExpr>,
    ) -> bool {
        self.store_view.is_some()
            && self
                .current_prepared_request_root
                .as_deref()
                .is_some_and(|request_root| request_root != scope_canonical_id)
            && (!substitutions.is_empty() || is_package_source(Some(scope_canonical_id)))
    }

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
            _ => self.project_expr_surface_expr(scope_canonical_id, &member.ty),
        }?;
        Some(ProjectedMember {
            name: member_name.to_string(),
            ty: projected_ty,
            optional: member.optional,
            readonly: member.readonly,
            is_method: member.is_method,
        })
    }

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

    fn solve_or_project_prepared_member_leaf_expr(
        &mut self,
        resolution_scope_canonical_id: &str,
        active_scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let active_result =
            self.solve_or_project_leaf_expr_until_stable(active_scope_canonical_id, expr);
        if resolution_scope_canonical_id == active_scope_canonical_id
            || !self.expr_references_prepared_scope_symbol(resolution_scope_canonical_id, expr)
        {
            return active_result;
        }

        self.solve_or_project_leaf_expr_until_stable(resolution_scope_canonical_id, expr)
            .or(active_result)
    }

    fn solve_or_project_leaf_expr_until_stable(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let mut current = expr.clone();
        let mut last = None;
        for _ in 0..3 {
            let next = self
                .solve_expr_type_expr(scope_canonical_id, &current)
                .or_else(|| self.project_expr_surface_expr(scope_canonical_id, &current));
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

        let result = self
            .prepared_type_decl(resolution_scope_canonical_id, symbol_name)
            .and_then(|prepared| {
                if let Some(member_name) = path.first() {
                    if let Some(member) = prepared.member(member_name) {
                        let member_ty = substitute_type_expr_if_needed(&member.ty, substitutions);
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
            let projected_expr = substitute_type_expr_if_needed(expr, substitutions);
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
                        Some(substitute_type_expr_if_needed(&property.ty, substitutions))
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
                        let target_substitutions = prepared_type_param_substitutions(
                            target_prepared.as_ref(),
                            type_arguments.as_ref(),
                        )?;
                        self.project_prepared_member_path_route_projection_from_symbol(
                            &target_canonical_id,
                            active_scope_canonical_id,
                            &target_symbol_name,
                            path,
                            &target_substitutions,
                            visited,
                        )
                    }
                }
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
                    substitute_type_expr_if_needed(expr, substitutions),
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
        if let Some(cached) = self.prepared_member_cache.get(&cache_key) {
            return cached.clone();
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
        self.prepared_member_cache.insert(cache_key, result.clone());
        result
    }

    fn project_inherited_member_route_projection_from_expr(
        &mut self,
        scope_canonical_id: &str,
        prepared: &std::sync::Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>,
        expr: &TypeExpr,
        member_name: &str,
        visited: &mut FxHashSet<(String, String)>,
    ) -> Option<ProjectedMember> {
        match expr {
            TypeExpr::Parenthesized(inner) => self
                .project_inherited_member_route_projection_from_expr(
                    scope_canonical_id,
                    prepared,
                    inner,
                    member_name,
                    visited,
                ),
            TypeExpr::Intersection(parts) => parts.iter().rev().find_map(|part| {
                self.project_inherited_member_route_projection_from_expr(
                    scope_canonical_id,
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
                self.project_type_member(scope_canonical_id, symbol_name, member_name)?
            };
            if let Some(store_view) = self.store_view {
                self.cache_projected_member(
                    scope_canonical_id,
                    symbol_name,
                    &projected_member,
                    store_view,
                );
            }
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

    fn project_routed_expr_surface_expr_direct(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &super::RouteDemand,
    ) -> Option<TypeExpr> {
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };

        let route_expr = routed_expr_surface_key_expr(root_symbol, route)?;
        let subject = TypeSurfaceKey {
            canonical_owner: scope_canonical_id.to_owned(),
            symbol_name: root_symbol.to_owned(),
            instantiation_hash: 0,
            context_hash: 0,
        };
        let op_key = TypeSurfaceOpKey::RoutedExpr {
            subject,
            route: route.clone(),
        };
        let cached_scope_payload = self.scope_payload_for_scope(scope_canonical_id);

        if let Some(store_view) = self.store_view {
            let host = self.host;
            let facts = self
                .type_surface_facts(scope_canonical_id)
                .unwrap_or_default();
            let owner_engine = &mut self.owner_engine;
            let cached = host
                .resolver
                .runtime
                .type_surfaces
                .get_or_project_with_facts(op_key, store_view, || {
                    if host
                        .prepared_type_decl_in_view(
                            scope_canonical_id,
                            root_symbol,
                            Some(store_view),
                        )
                        .is_none()
                    {
                        return Some((TypeSurfaceOpResult::Miss, facts.clone()));
                    }
                    let solver_host = if let Some(ref scope_payload) = cached_scope_payload {
                        SessionSolverHost::from_scope_payload(
                            host,
                            Some(store_view),
                            scope_canonical_id,
                            scope_payload.clone(),
                        )
                    } else {
                        SessionSolverHost::new(host, Some(store_view))
                    };
                    owner_engine
                        .project_expr_surface_as_type_expr(
                            &solver_host,
                            scope_canonical_id,
                            &route_expr,
                        )
                        .map(|expr| (TypeSurfaceOpResult::Expr(expr), facts.clone()))
                })?;
            return cached.as_expr().cloned();
        }

        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        self.owner_engine.project_expr_surface_as_type_expr(
            &solver_host,
            scope_canonical_id,
            &route_expr,
        )
    }

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

    fn type_surface_facts(
        &self,
        scope_canonical_id: &str,
    ) -> Option<Vec<crate::resolver_core::FactVersionRef>> {
        let store_view = self.store_view?;
        let mut facts = Vec::new();
        if let Some(hash) = store_view
            .whole_hash(scope_canonical_id)
            .or_else(|| self.host.get_whole_hash(scope_canonical_id))
        {
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
        self.prepared_surface_cache.len()
    }

    #[cfg(test)]
    fn debug_prepared_member_cache_len(&self) -> usize {
        self.prepared_member_cache.len()
    }

    #[cfg(test)]
    fn debug_prepared_target_cache_len(&self) -> usize {
        self.prepared_target_cache.len()
    }
}

fn local_type_symbol_metadata_for_known_source(
    host: &VerterHost,
    canonical_source: &str,
    resolved_name: &str,
    store_view: Option<&HostStoreView>,
) -> Option<ResolvedLocalTypeSymbolMetadata> {
    let analysis = host.external_type_analysis_in_view(canonical_source, store_view)?;
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
    store_view: Option<&'a HostStoreView>,
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

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        self.host
            .read_analysis_source_in_view(canonical_source, self.store_view)
            .as_deref()
            .map(str::to_string)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<DeclarationId> {
        self.host.local_type_declaration_id_in_view(
            canonical_source,
            resolved_name,
            self.store_view,
        )
    }

    fn resolve_type_dependency_canonical(
        &self,
        _from_canonical: &str,
        _import_source: &str,
    ) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone)]
enum PreparedSurfaceProjection {
    Surface(std::sync::Arc<ProjectedSurface>),
    Empty,
    Unsupported,
}

fn prepared_substitution_key(
    substitutions: &FxHashMap<String, TypeExpr>,
) -> PreparedSubstitutionKey {
    if substitutions.is_empty() {
        return PreparedSubstitutionKey::Empty;
    }

    let mut entries = substitutions
        .iter()
        .map(|(name, ty)| (name.clone(), ty.clone()))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    PreparedSubstitutionKey::Entries(entries)
}

fn prepared_substitution_instantiation_hash(substitutions: &FxHashMap<String, TypeExpr>) -> u64 {
    if substitutions.is_empty() {
        return 0;
    }

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    prepared_substitution_key(substitutions).hash(&mut hasher);
    hasher.finish()
}

fn projected_surface_is_empty(surface: &ProjectedSurface) -> bool {
    surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
}

fn projected_surface_from_object_expr(
    object: &verter_semantic::analysis::type_expr::ObjectExpr,
) -> ProjectedSurface {
    use verter_semantic::analysis::type_expr::ObjectMember;

    let mut members = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for member in &object.properties {
        match member {
            ObjectMember::Property(property) => members.push(ProjectedMember {
                name: property.name.clone(),
                ty: property.ty.clone(),
                optional: property.optional,
                readonly: property.readonly,
                is_method: false,
            }),
            ObjectMember::Method(method) => members.push(ProjectedMember {
                name: method.name.clone(),
                ty: TypeExpr::Function(std::sync::Arc::new(method.function.clone())),
                optional: method.optional,
                readonly: false,
                is_method: true,
            }),
            ObjectMember::CallSignature(function) => {
                call_signatures.push(TypeExpr::Function(std::sync::Arc::new(function.clone())));
            }
            ObjectMember::ConstructSignature(function) => {
                construct_signatures
                    .push(TypeExpr::Function(std::sync::Arc::new(function.clone())));
            }
            ObjectMember::IndexSignature(_) => has_index_signature = true,
        }
    }

    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }
}

fn projected_surface_from_object_expr_with_substitutions(
    object: &verter_semantic::analysis::type_expr::ObjectExpr,
    _type_params: &[verter_semantic::analysis::type_expr::TypeParam],
    substitutions: &FxHashMap<String, TypeExpr>,
) -> ProjectedSurface {
    use verter_semantic::analysis::type_expr::ObjectMember;

    if substitutions.is_empty() {
        return projected_surface_from_object_expr(object);
    }

    let mut members = Vec::new();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for member in &object.properties {
        match member {
            ObjectMember::Property(property) => members.push(ProjectedMember {
                name: property.name.clone(),
                ty: substitute_type_expr_if_needed(&property.ty, substitutions),
                optional: property.optional,
                readonly: property.readonly,
                is_method: false,
            }),
            ObjectMember::Method(method) => members.push(ProjectedMember {
                name: method.name.clone(),
                ty: TypeExpr::Function(std::sync::Arc::new(substitute_function_expr_if_needed(
                    &method.function,
                    substitutions,
                ))),
                optional: method.optional,
                readonly: false,
                is_method: true,
            }),
            ObjectMember::CallSignature(function) => call_signatures.push(TypeExpr::Function(
                std::sync::Arc::new(substitute_function_expr_if_needed(function, substitutions)),
            )),
            ObjectMember::ConstructSignature(function) => {
                construct_signatures.push(TypeExpr::Function(std::sync::Arc::new(
                    substitute_function_expr_if_needed(function, substitutions),
                )))
            }
            ObjectMember::IndexSignature(_) => has_index_signature = true,
        }
    }

    ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }
}

fn projected_surface_from_function_expr(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
) -> ProjectedSurface {
    ProjectedSurface {
        members: Vec::new(),
        call_signatures: vec![TypeExpr::Function(std::sync::Arc::new(function.clone()))],
        construct_signatures: Vec::new(),
        has_index_signature: false,
    }
}

fn projected_surface_from_function_expr_with_substitutions(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    _type_params: &[verter_semantic::analysis::type_expr::TypeParam],
    substitutions: &FxHashMap<String, TypeExpr>,
) -> ProjectedSurface {
    if substitutions.is_empty() {
        return projected_surface_from_function_expr(function);
    }

    ProjectedSurface {
        members: Vec::new(),
        call_signatures: vec![TypeExpr::Function(std::sync::Arc::new(
            substitute_function_expr_if_needed(function, substitutions),
        ))],
        construct_signatures: Vec::new(),
        has_index_signature: false,
    }
}

fn projected_surface_from_parts_intersection(
    parts: Vec<std::sync::Arc<ProjectedSurface>>,
) -> PreparedSurfaceProjection {
    if parts.is_empty() {
        return PreparedSurfaceProjection::Empty;
    }

    let mut merged_members: FxHashMap<String, ProjectedMember> = FxHashMap::default();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;

    for surface in parts {
        let surface = projected_surface_unwrap_or_clone(surface);
        for member in surface.members {
            merged_members.entry(member.name.clone()).or_insert(member);
        }
        call_signatures.extend(surface.call_signatures);
        construct_signatures.extend(surface.construct_signatures);
        has_index_signature |= surface.has_index_signature;
    }

    let mut members = merged_members.into_values().collect::<Vec<_>>();
    members.sort_by(|left, right| left.name.cmp(&right.name));

    PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }))
}

fn projected_surface_from_parts_union(
    parts: Vec<std::sync::Arc<ProjectedSurface>>,
) -> PreparedSurfaceProjection {
    if parts.is_empty() {
        return PreparedSurfaceProjection::Empty;
    }

    let mut merged_members: FxHashMap<String, (ProjectedMember, usize)> = FxHashMap::default();
    let mut call_signatures = Vec::new();
    let mut construct_signatures = Vec::new();
    let mut has_index_signature = false;
    let mut total_surface_variants = 0usize;

    for surface in parts {
        let surface = projected_surface_unwrap_or_clone(surface);
        if projected_surface_is_empty(&surface) {
            continue;
        }
        total_surface_variants += 1;
        for member in surface.members {
            match merged_members.entry(member.name.clone()) {
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert((member, 1));
                }
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let (existing, seen_variants) = entry.get_mut();
                    *seen_variants += 1;
                    existing.optional = existing.optional || member.optional;
                    existing.readonly = existing.readonly && member.readonly;
                    existing.is_method = existing.is_method && member.is_method;
                    if existing.ty != member.ty {
                        existing.ty = TypeExpr::union(vec![existing.ty.clone(), member.ty]);
                    }
                }
            }
        }
        call_signatures.extend(surface.call_signatures);
        construct_signatures.extend(surface.construct_signatures);
        has_index_signature |= surface.has_index_signature;
    }

    if total_surface_variants == 0 {
        return PreparedSurfaceProjection::Empty;
    }

    let mut members = merged_members
        .into_values()
        .map(|(mut member, seen_variants)| {
            if seen_variants < total_surface_variants {
                member.optional = true;
            }
            member
        })
        .collect::<Vec<_>>();
    members.sort_by(|left, right| left.name.cmp(&right.name));

    PreparedSurfaceProjection::Surface(std::sync::Arc::new(ProjectedSurface {
        members,
        call_signatures,
        construct_signatures,
        has_index_signature,
    }))
}

fn apply_surface_member_modifier(
    projection: PreparedSurfaceProjection,
    mut mutate: impl FnMut(&mut ProjectedMember),
) -> PreparedSurfaceProjection {
    match projection {
        PreparedSurfaceProjection::Surface(surface) => {
            let mut surface = projected_surface_unwrap_or_clone(surface);
            for member in &mut surface.members {
                mutate(member);
            }
            PreparedSurfaceProjection::Surface(std::sync::Arc::new(surface))
        }
        PreparedSurfaceProjection::Empty => PreparedSurfaceProjection::Empty,
        PreparedSurfaceProjection::Unsupported => PreparedSurfaceProjection::Unsupported,
    }
}

fn apply_surface_member_filter(
    projection: PreparedSurfaceProjection,
    keep: impl Fn(&str) -> bool,
) -> PreparedSurfaceProjection {
    match projection {
        PreparedSurfaceProjection::Surface(surface) => {
            let mut surface = projected_surface_unwrap_or_clone(surface);
            surface.members.retain(|member| keep(member.name.as_str()));
            if projected_surface_is_empty(&surface) {
                PreparedSurfaceProjection::Empty
            } else {
                PreparedSurfaceProjection::Surface(std::sync::Arc::new(surface))
            }
        }
        PreparedSurfaceProjection::Empty => PreparedSurfaceProjection::Empty,
        PreparedSurfaceProjection::Unsupported => PreparedSurfaceProjection::Unsupported,
    }
}

fn projected_surface_unwrap_or_clone(
    surface: std::sync::Arc<ProjectedSurface>,
) -> ProjectedSurface {
    std::sync::Arc::try_unwrap(surface).unwrap_or_else(|shared| shared.as_ref().clone())
}

fn prepared_type_param_substitutions(
    prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
    type_arguments: &[TypeExpr],
) -> Option<FxHashMap<String, TypeExpr>> {
    if type_arguments.len() > prepared.type_parameters.len() {
        return None;
    }

    let mut substitutions = FxHashMap::default();
    for (index, type_parameter) in prepared.type_parameters.iter().enumerate() {
        let arg = if let Some(arg) = type_arguments.get(index) {
            arg.clone()
        } else if let Some(default) = type_parameter.default.as_deref() {
            default.clone()
        } else {
            continue;
        };
        if is_identity_type_param_binding(&arg, &type_parameter.name) {
            continue;
        }
        substitutions.insert(type_parameter.name.clone(), arg);
    }
    Some(substitutions)
}

fn is_identity_type_param_binding(expr: &TypeExpr, param_name: &str) -> bool {
    matches!(
        expr,
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() && name.as_ref() == param_name
    )
}

fn substitute_type_expr_if_needed(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> TypeExpr {
    if substitutions.is_empty() || !type_expr_references_substitutions(expr, substitutions) {
        expr.clone()
    } else {
        substitute_type_expr(expr, substitutions)
    }
}

fn substitute_function_expr_if_needed(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> verter_semantic::analysis::type_expr::FunctionExpr {
    if substitutions.is_empty() || !function_expr_references_substitutions(function, substitutions)
    {
        function.clone()
    } else {
        substitute_function_expr(function, substitutions)
    }
}

fn substituted_ref_expr_if_needed(
    expr: &TypeExpr,
    name: &str,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> Option<TypeExpr> {
    if substitutions.is_empty() {
        return None;
    }
    if let Some(substituted) = substitutions.get(name) {
        return Some(substituted.clone());
    }
    if !type_expr_references_substitutions(expr, substitutions) {
        return None;
    }
    assert_prepared_structural_substitution_slow_lane_allowed(expr);
    Some(substitute_type_expr(expr, substitutions))
}

fn substitute_type_expr(expr: &TypeExpr, substitutions: &FxHashMap<String, TypeExpr>) -> TypeExpr {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => substitutions
            .get(name.as_ref())
            .cloned()
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: std::sync::Arc::from(
                type_arguments
                    .iter()
                    .map(|arg| substitute_type_expr(arg, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
            substitute_type_expr(inner, substitutions),
        )),
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: std::sync::Arc::new(substitute_type_expr(element, substitutions)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: substitute_type_expr(&element.ty, substitutions),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => TypeExpr::Union(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(types) => TypeExpr::Intersection(std::sync::Arc::from(
            types
                .iter()
                .map(|ty| substitute_type_expr(ty, substitutions))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Object(object) => TypeExpr::Object(std::sync::Arc::new(
            verter_semantic::analysis::type_expr::ObjectExpr {
                properties: object
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(property) => ObjectMember::Property(
                            verter_semantic::analysis::type_expr::ObjectProperty {
                                name: property.name.clone(),
                                ty: substitute_type_expr(&property.ty, substitutions),
                                optional: property.optional,
                                readonly: property.readonly,
                            },
                        ),
                        ObjectMember::Method(method) => {
                            let mut method = method.clone();
                            for parameter in &mut method.function.parameters {
                                parameter.ty = substitute_type_expr(&parameter.ty, substitutions);
                            }
                            if let Some(return_type) = method.function.return_type.as_mut() {
                                *return_type = std::sync::Arc::new(substitute_type_expr(
                                    return_type,
                                    substitutions,
                                ));
                            }
                            ObjectMember::Method(method)
                        }
                        ObjectMember::IndexSignature(signature) => ObjectMember::IndexSignature(
                            verter_semantic::analysis::type_expr::IndexSignature {
                                key_name: signature.key_name.clone(),
                                key_type: substitute_type_expr(&signature.key_type, substitutions),
                                value_type: substitute_type_expr(
                                    &signature.value_type,
                                    substitutions,
                                ),
                                readonly: signature.readonly,
                            },
                        ),
                        ObjectMember::CallSignature(function) => ObjectMember::CallSignature(
                            substitute_function_expr(function, substitutions),
                        ),
                        ObjectMember::ConstructSignature(function) => {
                            ObjectMember::ConstructSignature(substitute_function_expr(
                                function,
                                substitutions,
                            ))
                        }
                    })
                    .collect(),
            },
        )),
        TypeExpr::Function(function) => TypeExpr::Function(std::sync::Arc::new(
            substitute_function_expr(function, substitutions),
        )),
        TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: std::sync::Arc::new(substitute_type_expr(object, substitutions)),
            index: std::sync::Arc::new(substitute_type_expr(index, substitutions)),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: std::sync::Arc::new(substitute_type_expr(check, substitutions)),
            extends: std::sync::Arc::new(substitute_type_expr(extends, substitutions)),
            true_type: std::sync::Arc::new(substitute_type_expr(true_type, substitutions)),
            false_type: std::sync::Arc::new(substitute_type_expr(false_type, substitutions)),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => {
            let mut scoped_substitutions = substitutions.clone();
            scoped_substitutions.remove(parameter.as_str());
            TypeExpr::Mapped {
                parameter: parameter.clone(),
                source: std::sync::Arc::new(substitute_type_expr(source, &scoped_substitutions)),
                value: std::sync::Arc::new(substitute_type_expr(value, &scoped_substitutions)),
                optional: *optional,
                readonly: *readonly,
                name_type: name_type.as_deref().map(|inner| {
                    std::sync::Arc::new(substitute_type_expr(inner, &scoped_substitutions))
                }),
            }
        }
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: std::sync::Arc::from(
                expressions
                    .iter()
                    .map(|inner| substitute_type_expr(inner, substitutions))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::Rest(inner) => TypeExpr::Rest(std::sync::Arc::new(substitute_type_expr(
            inner,
            substitutions,
        ))),
        TypeExpr::TypeParameter(type_parameter) => {
            let mut type_parameter = type_parameter.clone();
            if let Some(constraint) = type_parameter.constraint.as_mut() {
                *constraint = std::sync::Arc::new(substitute_type_expr(constraint, substitutions));
            }
            if let Some(default) = type_parameter.default.as_mut() {
                *default = std::sync::Arc::new(substitute_type_expr(default, substitutions));
            }
            TypeExpr::TypeParameter(type_parameter)
        }
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Infer { .. } => expr.clone(),
    }
}

fn substitute_function_expr(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> verter_semantic::analysis::type_expr::FunctionExpr {
    let mut scoped_substitutions = substitutions.clone();
    for type_parameter in &function.type_parameters {
        scoped_substitutions.remove(type_parameter.name.as_str());
    }

    let mut function = function.clone();
    for parameter in &mut function.parameters {
        parameter.ty = substitute_type_expr(&parameter.ty, &scoped_substitutions);
    }
    if let Some(return_type) = function.return_type.as_mut() {
        *return_type =
            std::sync::Arc::new(substitute_type_expr(return_type, &scoped_substitutions));
    }
    for type_parameter in &mut function.type_parameters {
        if let Some(constraint) = type_parameter.constraint.as_mut() {
            *constraint =
                std::sync::Arc::new(substitute_type_expr(constraint, &scoped_substitutions));
        }
        if let Some(default) = type_parameter.default.as_mut() {
            *default = std::sync::Arc::new(substitute_type_expr(default, &scoped_substitutions));
        }
    }
    function
}

fn function_expr_references_substitutions(
    function: &verter_semantic::analysis::type_expr::FunctionExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> bool {
    function.type_parameters.iter().any(|parameter| {
        parameter
            .constraint
            .as_deref()
            .is_some_and(|constraint| type_expr_references_substitutions(constraint, substitutions))
            || parameter
                .default
                .as_deref()
                .is_some_and(|default| type_expr_references_substitutions(default, substitutions))
    }) || function
        .parameters
        .iter()
        .any(|parameter| type_expr_references_substitutions(&parameter.ty, substitutions))
        || function.return_type.as_deref().is_some_and(|return_type| {
            type_expr_references_substitutions(return_type, substitutions)
        })
}

fn projected_surface_to_type_expr(surface: &ProjectedSurface) -> Option<TypeExpr> {
    use std::sync::Arc;
    use verter_semantic::analysis::type_expr::{
        FunctionExpr, IndexSignature, MethodSignature, ObjectExpr, ObjectMember, ObjectProperty,
        PrimitiveName,
    };

    if surface.members.is_empty()
        && surface.call_signatures.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
    {
        return None;
    }

    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return surface.call_signatures.first().cloned();
    }

    let mut properties = surface
        .members
        .iter()
        .map(|member| {
            if member.is_method {
                if let TypeExpr::Function(function) = &member.ty {
                    return ObjectMember::Method(MethodSignature {
                        name: member.name.clone(),
                        function: (**function).clone(),
                        optional: member.optional,
                    });
                }
            }

            ObjectMember::Property(ObjectProperty {
                name: member.name.clone(),
                ty: member.ty.clone(),
                optional: member.optional,
                readonly: member.readonly,
            })
        })
        .collect::<Vec<_>>();

    for signature in &surface.call_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::CallSignature(FunctionExpr {
                parameters: function.parameters.clone(),
                return_type: function.return_type.clone(),
                type_parameters: function.type_parameters.clone(),
            }));
        }
    }

    for signature in &surface.construct_signatures {
        if let TypeExpr::Function(function) = signature {
            properties.push(ObjectMember::ConstructSignature(FunctionExpr {
                parameters: function.parameters.clone(),
                return_type: function.return_type.clone(),
                type_parameters: function.type_parameters.clone(),
            }));
        }
    }

    if surface.has_index_signature {
        properties.push(ObjectMember::IndexSignature(IndexSignature {
            key_name: "key".to_string(),
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        }));
    }

    Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
}

fn projected_surface_to_expanded_shape(
    surface: &ProjectedSurface,
) -> verter_semantic::analysis::type_expand::ExpandedObjectShape {
    use verter_semantic::analysis::type_expand::{
        ExpandedCallSignature, ExpandedIndexSignature, ExpandedObjectShape, ExpandedParameter,
        ExpandedProperty,
    };
    use verter_semantic::analysis::type_expr::PrimitiveName;

    let properties = surface
        .members
        .iter()
        .map(|member| ExpandedProperty {
            name: member.name.clone(),
            ty: member.ty.clone(),
            optional: member.optional,
            readonly: member.readonly,
        })
        .collect::<Vec<_>>();

    let mut call_signatures = surface
        .call_signatures
        .iter()
        .chain(surface.construct_signatures.iter())
        .filter_map(|signature| match signature {
            TypeExpr::Function(function) => Some(ExpandedCallSignature {
                parameters: function
                    .parameters
                    .iter()
                    .map(|parameter| ExpandedParameter {
                        name: parameter.name.clone().unwrap_or_default(),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect(),
                return_type: function
                    .return_type
                    .as_ref()
                    .map(|return_type| return_type.as_ref().clone())
                    .unwrap_or(TypeExpr::Primitive(PrimitiveName::Void)),
                type_parameters: function.type_parameters.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut index_signatures = Vec::new();
    if surface.has_index_signature {
        index_signatures.push(ExpandedIndexSignature {
            key_type: TypeExpr::Primitive(PrimitiveName::String),
            value_type: TypeExpr::Unknown {
                raw: "projectedOpenSurface".to_string(),
            },
            readonly: false,
        });
    }

    // Preserve previous round-trip behavior: call and construct signatures
    // both become call signatures after object-shape extraction.
    if !surface.call_signatures.is_empty() && !surface.construct_signatures.is_empty() {
        call_signatures.shrink_to_fit();
    }

    ExpandedObjectShape {
        properties,
        index_signatures,
        call_signatures,
    }
}

fn routed_expr_surface_key_expr(root_symbol: &str, route: &super::RouteDemand) -> Option<TypeExpr> {
    match route {
        super::RouteDemand::Whole => Some(TypeExpr::named(root_symbol)),
        super::RouteDemand::MemberPath(path) if !path.is_empty() => Some(path.iter().fold(
            TypeExpr::named(root_symbol),
            |object, member| TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(object),
                index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
            },
        )),
        super::RouteDemand::Pick(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Pick"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        super::RouteDemand::Omit(members) if !members.is_empty() => Some(TypeExpr::Ref {
            name: std::sync::Arc::from("Omit"),
            type_arguments: std::sync::Arc::from(vec![
                TypeExpr::named(root_symbol),
                TypeExpr::union(
                    members
                        .iter()
                        .cloned()
                        .map(TypeExpr::string_literal)
                        .collect(),
                ),
            ]),
        }),
        _ => None,
    }
}

fn materialized_member_surface_key(
    scope_canonical_id: &str,
    expr: &TypeExpr,
    nested_surface: bool,
) -> Option<MaterializedMemberSurfaceKey> {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => Some(MaterializedMemberSurfaceKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            target: MaterializedMemberSurfaceTarget::Symbol(name.to_string()),
            nested_surface,
        }),
        _ => super::component_meta_registry::component_meta_registry_public_indexed_access_route(
            expr,
        )
        .map(|(root_symbol, route)| MaterializedMemberSurfaceKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            target: MaterializedMemberSurfaceTarget::RoutedMember { root_symbol, route },
            nested_surface,
        })
        .or_else(|| {
            materialized_member_surface_structural_cacheable(expr).then(|| {
                MaterializedMemberSurfaceKey {
                    scope_canonical_id: scope_canonical_id.to_string(),
                    target: MaterializedMemberSurfaceTarget::Structural(expr.clone()),
                    nested_surface,
                }
            })
        }),
    }
}

fn materialized_member_surface_structural_cacheable(expr: &TypeExpr) -> bool {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. } => true,
        TypeExpr::Ref { type_arguments, .. } => type_arguments.is_empty(),
        TypeExpr::IndexedAccess { .. } => {
            super::component_meta_registry::component_meta_registry_public_indexed_access_route(
                expr,
            )
            .is_some()
        }
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => materialized_member_surface_structural_cacheable(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| materialized_member_surface_structural_cacheable(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .all(materialized_member_surface_structural_cacheable),
        TypeExpr::Object(object) => object.properties.iter().all(|member| match member {
            ObjectMember::Property(property) => {
                materialized_member_surface_structural_cacheable(&property.ty)
            }
            ObjectMember::IndexSignature(signature) => {
                materialized_member_surface_structural_cacheable(&signature.key_type)
                    && materialized_member_surface_structural_cacheable(&signature.value_type)
            }
            ObjectMember::CallSignature(function) | ObjectMember::ConstructSignature(function) => {
                function.parameters.iter().all(|parameter| {
                    materialized_member_surface_structural_cacheable(&parameter.ty)
                }) && function.return_type.as_deref().is_none_or(|return_type| {
                    materialized_member_surface_structural_cacheable(return_type)
                })
            }
            ObjectMember::Method(method) => {
                method.function.parameters.iter().all(|parameter| {
                    materialized_member_surface_structural_cacheable(&parameter.ty)
                }) && method
                    .function
                    .return_type
                    .as_deref()
                    .is_none_or(|return_type| {
                        materialized_member_surface_structural_cacheable(return_type)
                    })
            }
        }),
        TypeExpr::Function(function) => {
            function
                .parameters
                .iter()
                .all(|parameter| materialized_member_surface_structural_cacheable(&parameter.ty))
                && function.return_type.as_deref().is_none_or(|return_type| {
                    materialized_member_surface_structural_cacheable(return_type)
                })
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .all(materialized_member_surface_structural_cacheable),
        TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => false,
    }
}

fn filter_identity_ref(result: &SolverResult<TypeExpr>, requested_name: &str) -> Option<TypeExpr> {
    match &result.value {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == requested_name && type_arguments.is_empty() => None,
        _ => Some(result.value.clone()),
    }
}

fn is_package_source(source: Option<&str>) -> bool {
    source.is_some_and(|s| s.contains("/node_modules/"))
}

fn is_direct_surface_no_deps(
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
) -> bool {
    let direct_surface = matches!(
        &prepared.body,
        TypeExpr::Object(_)
            | TypeExpr::Union(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::Function(_)
            | TypeExpr::Mapped { .. }
    );
    direct_surface
        && prepared.local_deps.is_empty()
        && (prepared.external_deps.is_empty()
            || prepared
                .external_deps
                .iter()
                .all(|dep| dep.canonical_id.contains("/node_modules/")))
}

fn prepared_member_body_stays_shallow(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::Infer { .. }
        | TypeExpr::TypeOf(_) => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            prepared_member_body_stays_shallow(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            !types.is_empty() && types.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Array { element, .. } => prepared_member_body_stays_shallow(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|element| prepared_member_body_stays_shallow(&element.ty)),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().all(prepared_member_body_stays_shallow)
        }
        TypeExpr::Function(function) => {
            function.type_parameters.iter().all(|parameter| {
                parameter
                    .constraint
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
                    && parameter
                        .default
                        .as_deref()
                        .is_none_or(prepared_member_body_stays_shallow)
            }) && function
                .parameters
                .iter()
                .all(|parameter| prepared_member_body_stays_shallow(&parameter.ty))
                && function
                    .return_type
                    .as_deref()
                    .is_none_or(prepared_member_body_stays_shallow)
        }
        TypeExpr::Ref { .. }
        | TypeExpr::Object(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::RecursiveRef { .. } => false,
    }
}

fn prepared_decl_keeps_raw_symbolic_non_object_alias(
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    expr: &TypeExpr,
) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            prepared
                .name_resolution
                .get(name.as_ref())
                .is_some_and(|resolved| resolved.canonical_id.contains("/node_modules/"))
                && type_arguments
                    .iter()
                    .all(|arg| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, arg))
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, element)
        }
        TypeExpr::Tuple { elements, .. } => elements.iter().all(|element| {
            prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &element.ty)
        }),
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => types
            .iter()
            .all(|ty| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, ty)),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .all(|param| prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, &param.ty))
                && func.return_type.as_deref().is_none_or(|return_type| {
                    prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, return_type)
                })
                && func.type_parameters.iter().all(|param| {
                    param.constraint.as_deref().is_none_or(|constraint| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, constraint)
                    }) && param.default.as_deref().is_none_or(|default| {
                        prepared_decl_keeps_raw_symbolic_non_object_alias(prepared, default)
                    })
                })
        }
        TypeExpr::Object(object) => object.properties.is_empty(),
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TypeOf(_) => false,
    }
}

fn is_builtin_name(name: &str) -> bool {
    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name).is_some()
        || matches!(name, "Array" | "ReadonlyArray" | "Promise")
}

fn prepared_type_decl_canonical_dependencies(
    resolved_id: &str,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
) -> BTreeSet<String> {
    let mut canonical_dependencies = BTreeSet::from([resolved_id.to_string()]);
    if let Some((defining_file, _)) = prepared.cache_deps.defining_file.as_ref() {
        canonical_dependencies.insert(defining_file.clone());
    }
    for (participant, _) in &prepared.cache_deps.barrel_participants {
        canonical_dependencies.insert(participant.clone());
    }
    for dep in &prepared.external_deps {
        if !dep.canonical_id.is_empty() {
            canonical_dependencies.insert(dep.canonical_id.clone());
        }
    }
    for identity in prepared.name_resolution.values() {
        if !identity.canonical_id.is_empty() {
            canonical_dependencies.insert(identity.canonical_id.clone());
        }
    }
    canonical_dependencies
}

fn resolve_imported_registry_symbol_with_budget<F>(
    host: &VerterHost,
    canonical_id: &str,
    exported_name: &str,
    store_view: Option<&HostStoreView>,
    mut allow_route: F,
) -> Option<ResolvedImportedRegistrySymbol>
where
    F: FnMut() -> bool,
{
    let (resolved_id, resolved_name) = if host
        .prepared_type_decl_in_view(canonical_id, exported_name, store_view)
        .is_some()
    {
        (canonical_id.to_string(), exported_name.to_string())
    } else {
        if !allow_route() {
            return None;
        }
        host.resolve_named_type_export_target_shallow_in_view(
            canonical_id,
            exported_name,
            store_view,
        )?
    };

    let prepared = host.prepared_type_decl_in_view(&resolved_id, &resolved_name, store_view)?;

    Some(ResolvedImportedRegistrySymbol {
        canonical_id: resolved_id.clone(),
        exported_name: resolved_name,
        body: prepared.body.clone(),
        canonical_dependencies: prepared_type_decl_canonical_dependencies(
            resolved_id.as_str(),
            prepared.as_ref(),
        ),
    })
}

fn type_expr_references_type_params(
    expr: &TypeExpr,
    type_params: &[verter_semantic::analysis::type_expr::TypeParam],
) -> bool {
    type_expr_references_names(expr, &|name| {
        type_params.iter().any(|param| param.name == name)
    })
}

fn type_expr_references_substitutions(
    expr: &TypeExpr,
    substitutions: &FxHashMap<String, TypeExpr>,
) -> bool {
    type_expr_references_names(expr, &|name| substitutions.contains_key(name))
}

fn type_expr_references_names(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
    fn visit(expr: &TypeExpr, contains_name: &impl Fn(&str) -> bool) -> bool {
        match expr {
            TypeExpr::Primitive(_)
            | TypeExpr::Literal(_)
            | TypeExpr::Unknown { .. }
            | TypeExpr::RecursiveRef { .. }
            | TypeExpr::TypeOf(_)
            | TypeExpr::Infer { .. } => false,
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                contains_name(name.as_ref())
                    || type_arguments.iter().any(|arg| visit(arg, contains_name))
            }
            TypeExpr::TypeParameter(param) => {
                contains_name(param.name.as_str())
                    || param
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, contains_name))
                    || param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, contains_name))
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => visit(inner, contains_name),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, contains_name)),
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, contains_name)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, contains_name)
                }
                verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, contains_name)
                        || visit(&signature.value_type, contains_name)
                }
                verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                    function,
                ) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, contains_name))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, contains_name))
                }
            }),
            TypeExpr::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, contains_name))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, contains_name))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, contains_name))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, contains_name))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, contains_name) || visit(index, contains_name)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, contains_name)
                    || visit(extends, contains_name)
                    || visit(true_type, contains_name)
                    || visit(false_type, contains_name)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, contains_name)
                    || visit(value, contains_name)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, contains_name))
            }
        }
    }

    visit(expr, contains_name)
}

#[cfg(test)]
mod tests {
    use super::forbid_direct_pick_routed_expr_slow_lane_for_tests;
    use super::ComponentMetaQueryEngine;
    use super::{
        forbid_prepared_structural_substitution_slow_lane_for_tests,
        prepared_substitution_instantiation_hash, type_expr_references_type_params,
    };
    use crate::resolver_core::solver_host::SessionSolverHost;
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared declaration should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface
        );
        assert!(
            declaration
                .text
                .as_deref()
                .is_some_and(|text| text.contains("interface AvatarProps")),
            "direct prepared declaration should still recover the local declaration text",
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

        let declaration = engine
            .resolve_direct_prepared_type_declaration_metadata("/src/Avatar.vue", "AvatarProps")
            .expect("direct prepared metadata should resolve");

        assert_eq!(declaration.canonical_source, "/src/Avatar.vue");
        assert_eq!(declaration.resolved_name, "AvatarProps");
        assert_eq!(
            declaration.kind,
            crate::resolver_core::ResolvedDeclarationKind::Interface
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

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
            engine.solve_count(),
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

        let projected = engine
            .project_prepared_member_route_surface_expr("/workspace/src/Link.vue", "Props", "to")
            .expect("prepared package member route should project");

        assert_eq!(
            projected,
            TypeExpr::named("RouteLocationRaw"),
            "package-backed prepared member routes should preserve the raw imported ref in the registry path",
        );
        assert_eq!(
            engine.solve_count(),
            0,
            "package-backed prepared member routes should stay shallow instead of invoking solver projection",
        );
    }

    #[test]
    fn project_prepared_type_surface_shape_keeps_imported_package_projection_off_module_facts() {
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

        let view = host.resolver_store_view();
        let solver_host = SessionSolverHost::new(&host, Some(&view));
        let mut engine = ComponentMetaQueryEngine::new(&host, Some(&view), &solver_host);

        let shape = engine
            .project_prepared_type_surface_shape("/workspace/src/Child.vue", "Wrapper")
            .expect("prepared package wrapper projection should resolve");

        assert!(
            shape.properties.iter().any(|property| property.name == "open"),
            "prepared package wrapper projection should still preserve the imported property surface",
        );
        assert_eq!(
            engine.solve_count(),
            0,
            "prepared package wrapper projection should stay on shallow projection without solver fallback",
        );
        assert!(
            host.resolver
                .runtime
                .module_facts
                .get_any("/workspace/node_modules/pkg/dist/index.d.ts")
                .is_none(),
            "prepared package projection should keep the provider barrel off ModuleFactsDb",
        );
        assert!(
            host.resolver
                .runtime
                .module_facts
                .get_any("/workspace/node_modules/pkg/dist/index3.d.ts")
                .is_none(),
            "prepared package projection should keep the routed package target off ModuleFactsDb",
        );
        assert!(
            host.resolver
                .runtime
                .module_facts
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let expr = TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("Tabs")),
                index: Arc::new(TypeExpr::string_literal("variants")),
            }),
            index: Arc::new(TypeExpr::string_literal("color")),
        };

        let projected = query_engine
            .project_expr_surface_expr("/src/App.vue", &expr)
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let first = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("generic inherited omit surface should project");
        let surface_cache_after_first = query_engine.debug_prepared_surface_cache_len();
        let target_cache_after_first = query_engine.debug_prepared_target_cache_len();
        assert!(
            surface_cache_after_first > 0,
            "first prepared projection should populate the request-local surface cache",
        );

        let second = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
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
            query_engine.solve_count(),
            0,
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

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
            query_engine.solve_count(),
            0,
            "shared prepared surface handles must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_reuses_shared_root_surface_cache_across_requests() {
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};

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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));

        let mut first_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let first = first_query
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("first prepared projection should succeed");
        assert_eq!(
            first_query.debug_prepared_root_surface_projection_count(),
            1,
            "first query should compute the prepared root surface once",
        );

        let stable_surface_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: "/src/App.vue".to_string(),
            symbol_name: "ColorModeSelectProps".to_string(),
            instantiation_hash: 0,
            context_hash: 0,
        });
        assert!(
            matches!(
                host.resolver_runtime()
                    .type_surfaces
                    .get(&stable_surface_key, &store_view)
                    .as_deref(),
                Some(TypeSurfaceOpResult::Surface(_))
            ),
            "prepared root surfaces should publish into the shared type-surface DB for later requests",
        );

        let mut second_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let second = second_query
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("repeat prepared projection should reuse the shared cache");

        assert_eq!(second, first);
        assert_eq!(
            second_query.debug_prepared_root_surface_projection_count(),
            0,
            "warm prepared root-surface lookups should reuse the shared DB instead of recomputing the projection",
        );
        assert_eq!(
            second_query.solve_count(),
            0,
            "shared prepared root-surface reuse must stay off the semantic solver",
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let expr_surface = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared surface should project");
        let direct_shape = query_engine
            .project_prepared_type_surface_shape("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared shape should project");

        assert_eq!(
            direct_shape,
            verter_semantic::analysis::type_expand::type_expr_to_object_shape(&expr_surface),
            "direct prepared shape projection should match the previous type-expr roundtrip",
        );
        assert_eq!(
            query_engine.solve_count(),
            0,
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let projected = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("prepared surface should project");

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );
        assert_eq!(
            query_engine.debug_prepared_type_decl_query_count(),
            3,
            "one projection should only query each prepared declaration once: ColorModeSelectProps, SelectMenuProps, and RootProps",
        );
        assert_eq!(
            query_engine.solve_count(),
            0,
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let identity_surface = query_engine
            .project_prepared_type_surface_expr("/src/App.vue", "IdentityProps")
            .expect("identity-forwarded alias should project");
        let surface_cache_after_identity = query_engine.debug_prepared_surface_cache_len();

        let root_surface = query_engine
            .project_prepared_type_surface_expr("/src/base.ts", "RootProps")
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
            query_engine.solve_count(),
            0,
            "identity-forwarded cache reuse must stay solver-free",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_publishes_stable_imported_generic_members_for_cross_request_reuse(
    ) {
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};
        use verter_semantic::analysis::type_expr::TypeExpr as MetaTypeExpr;

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  disabled?: boolean
  value?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/shared.ts".to_string(),
            Arc::from(
                r#"
import type { RootProps } from './base'

export interface SelectMenuProps<T> extends Pick<RootProps<T>, 'open' | 'disabled'> {
  items?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/ColorModeSelect.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SelectMenuProps } from './shared'

type Item = { label?: string }

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/InputMenu.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SelectMenuProps } from './shared'

type Item = { label?: string }

export interface InputMenuProps extends Pick<SelectMenuProps<Item[]>, 'open'> {}
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
        assert!(host.ensure_loaded("/src/ColorModeSelect.vue"));
        assert!(host.ensure_loaded("/src/InputMenu.vue"));

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));

        let mut first_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let first = first_query
            .project_prepared_type_surface_expr("/src/ColorModeSelect.vue", "ColorModeSelectProps")
            .expect("first query should project the imported generic pick members");
        assert!(
            matches!(first, TypeExpr::Object(_)),
            "first query should still materialize the prepared object surface",
        );
        let mut substitutions = FxHashMap::default();
        substitutions.insert(
            "T".to_string(),
            MetaTypeExpr::Array {
                element: std::sync::Arc::new(MetaTypeExpr::named("Item")),
                readonly: false,
            },
        );
        let instantiation_hash = prepared_substitution_instantiation_hash(&substitutions);

        let stable_member_key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: "/src/base.ts".to_string(),
                symbol_name: "RootProps".to_string(),
                instantiation_hash,
                context_hash: 0,
            },
            member_name: "open".to_string(),
        };
        assert!(
            matches!(
                host.resolver_runtime()
                    .type_surfaces
                    .get(&stable_member_key, &store_view)
                    .as_deref(),
                Some(TypeSurfaceOpResult::Member(_))
            ),
            "stable imported generic members should publish into the shared type-surface DB after the first query",
        );

        let dependent_member_key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: "/src/base.ts".to_string(),
                symbol_name: "RootProps".to_string(),
                instantiation_hash,
                context_hash: 0,
            },
            member_name: "value".to_string(),
        };
        assert!(
            host.resolver_runtime()
                .type_surfaces
                .get(&dependent_member_key, &store_view)
                .is_none(),
            "generic-dependent members must stay out of the shared cache because their projected meaning depends on the caller substitutions",
        );

        let mut second_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let second = second_query
            .project_prepared_type_surface_expr("/src/InputMenu.vue", "InputMenuProps")
            .expect("second query should reuse the cached imported member route");
        let TypeExpr::Object(object) = second else {
            panic!("second query should still project an object surface");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    Some(method.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["open"]),
            "cross-request reuse should stay on the requested imported member route only",
        );
        assert_eq!(
            second_query.debug_prepared_shared_member_hit_count(),
            1,
            "the second query should hit the shared imported member cache exactly once for RootProps['open']",
        );
        assert_eq!(
            second_query.solve_count(),
            0,
            "cross-request member reuse must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_publishes_stable_imported_generic_surfaces_for_cross_request_reuse(
    ) {
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};
        use verter_semantic::analysis::type_expr::TypeExpr as MetaTypeExpr;

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/base.ts".to_string(),
            Arc::from(
                r#"
export interface RootProps<T> {
  open?: boolean
  disabled?: boolean
  value?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/shared.ts".to_string(),
            Arc::from(
                r#"
import type { RootProps } from './base'

export interface SelectMenuProps<T> extends Pick<RootProps<T>, 'open' | 'disabled'> {
  items?: T
}
"#,
            ),
        );
        ws.inject_file(
            "/src/ColorModeSelect.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SelectMenuProps } from './shared'

type Item = { label?: string }

export interface ColorModeSelectProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/ColorModeSelectCopy.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SelectMenuProps } from './shared'

type Item = { label?: string }

export interface ColorModeSelectCopyProps extends Omit<SelectMenuProps<Item[]>, 'items'> {}
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
        assert!(host.ensure_loaded("/src/ColorModeSelect.vue"));
        assert!(host.ensure_loaded("/src/ColorModeSelectCopy.vue"));

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));

        let mut first_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let first = first_query
            .project_prepared_type_surface_expr("/src/ColorModeSelect.vue", "ColorModeSelectProps")
            .expect("first query should project the imported generic omit surface");
        assert!(
            matches!(first, TypeExpr::Object(_)),
            "first query should still materialize the prepared object surface",
        );

        let mut substitutions = FxHashMap::default();
        substitutions.insert(
            "T".to_string(),
            MetaTypeExpr::Array {
                element: std::sync::Arc::new(MetaTypeExpr::named("Item")),
                readonly: false,
            },
        );
        let instantiation_hash = prepared_substitution_instantiation_hash(&substitutions);
        let stable_surface_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: "/src/shared.ts".to_string(),
            symbol_name: "SelectMenuProps".to_string(),
            instantiation_hash,
            context_hash: 0,
        });
        assert!(
            matches!(
                host.resolver_runtime()
                    .type_surfaces
                    .get(&stable_surface_key, &store_view)
                    .as_deref(),
                Some(TypeSurfaceOpResult::Surface(_))
            ),
            "stable imported generic surfaces should publish into the shared type-surface DB after the first query",
        );

        let mut second_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let second = second_query
            .project_prepared_type_surface_expr(
                "/src/ColorModeSelectCopy.vue",
                "ColorModeSelectCopyProps",
            )
            .expect("second query should reuse the cached imported generic surface");
        let TypeExpr::Object(object) = second else {
            panic!("second query should still project an object surface");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    Some(method.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["disabled", "open"]),
            "cross-request reuse should keep the imported generic whole-surface projection exact",
        );
        assert!(
            second_query.debug_prepared_shared_surface_hit_count() > 0,
            "second query should reuse at least one shared imported generic surface instead of reprojecting it request-locally",
        );
        assert_eq!(
            second_query.solve_count(),
            0,
            "cross-request whole-surface reuse must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_keeps_non_generic_imported_surfaces_request_local() {
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey};

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/shared.ts".to_string(),
            Arc::from(
                r#"
export interface SharedProps {
  open?: boolean
  disabled?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/A.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SharedProps } from './shared'

export interface AProps extends SharedProps {}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/B.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SharedProps } from './shared'

export interface BProps extends SharedProps {}
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
        assert!(host.ensure_loaded("/src/A.vue"));
        assert!(host.ensure_loaded("/src/B.vue"));

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));

        let mut first_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let first = first_query
            .project_prepared_type_surface_expr("/src/A.vue", "AProps")
            .expect("first query should project the imported surface");
        assert!(
            matches!(first, TypeExpr::Object(_)),
            "first query should still materialize the prepared object surface",
        );

        let stable_surface_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: "/src/shared.ts".to_string(),
            symbol_name: "SharedProps".to_string(),
            instantiation_hash: 0,
            context_hash: 0,
        });
        assert!(
            host.resolver_runtime()
                .type_surfaces
                .get(&stable_surface_key, &store_view)
                .is_none(),
            "non-generic imported whole surfaces should stay request-local instead of prepublishing root-cache entries from nested projection",
        );

        let mut second_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let second = second_query
            .project_prepared_type_surface_expr("/src/B.vue", "BProps")
            .expect("second query should still project the imported surface");
        let TypeExpr::Object(object) = second else {
            panic!("second query should still project an object surface");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    Some(method.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["disabled", "open"]),
            "non-generic imported whole-surface projection should stay exact even without shared nested-surface reuse",
        );
        assert!(
            second_query.debug_prepared_shared_surface_hit_count() == 0,
            "non-generic imported whole surfaces should not hit the shared nested-surface cache",
        );
        assert_eq!(
            second_query.solve_count(),
            0,
            "request-local non-generic imported whole-surface projection must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_publishes_package_backed_non_generic_surfaces_for_cross_request_reuse(
    ) {
        use crate::resolver_core::{TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult};

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from(
                r#"
export interface SharedProps {
  open?: boolean
  disabled?: boolean
}
"#,
            ),
        );
        ws.inject_file(
            "/src/A.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SharedProps } from 'pkg'

export interface AProps extends SharedProps {}
</script>
<template><div /></template>"#,
            ),
        );
        ws.inject_file(
            "/src/B.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
import type { SharedProps } from 'pkg'

export interface BProps extends SharedProps {}
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
        assert!(host.ensure_loaded("/src/A.vue"));
        assert!(host.ensure_loaded("/src/B.vue"));
        host.set_import_dependencies(
            "/src/A.vue",
            vec![crate::DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        host.set_import_dependencies(
            "/src/B.vue",
            vec![crate::DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));

        let mut first_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let first = first_query
            .project_prepared_type_surface_expr("/src/A.vue", "AProps")
            .expect("first query should project the imported package surface");
        assert!(
            matches!(first, TypeExpr::Object(_)),
            "first query should still materialize the prepared object surface",
        );

        let stable_surface_key = TypeSurfaceOpKey::Surface(TypeSurfaceKey {
            canonical_owner: "/src/node_modules/pkg/index.d.ts".to_string(),
            symbol_name: "SharedProps".to_string(),
            instantiation_hash: 0,
            context_hash: 0,
        });
        assert!(
            matches!(
                host.resolver_runtime()
                    .type_surfaces
                    .get(&stable_surface_key, &store_view)
                    .as_deref(),
                Some(TypeSurfaceOpResult::Surface(_))
            ),
            "package-backed imported whole surfaces should publish into the shared type-surface DB after the first query",
        );

        let mut second_query =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let second = second_query
            .project_prepared_type_surface_expr("/src/B.vue", "BProps")
            .expect("second query should reuse the cached imported package surface");
        let TypeExpr::Object(object) = second else {
            panic!("second query should still project an object surface");
        };
        let member_names: std::collections::BTreeSet<_> = object
            .properties
            .iter()
            .filter_map(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    Some(property.name.as_str())
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    Some(method.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            member_names,
            std::collections::BTreeSet::from(["disabled", "open"]),
            "cross-request package-backed whole-surface reuse should keep the imported projection exact",
        );
        assert!(
            second_query.debug_prepared_shared_surface_hit_count() > 0,
            "second query should reuse at least one shared imported package surface instead of reprojecting it request-locally",
        );
        assert_eq!(
            second_query.solve_count(),
            0,
            "cross-request package-backed whole-surface reuse must stay off the semantic solver",
        );
    }

    #[test]
    fn project_prepared_type_surface_expr_keeps_local_generic_members_request_local() {
        use crate::resolver_core::type_surface_db::{TypeSurfaceKey, TypeSurfaceOpKey};

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                r#"<script lang="ts">
interface RootProps<T> {
  open?: boolean
  value?: T
}

type Item = { label?: string }

export interface AppProps extends Pick<RootProps<Item[]>, 'open'> {}
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query = ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let projected = query
            .project_prepared_type_surface_expr("/src/App.vue", "AppProps")
            .expect("local prepared pick should still project");
        let TypeExpr::Object(object) = projected else {
            panic!("local prepared pick should project an object surface");
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
            std::collections::BTreeSet::from(["open"]),
            "local generic picks should still stay on the requested member route",
        );

        let mut substitutions = FxHashMap::default();
        substitutions.insert(
            "T".to_string(),
            TypeExpr::Array {
                element: std::sync::Arc::new(TypeExpr::named("Item")),
                readonly: false,
            },
        );
        let key = TypeSurfaceOpKey::Member {
            subject: TypeSurfaceKey {
                canonical_owner: "/src/App.vue".to_string(),
                symbol_name: "RootProps".to_string(),
                instantiation_hash: prepared_substitution_instantiation_hash(&substitutions),
                context_hash: 0,
            },
            member_name: "open".to_string(),
        };
        assert!(
            host.resolver_runtime()
                .type_surfaces
                .get(&key, &store_view)
                .is_none(),
            "same-file generic members should stay request-local instead of publishing into the shared type-surface DB",
        );
        assert_eq!(
            query.debug_prepared_shared_member_hit_count(),
            0,
            "same-file generic member projection should not consult the shared imported-member cache",
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route = crate::resolver_core::RouteDemand::Pick(vec![
            "open".to_string(),
            "defaultOpen".to_string(),
            "disabled".to_string(),
        ]);

        let first = query_engine
            .project_route_surface_expr("/src/base.ts", "Props", &route)
            .expect("prepared pick route should project");
        let member_cache_after_first = query_engine.debug_prepared_member_cache_len();
        assert!(
            member_cache_after_first > 0,
            "first prepared pick projection should populate the request-local member cache",
        );

        let second = query_engine
            .project_route_surface_expr("/src/base.ts", "Props", &route)
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
        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
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
            query_engine.solve_count(),
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
        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
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
            query_engine.solve_count(),
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
        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["to".to_string(), "target".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
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
            query_engine.solve_count(),
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
        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
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
            query_engine.solve_count(),
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
        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);
        let route =
            crate::resolver_core::RouteDemand::Pick(vec!["target".to_string(), "to".to_string()]);

        let _guard = forbid_direct_pick_routed_expr_slow_lane_for_tests();
        let projected = query_engine
            .project_route_surface_expr("/src/Link.vue", "LinkProps", &route)
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
            query_engine.solve_count(),
            0,
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let projected = query_engine
            .project_type_surface_expr("/src/EditorToolbar.vue", "EditorToolbarProps")
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
            query_engine.solve_count(),
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let projected = query_engine
            .project_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
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
            query_engine.solve_count(),
            0,
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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        let projected = query_engine
            .project_type_surface_expr("/src/App.vue", "ColorModeSelectProps")
            .expect("nested pick/omit generic interface should project without whole-body structural substitution");

        assert!(
            matches!(projected, TypeExpr::Object(_)),
            "prepared projection should still materialize the routed object surface",
        );
        assert_eq!(
            query_engine.solve_count(),
            0,
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut query_engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

        let projected =
            query_engine.project_prepared_type_surface_expr("/src/types.ts", "ComboboxRootProps");
        assert!(
            projected.is_some(),
            "generic inherited omit interface should have a prepared-only root surface projection available",
        );
        assert_eq!(
            query_engine.solve_count(),
            0,
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);

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

        let store_view = host.resolver_store_view();
        let owner_solver_host = SessionSolverHost::new(&host, Some(&store_view));
        let mut query_engine =
            ComponentMetaQueryEngine::new(&host, Some(&store_view), &owner_solver_host);

        let _guard = forbid_prepared_structural_substitution_slow_lane_for_tests();
        assert!(
            query_engine
                .project_prepared_type_surface_expr("/src/App.vue", "Concrete")
                .is_none(),
            "unbound generic forwarding should stay symbolic instead of taking the structural substitution slow lane",
        );
        assert_eq!(
            query_engine.solve_count(),
            0,
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

        let solver_host = SessionSolverHost::new(&host, None);
        let mut engine = ComponentMetaQueryEngine::new(&host, None, &solver_host);
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
}
