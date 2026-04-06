//! Declaration-aware component-meta query engine.
//!
//! Provides query-local memoization for component-meta solves by
//! `(scope_canonical_id, symbol_name)`, while delegating actual solving to the
//! request-scoped `TypeQueryEngine`.

use rustc_hash::FxHashMap;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::type_solver::host::TypeSolverHost;
use verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine;
use verter_semantic::analysis::type_solver::result::SolverResult;

use crate::resolver_core::solver_host::SessionSolverHost;
use crate::resolver_store::HostStoreView;
use crate::VerterHost;

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
/// The owner engine is shared across macro expansion and later registry
/// materialization. Imported registry entries additionally memoize by
/// declaration scope so the same textual reference does not alias across files.
pub struct ComponentMetaQueryEngine<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a HostStoreView>,
    owner_engine: TypeQueryEngine<'a>,
    scoped_cache: FxHashMap<ScopedSolveKey, ScopedSolveEntry>,
    scoped_total_steps: u64,
    scoped_solve_count: u32,
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
            scoped_total_steps: 0,
            scoped_solve_count: 0,
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
            scoped_total_steps: 0,
            scoped_solve_count: 0,
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
        let mut scoped_engine = TypeQueryEngine::new(&solver_host);
        let (result, _trace) = scoped_engine.solve_with_trace(&type_ref);
        self.scoped_total_steps += result.steps;
        self.scoped_solve_count += 1;
        let filtered = filter_identity_ref(&result, requested_name);
        self.scoped_cache.insert(key, ScopedSolveEntry { result });
        filtered
    }

    pub fn scoped_cache_len(&self) -> usize {
        self.scoped_cache.len()
    }

    pub fn total_steps(&self) -> u64 {
        self.owner_engine.total_steps() + self.scoped_total_steps
    }

    pub fn solve_count(&self) -> u32 {
        self.owner_engine.solve_count() + self.scoped_solve_count
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
