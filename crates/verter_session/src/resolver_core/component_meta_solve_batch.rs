//! Declaration-aware component-meta solve batch.
//!
//! Provides a query-local memo that caches solver results by
//! `(scope_canonical_id, symbol_name)` — the declaration-aware cache key.
//! This replaces the per-entry `solve_type` calls in registry append and
//! the isolated `SolveBatch` in macro expansion, so both phases share one
//! query-local memo over host-owned caches.

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::result::SolverResult;
use verter_semantic::analysis::type_solver::solve::SolveBatch;

use crate::resolver_core::solver_host::SessionSolverHost;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// Cache key for declaration-scoped solve results.
/// Uses (scope_canonical_id, symbol_name) to distinguish the same textual
/// `TypeExpr` resolved in different declaration file scopes.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct ScopedSolveKey {
    scope_canonical_id: String,
    symbol_name: String,
}

/// Cached result from a declaration-scoped solve.
#[derive(Debug, Clone)]
struct ScopedSolveEntry {
    result: SolverResult<TypeExpr>,
}

/// Query-local declaration-aware solve memo for component-meta.
///
/// Two cache layers:
/// 1. **Owner-scoped** (`SolveBatch`): keyed by `TypeExpr`, used for macro
///    expansion where all solves share one owner-scoped `SessionSolverHost`.
/// 2. **Declaration-scoped** (`scoped_cache`): keyed by
///    `(scope_canonical_id, symbol_name)`, used for registry entries from
///    imported files where each entry may have a different declaration scope.
///
/// Both layers sit on top of host-owned shared caches (prepared decls,
/// shallow file state, route facts). No per-request file caches are created.
pub struct ComponentMetaSolveBatch<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    /// Owner-scoped batch for Phase 1 macro expansion solves.
    owner_batch: SolveBatch<'a>,
    /// Declaration-scoped cache for Phase 2 registry solves.
    scoped_cache: FxHashMap<ScopedSolveKey, ScopedSolveEntry>,
}

impl<'a> ComponentMetaSolveBatch<'a> {
    /// Create a new batch backed by the given host and owner-scoped solver host.
    ///
    /// The `owner_solver_host` should be a `SessionSolverHost` scoped to the
    /// component's owner file. It is used for all Phase 1 macro expansion solves.
    pub fn new(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_solver_host: &'a dyn TypeSolverHost,
    ) -> Self {
        Self {
            host,
            store_view,
            owner_batch: SolveBatch::new(owner_solver_host),
            scoped_cache: FxHashMap::default(),
        }
    }

    /// Create a new batch from an existing owner-scoped `SolveBatch`.
    /// Used when Phase 1 has already created a batch and Phase 2 should
    /// reuse its cached state.
    pub fn from_owner_batch(
        host: &'a VerterHost,
        store_view: Option<&'a HostStoreView>,
        owner_batch: SolveBatch<'a>,
    ) -> Self {
        Self {
            host,
            store_view,
            owner_batch,
            scoped_cache: FxHashMap::default(),
        }
    }

    /// Solve a named type in the owner's declaration scope.
    /// Used as an owner-fallback when scoped resolution fails.
    pub fn solve_owner_named(&mut self, requested_name: &str) -> Option<TypeExpr> {
        let expr = TypeExpr::named(requested_name);
        let result = self.owner_batch.solve(&expr);
        filter_identity_ref(&result, requested_name)
    }

    /// Solve a registry declaration in its defining file's scope.
    /// Used by registry append (Phase 2).
    ///
    /// Cache key is `(scope_canonical_id, symbol_name)`. If the same
    /// declaration was already solved earlier in this query (whether by
    /// Phase 1 through the owner batch or by a prior Phase 2 entry),
    /// the cached result is returned.
    pub fn solve_scoped(
        &mut self,
        scope_canonical_id: &str,
        requested_name: &str,
    ) -> Option<TypeExpr> {
        let key = ScopedSolveKey {
            scope_canonical_id: scope_canonical_id.to_string(),
            symbol_name: requested_name.to_string(),
        };

        // Check scoped cache first
        if let Some(entry) = self.scoped_cache.get(&key) {
            return filter_identity_ref(&entry.result, requested_name);
        }

        // Fast path: package source — return prepared body as-is, no solver needed.
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

        // Fast path: prepared decl with direct surface and no deps that need solving.
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

        // Shallow state must be available for the solver path. If not, return
        // None without caching (shallow state may become available later in the
        // same query as dependencies are materialized).
        self.host
            .shallow_file_state_in_view(scope_canonical_id, self.store_view)?;

        // Full solver path
        let solver_host = SessionSolverHost::with_declaration_scope(
            self.host,
            self.store_view,
            scope_canonical_id,
        );
        let type_ref = TypeExpr::named(requested_name);
        let (result, _trace) = verter_semantic::analysis::type_solver::solve::solve_type_with_trace(
            &type_ref,
            &solver_host,
        );

        let filtered = filter_identity_ref(&result, requested_name);
        let entry = ScopedSolveEntry { result };
        self.scoped_cache.insert(key, entry);
        filtered
    }

    /// Number of entries in the scoped cache. For diagnostics/benchmarking.
    pub fn scoped_cache_len(&self) -> usize {
        self.scoped_cache.len()
    }
}

/// Check if a solve result is just `Ref(requested_name)` unchanged —
/// meaning the solver couldn't resolve it. Returns None in that case.
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
