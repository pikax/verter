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

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine;
use verter_semantic::analysis::type_solver::result::SolverResult;

use super::declaration_metadata::ResolvedTypeDeclaration;
use super::route_demand::RouteDemand;
use crate::resolver_core::solver_host::SessionSolverHost;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Cached import route: (resolved_canonical_id, resolved_exported_name, prepared alias).
type PreparedAliasEntry = Option<(String, String, super::CachedPreparedImportedTypeAlias)>;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ScopedSolveKey {
    scope_canonical_id: String,
    symbol_name: String,
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
    /// Cached import-route resolutions.
    routes: FxHashMap<(String, String, RouteDemand), PreparedAliasEntry>,
    /// Cached type declarations.
    declarations: FxHashMap<(String, String), ResolvedTypeDeclaration>,
    /// Cached resolvability checks.
    resolvable: FxHashMap<(String, String), bool>,
    /// Cached owner collection expressions.
    owner_collection_exprs:
        FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>,
}

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
            routes: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
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
            routes: FxHashMap::default(),
            declarations: FxHashMap::default(),
            resolvable: FxHashMap::default(),
            owner_collection_exprs: FxHashMap::default(),
        }
    }

    pub fn owner_engine_mut(&mut self) -> &mut TypeQueryEngine<'a> {
        &mut self.owner_engine
    }

    pub fn into_owner_engine(self) -> TypeQueryEngine<'a> {
        self.owner_engine
    }

    pub fn solve_owner_named(&mut self, requested_name: &str) -> Option<TypeExpr> {
        let expr = TypeExpr::named(requested_name);
        let result = self.owner_engine.solve(&expr);
        filter_identity_ref(&result, requested_name)
    }

    /// Pre-seed import-route resolutions for the initial registry entries from
    /// imported sources.
    pub fn pre_seed_routes(
        &mut self,
        registry_meta: &[super::component_meta::ResolvedTypeRegistryMeta],
        owner_canonical: &str,
    ) {
        for meta in registry_meta {
            let source = meta.declaration.canonical_source.as_str();
            if source.is_empty() || source == owner_canonical {
                continue;
            }
            let name = if meta.declaration.resolved_name.is_empty() {
                meta.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let _ = self.resolve_prepared_alias(source, name, &RouteDemand::Whole);
        }
    }

    /// Resolve a prepared import alias, cached per query.
    pub fn resolve_prepared_alias(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
        route: &RouteDemand,
    ) -> PreparedAliasEntry {
        let key = (
            canonical_id.to_string(),
            exported_name.to_string(),
            route.clone(),
        );
        self.routes
            .entry(key)
            .or_insert_with_key(|_| {
                self.host
                    .resolve_prepared_symbol_dependency_alias_for_route_in_view(
                        canonical_id,
                        exported_name,
                        route,
                        self.store_view,
                    )
            })
            .clone()
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
    pub fn can_resolve(
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
        *self.resolvable.entry(key).or_insert_with_key(|_| {
            can_resolve_ref(
                self.host,
                owner_canonical,
                exported_name,
                source_hint,
                self.store_view,
            )
        })
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

    pub fn solve_scoped(
        &mut self,
        scope_canonical_id: &str,
        requested_name: &str,
    ) -> Option<TypeExpr> {
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

        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        let type_ref = TypeExpr::named(requested_name);
        let (result, _trace) =
            self.owner_engine
                .solve_scoped(&solver_host, scope_canonical_id, &type_ref);
        // Steps and solve_count are tracked by the shared owner_engine.
        let filtered = filter_identity_ref(&result, requested_name);
        self.scoped_cache.insert(key, ScopedSolveEntry { result });
        filtered
    }

    /// Solve an arbitrary `TypeExpr` in a declaration scope, sharing the
    /// request-scoped engine. Used by the materialization tree walker so
    /// that repeated solves within one component-meta request benefit from
    /// the shared projection and instantiation caches.
    pub fn solve_expr_in_scope(&mut self, scope_canonical_id: &str, expr: &TypeExpr) -> TypeExpr {
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        let (result, _trace) =
            self.owner_engine
                .solve_scoped(&solver_host, scope_canonical_id, expr);
        result.value
    }

    pub fn scoped_cache_len(&self) -> usize {
        self.scoped_cache.len()
    }

    pub fn routes_count(&self) -> usize {
        self.routes.len()
    }

    pub fn total_steps(&self) -> u64 {
        self.owner_engine.total_steps()
    }

    pub fn solve_count(&self) -> u32 {
        self.owner_engine.solve_count()
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
    pub fn project_type_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedSurface> {
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        self.owner_engine
            .project_surface(subject_id, &solver_host, scope_canonical_id)
    }

    pub fn project_type_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        self.owner_engine.project_expr_surface_as_type_expr(
            &solver_host,
            scope_canonical_id,
            &TypeExpr::named(symbol_name),
        )
    }

    /// Project a single member from a type expression in a declaration scope.
    pub fn project_type_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedMember> {
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        self.owner_engine
            .project_member(subject_id, member_name, &solver_host, scope_canonical_id)
    }

    /// Project the keyspace (member names) from a type expression in a
    /// declaration scope.
    pub fn project_type_keyspace(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_semantic::analysis::type_solver::query_engine::ProjectedKeyspace> {
        use verter_semantic::analysis::type_solver::query_engine::SubjectKey;

        let subject_key = SubjectKey::Decl {
            canonical_id: scope_canonical_id.to_string(),
            symbol_name: symbol_name.to_string(),
            args_hash: 0,
            conditional_ctx_hash: 0,
        };
        let subject_id = self.owner_engine.intern_subject(subject_key);
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        self.owner_engine
            .project_keyspace(subject_id, &solver_host, scope_canonical_id)
    }

    pub fn project_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> Option<TypeExpr> {
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        self.owner_engine
            .project_expr_surface_as_type_expr(&solver_host, scope_canonical_id, expr)
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

fn can_resolve_ref(
    host: &VerterHost,
    owner_canonical: &str,
    exported_name: &str,
    source_hint: Option<&str>,
    store_view: Option<&HostStoreView>,
) -> bool {
    let source = source_hint
        .filter(|source| !source.is_empty())
        .unwrap_or(owner_canonical);

    if host
        .prepared_type_decl_in_view(source, exported_name, store_view)
        .is_some()
    {
        return true;
    }

    if let Some((resolved_id, resolved_name, _)) = host
        .resolve_prepared_symbol_dependency_alias_for_route_in_view(
            source,
            exported_name,
            &crate::resolver_core::RouteDemand::Whole,
            store_view,
        )
    {
        if (resolved_id != source || resolved_name != exported_name)
            && host
                .prepared_type_decl_in_view(&resolved_id, &resolved_name, store_view)
                .is_some()
        {
            return true;
        }
    }

    false
}
