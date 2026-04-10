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

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedSurface, TypeQueryEngine};
use verter_semantic::analysis::type_solver::result::SolverResult;

use super::declaration_metadata::ResolvedTypeDeclaration;
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
    symbol_name: String,
    nested_surface: bool,
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
    fuse_budgets: FuseBudgets,
    fuse_state: FuseState,
}

#[cfg(test)]
static FORBID_STRUCTURAL_SLOW_LANE: AtomicBool = AtomicBool::new(false);

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
fn assert_structural_slow_lane_allowed() {
    assert!(
        !FORBID_STRUCTURAL_SLOW_LANE.load(Ordering::SeqCst),
        "component-meta structural slow lane should not be used on the DB-backed production path",
    );
}

#[cfg(not(test))]
fn assert_structural_slow_lane_allowed() {}

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
            scoped_cache: FxHashMap::default(),
            imported_registry_symbols: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
            scope_payloads: FxHashMap::default(),
            materialized_member_surfaces: FxHashMap::default(),
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
            scoped_cache: FxHashMap::default(),
            imported_registry_symbols: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
            scope_payloads: FxHashMap::default(),
            materialized_member_surfaces: FxHashMap::default(),
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

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        self.declarations
            .entry(key)
            .or_insert_with_key(|_| {
                crate::meta_resolve::resolve_type_declaration_in_view(
                    self.host,
                    canonical_source,
                    requested_name,
                    self.store_view,
                )
            })
            .clone()
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
        let resolved = if self
            .host
            .prepared_type_decl_in_view(source_key, exported_name, self.store_view)
            .is_some()
        {
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
        self.owner_collection_exprs
            .entry(name.to_string())
            .or_insert_with_key(|_| {
                self.host
                    .prepared_type_decl_in_view(owner_canonical, name, self.store_view)
                    .map(|prepared| prepared.body.clone())
            })
            .clone()
    }

    pub fn named_decl_body(&self, canonical_id: &str, name: &str) -> Option<TypeExpr> {
        self.host
            .prepared_type_decl_in_view(canonical_id, name, self.store_view)
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
                .host
                .prepared_type_decl_in_view(scope_canonical_id, requested_name, self.store_view)
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

        if let Some(prepared) = self.host.prepared_type_decl_in_view(
            scope_canonical_id,
            requested_name,
            self.store_view,
        ) {
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

    pub fn trace_summary(
        &self,
    ) -> &verter_semantic::analysis::type_solver::query_engine::SolverTraceSummary {
        &self.owner_engine.trace_summary
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
                        .project_surface(subject_id, &solver_host, scope_canonical_id)
                        .map(|surface| (TypeSurfaceOpResult::Surface(surface), facts.clone()))
                })?;
            return cached.as_surface().cloned();
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
        let solver_host = self.solver_host_for_scope(scope_canonical_id);
        self.owner_engine
            .project_expr_surface_as_type_expr(&solver_host, scope_canonical_id, expr)
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
            symbol_name: name.to_string(),
            nested_surface,
        }),
        _ => None,
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
        host.resolve_named_type_export_target_in_view(canonical_id, exported_name, store_view)?
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
