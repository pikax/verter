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
use verter_semantic::analysis::type_eval::DeclarationId;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::query_engine::{ProjectedSurface, TypeQueryEngine};
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

    pub fn resolve_direct_prepared_type_declaration(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        if self
            .host
            .prepared_type_decl_in_view(canonical_source, resolved_name, self.store_view)
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
            .host
            .prepared_type_decl_in_view(canonical_source, resolved_name, self.store_view)
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
        use crate::resolver_core::type_surface_db::{
            TypeSurfaceKey, TypeSurfaceOpKey, TypeSurfaceOpResult,
        };

        if let super::RouteDemand::MemberPath(path) = route {
            if let [member_name] = path.as_slice() {
                if let Some(projected_expr) = self.project_prepared_member_route_surface_expr(
                    scope_canonical_id,
                    root_symbol,
                    member_name,
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
        }

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

    fn project_prepared_member_route_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        let prepared = self.host.prepared_type_decl_in_view(
            scope_canonical_id,
            symbol_name,
            self.store_view,
        )?;
        let member = prepared.member(member_name)?;
        if type_expr_references_type_params(&member.ty, &prepared.type_parameters) {
            return None;
        }
        match &member.ty {
            TypeExpr::Object(_) => Some(member.ty.clone()),
            _ if prepared_member_body_stays_shallow(&member.ty) => Some(member.ty.clone()),
            _ if crate::meta_resolve::component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                &member.ty,
                scope_canonical_id,
                self,
            ) =>
            {
                Some(member.ty.clone())
            }
            _ => self.project_expr_surface_expr(scope_canonical_id, &member.ty),
        }
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

        let prepared = self.host.prepared_type_decl_in_view(
            scope_canonical_id,
            symbol_name,
            self.store_view,
        )?;
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

fn type_expr_references_type_params(
    expr: &TypeExpr,
    type_params: &[verter_semantic::analysis::type_expr::TypeParam],
) -> bool {
    use rustc_hash::FxHashSet;

    fn visit(expr: &TypeExpr, type_param_names: &FxHashSet<&str>) -> bool {
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
                type_param_names.contains(name.as_ref())
                    || type_arguments
                        .iter()
                        .any(|arg| visit(arg, type_param_names))
            }
            TypeExpr::TypeParameter(param) => {
                type_param_names.contains(param.name.as_str())
                    || param
                        .constraint
                        .as_deref()
                        .is_some_and(|constraint| visit(constraint, type_param_names))
                    || param
                        .default
                        .as_deref()
                        .is_some_and(|default| visit(default, type_param_names))
            }
            TypeExpr::Parenthesized(inner)
            | TypeExpr::Array { element: inner, .. }
            | TypeExpr::KeyOf(inner)
            | TypeExpr::Rest(inner) => visit(inner, type_param_names),
            TypeExpr::Tuple { elements, .. } => elements
                .iter()
                .any(|element| visit(&element.ty, type_param_names)),
            TypeExpr::Union(types)
            | TypeExpr::Intersection(types)
            | TypeExpr::TemplateLiteral {
                expressions: types, ..
            } => types.iter().any(|ty| visit(ty, type_param_names)),
            TypeExpr::Object(object) => object.properties.iter().any(|member| match member {
                verter_semantic::analysis::type_expr::ObjectMember::Property(property) => {
                    visit(&property.ty, type_param_names)
                }
                verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(signature) => {
                    visit(&signature.key_type, type_param_names)
                        || visit(&signature.value_type, type_param_names)
                }
                verter_semantic::analysis::type_expr::ObjectMember::CallSignature(function)
                | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(
                    function,
                ) => {
                    function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, type_param_names))
                        || function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, type_param_names))
                }
                verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .any(|parameter| visit(&parameter.ty, type_param_names))
                        || method
                            .function
                            .return_type
                            .as_deref()
                            .is_some_and(|return_type| visit(return_type, type_param_names))
                }
            }),
            TypeExpr::Function(function) => {
                function
                    .parameters
                    .iter()
                    .any(|parameter| visit(&parameter.ty, type_param_names))
                    || function
                        .return_type
                        .as_deref()
                        .is_some_and(|return_type| visit(return_type, type_param_names))
                    || function.type_parameters.iter().any(|parameter| {
                        parameter
                            .constraint
                            .as_deref()
                            .is_some_and(|constraint| visit(constraint, type_param_names))
                            || parameter
                                .default
                                .as_deref()
                                .is_some_and(|default| visit(default, type_param_names))
                    })
            }
            TypeExpr::IndexedAccess { object, index } => {
                visit(object, type_param_names) || visit(index, type_param_names)
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                visit(check, type_param_names)
                    || visit(extends, type_param_names)
                    || visit(true_type, type_param_names)
                    || visit(false_type, type_param_names)
            }
            TypeExpr::Mapped {
                source,
                value,
                name_type,
                ..
            } => {
                visit(source, type_param_names)
                    || visit(value, type_param_names)
                    || name_type
                        .as_deref()
                        .is_some_and(|name_type| visit(name_type, type_param_names))
            }
        }
    }

    let type_param_names: FxHashSet<&str> = type_params
        .iter()
        .map(|param| param.name.as_str())
        .collect();
    !type_param_names.is_empty() && visit(expr, &type_param_names)
}

#[cfg(test)]
mod tests {
    use super::type_expr_references_type_params;
    use super::ComponentMetaQueryEngine;
    use crate::resolver_core::solver_host::SessionSolverHost;
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;
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
}
