//! Imported registry symbol resolution, direct prepared declaration
//! access, fuse/state/debug accessors, and ctx/dispatch entry helpers
//! for `ComponentMetaQueryEngine<'a>`.
//!
//! Inherent methods defined in a sibling `impl<'a>` block; they read
//! the engine's private read-through caches and dispatch to the ctx
//! store, then return resolved declarations or imported registry
//! symbols.
//!
//! Visibility:
//! - `pub fn resolve_imported_registry_symbol`, `pub fn
//!   resolve_direct_prepared_type_declaration`, `pub fn
//!   resolve_direct_prepared_type_declaration_metadata`, `pub fn
//!   resolve_type_declaration`, `pub fn resolve_final_prepared_type_target`,
//!   `pub fn can_resolve_registry_symbol`, `pub fn owner_collection_expr`,
//!   `pub fn named_decl_body`, `pub fn prepared_member_raw_type`,
//!   `pub fn enter_member_surface`, `pub fn exit_member_surface`,
//!   `pub fn allow_structural_slow_lane`, `pub fn allow_wildcard_route`,
//!   `pub fn allow_imported_root`, `pub fn allow_registry_deepening`,
//!   `pub fn allow_union_member`, `pub fn reset_union_members`,
//!   `pub fn has_fuse_tripped`, `pub fn fuse_trips` — all `pub` on the
//!   engine, callable from outside the crate.
//! - `pub(crate) fn materialize_member_surface_expr`,
//!   `pub(crate) fn projection_op_budget_exhausted`,
//!   `pub(crate) fn imported_registry_symbol_cache_len`,
//!   `pub(crate) fn materialized_member_surface_cache_len`,
//!   `pub(crate) fn debug_*`, `pub(crate) fn prepared_type_decl`,
//!   `pub(crate) fn ctx`,
//!   `pub(crate) fn dispatch_projected_surface`,
//!   `pub(crate) fn dispatch_projected_member`,
//!   `pub(crate) fn dispatch_projected_keyspace`,
//!   `pub(crate) fn dispatch_routed_expr_surface_expr` — crate-visible
//!   helpers used by `meta_resolve` and other engine impl methods.
//! - Private methods (`semantic_dispatch`, `dispatch_root_instantiated`)
//!   stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.

use verter_semantic::analysis::type_solver::query_engine::{
    ProjectedKeyspace, ProjectedMember, ProjectedSurface,
};
use verter_type_expr::TypeExpr;

use super::helpers::{is_builtin_name, resolve_imported_registry_symbol_with_budget};
use super::surface::{
    dispatch_route_expr_is_materialized, filtered_projected_surface,
    projected_surface_from_semantic_node, projected_surface_to_type_expr,
};
use super::{
    empty_semantic_args, engine_fact_signature_for_exported_type,
    local_type_symbol_metadata_for_known_source, ComponentMetaQueryEngine,
    DirectPreparedDeclarationResolver, ResolvedImportedRegistrySymbol, ResolvedTypeDeclaration,
};
use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
use crate::resolver_core::{FuseTrip, RouteDemand};
use crate::semantic_query::{
    PathSegment, ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
};

impl<'a> ComponentMetaQueryEngine<'a> {
    pub fn resolve_imported_registry_symbol(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<ResolvedImportedRegistrySymbol> {
        let key = (canonical_id.to_string(), exported_name.to_string());
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("imported_registry_symbols");
        if let Some(cached) = self.imported_registry_symbols.borrow().get(&key).cloned() {
            return cached;
        }
        // Step 3 closure: route through ctx-owned ImportedRegistryDb.
        // The local RefCell view above is non-authoritative scratch; the
        // DashMap-backed DB is the authoritative cross-request cache.
        let arc_key = (
            std::sync::Arc::<str>::from(canonical_id),
            std::sync::Arc::<str>::from(exported_name),
        );
        let host_db = self.ctx.project_type_store().imported_registry_db();
        let host_value = host_db.get_or_compute(&arc_key, self.ctx, || {
            let computed = resolve_imported_registry_symbol_with_budget(
                self.ctx,
                canonical_id,
                exported_name,
                || self.allow_wildcard_route(),
            );
            let fact_sig =
                engine_fact_signature_for_exported_type(self.ctx, canonical_id, exported_name);
            Some((computed, fact_sig))
        });
        let resolved: Option<ResolvedImportedRegistrySymbol> = match host_value {
            Some(opt_arc) => opt_arc.as_deref().cloned(),
            None => None,
        };
        self.imported_registry_symbols
            .borrow_mut()
            .insert(key, resolved.clone());
        resolved
    }

    pub fn resolve_direct_prepared_type_declaration(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<ResolvedTypeDeclaration> {
        self.prepared_type_decl(canonical_source, resolved_name)?;
        let metadata =
            local_type_symbol_metadata_for_known_source(self.ctx, canonical_source, resolved_name)?;
        let resolver = DirectPreparedDeclarationResolver { ctx: self.ctx };
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
        self.prepared_type_decl(canonical_source, resolved_name)?;
        let metadata =
            local_type_symbol_metadata_for_known_source(self.ctx, canonical_source, resolved_name)?;
        Some(ResolvedTypeDeclaration {
            requested_name: resolved_name.to_string(),
            declaration_id: self
                .ctx
                .local_type_declaration_id(canonical_source, resolved_name),
            resolved_name: resolved_name.to_string(),
            canonical_source: canonical_source.to_string(),
            span: metadata.span,
            kind: metadata.kind,
            text: None,
        })
    }

    /// Graph-native member-surface materialiser. Lowers `expr` to a
    /// `SemanticNodeId` via Navigate, runs the materialiser,
    /// accumulates the dep_signature into the per-request thread-local
    /// accumulator, and raises the materialised node back to TypeExpr.
    pub(crate) fn materialize_member_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        nested_surface: bool,
    ) -> verter_type_expr::TypeExpr {
        use crate::component_meta_materialize::{
            materialize_component_meta_structure, MaterializationScope, MaterializeOutcome,
            MaterializeStructureCacheKey,
        };
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::semantic_query::ProjectionMode;

        let dispatch = ProjectSemanticDispatch::new(self.ctx);
        let Some(base) = dispatch.lower_type_expr_in_scope_with_mode(
            scope_canonical_id,
            expr,
            ProjectionMode::Navigate,
        ) else {
            return expr.clone();
        };
        let key = MaterializeStructureCacheKey {
            scope_canonical_id: std::sync::Arc::from(scope_canonical_id),
            base,
            scope_axis: if nested_surface {
                MaterializationScope::Nested
            } else {
                MaterializationScope::TopLevel
            },
            mode: ProjectionMode::Expanded,
        };
        let read = materialize_component_meta_structure(self.ctx, key);
        crate::fact_signature_helpers::observe_fact_signature(
            &crate::fact_signature_helpers::dep_signature_to_fact_signature(&read.dep_signature),
        );
        let materialised_id = match read.value {
            MaterializeOutcome::Value(id)
            | MaterializeOutcome::Miss(id)
            | MaterializeOutcome::Recursive(id)
            | MaterializeOutcome::Tainted(id) => id,
            MaterializeOutcome::Error(_) => return expr.clone(),
        };
        dispatch
            .raise_node_to_type_expr(materialised_id)
            .unwrap_or_else(|| expr.clone())
    }

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("declarations");
        if let Some(cached) = self.declarations.borrow().get(&key).cloned() {
            return cached;
        }
        // Step 3 closure: route through ctx-owned DeclarationLookupDb.
        let arc_key = (
            std::sync::Arc::<str>::from(canonical_source),
            std::sync::Arc::<str>::from(requested_name),
        );
        let host_db = self.ctx.project_type_store().declaration_db();
        let host_value = host_db.get_or_compute(&arc_key, self.ctx, || {
            let computed = self
                .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
                .unwrap_or_else(|| {
                    self.ctx
                        .resolve_type_declaration_for_dep(canonical_source, requested_name)
                });
            let fact_sig =
                engine_fact_signature_for_exported_type(self.ctx, canonical_source, requested_name);
            Some((computed, fact_sig))
        });
        let declaration = match host_value {
            Some(arc_decl) => arc_decl.as_ref().clone(),
            None => self
                .resolve_direct_prepared_type_declaration(canonical_source, requested_name)
                .unwrap_or_else(|| {
                    self.ctx
                        .resolve_type_declaration_for_dep(canonical_source, requested_name)
                }),
        };
        self.declarations
            .borrow_mut()
            .insert(key, declaration.clone());
        declaration
    }

    pub fn resolve_final_prepared_type_target(
        &mut self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> (String, String) {
        if self
            .prepared_type_decl(canonical_source, resolved_name)
            .is_some()
        {
            return (canonical_source.to_string(), resolved_name.to_string());
        }

        self.ctx
            .resolve_named_type_export_target_shallow(canonical_source, resolved_name)
            .filter(|(target_canonical, target_name)| {
                self.prepared_type_decl(target_canonical.as_str(), target_name.as_str())
                    .is_some()
            })
            .unwrap_or_else(|| (canonical_source.to_string(), resolved_name.to_string()))
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
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("resolvable");
        if let Some(cached) = self.resolvable.borrow().get(&key).copied() {
            return cached;
        }
        // Step 3 closure: route through ctx-owned ResolvabilityDb.
        let arc_key = (
            std::sync::Arc::<str>::from(source_key),
            std::sync::Arc::<str>::from(exported_name),
        );
        let host_db = self.ctx.project_type_store().resolvable_db();
        let host_value = host_db.get_or_compute(&arc_key, self.ctx, || {
            let computed = if self.prepared_type_decl(source_key, exported_name).is_some() {
                true
            } else {
                self.resolve_imported_registry_symbol(source_key, exported_name)
                    .is_some()
            };
            let fact_sig =
                engine_fact_signature_for_exported_type(self.ctx, source_key, exported_name);
            Some((computed, fact_sig))
        });
        let resolved = host_value.unwrap_or(false);
        self.resolvable.borrow_mut().insert(key, resolved);
        resolved
    }

    /// Get the owner's collection expression for a name, cached per query.
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_type_expr::TypeExpr> {
        #[cfg(test)]
        crate::spike_instrumentation::record_cache_read("owner_collection_exprs");
        if let Some(cached) = self.owner_collection_exprs.borrow().get(name).cloned() {
            return cached;
        }

        // Step 3 closure: route through ctx-owned OwnerCollectionDb.
        let arc_key = (
            std::sync::Arc::<str>::from(owner_canonical),
            std::sync::Arc::<str>::from(name),
        );
        let host_db = self.ctx.project_type_store().owner_collection_db();
        let host_value = host_db.get_or_compute(&arc_key, self.ctx, || {
            let computed = self
                .prepared_type_decl(owner_canonical, name)
                .map(|prepared| prepared.body.clone());
            let fact_sig = engine_fact_signature_for_exported_type(self.ctx, owner_canonical, name);
            Some((computed, fact_sig))
        });
        let body: Option<verter_type_expr::TypeExpr> = match host_value {
            Some(opt_arc) => opt_arc.map(|arc_expr| arc_expr.as_ref().clone()),
            None => self
                .prepared_type_decl(owner_canonical, name)
                .map(|prepared| prepared.body.clone()),
        };
        self.owner_collection_exprs
            .borrow_mut()
            .insert(name.to_string(), body.clone());
        body
    }

    pub fn named_decl_body(&mut self, canonical_id: &str, name: &str) -> Option<TypeExpr> {
        self.prepared_type_decl(canonical_id, name)
            .map(|prepared| prepared.body.clone())
    }

    pub fn prepared_member_raw_type(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<TypeExpr> {
        self.prepared_type_decl(canonical_id, symbol_name)
            .and_then(|prepared| prepared.member(member_name).map(|member| member.ty.clone()))
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

    /// `pub(crate)` accessor for the projection-op fuse
    /// budget check. Used by the bridge helpers in `meta_resolve.rs`
    /// (post engine-method deletion) to gate the same-budget check the
    /// retired engine methods enforced.
    pub(crate) fn projection_op_budget_exhausted(&mut self) -> bool {
        self.fuse_state
            .check_projection_op_count(&self.fuse_budgets)
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
    pub fn fuse_trips(&self) -> &[FuseTrip] {
        &self.fuse_state.trips
    }

    #[cfg(test)]
    pub(crate) fn imported_registry_symbol_cache_len(&self) -> usize {
        self.imported_registry_symbols.borrow().len()
    }

    /// Cache size for the structural materialiser's final-result
    /// cache (ctx-owned `MaterializeStructureDb::live_count()`).
    #[cfg(test)]
    pub(crate) fn materialized_member_surface_cache_len(&self) -> usize {
        self.ctx
            .project_type_store()
            .materialize_structure_db()
            .live_count()
    }

    /// The corresponding test assertions migrated to behavior
    /// assertions / ctx `prepared_surface_db().live_count()` checks.
    /// Field + accessor retained until the broader counter cleanup.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_type_decl_query_count(&self) -> usize {
        self.prepared_type_decl_query_count
    }

    /// The corresponding test assertion migrated to a behavior
    /// assertion on the projected `define_props` shape. Field +
    /// accessor retained.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_root_surface_projection_count(&self) -> usize {
        self.prepared_root_surface_projection_count
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_shared_surface_hit_count(&self) -> usize {
        self.prepared_shared_surface_hit_count
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_shared_member_hit_count(&self) -> usize {
        self.prepared_shared_member_hit_count
    }

    pub(crate) fn prepared_type_decl(
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

        let resolved = self
            .ctx
            .prepared_type_decl(canonical_id, symbol_name)
            .or_else(|| {
                // Lazy first-time loading (see scope_payload_for_scope comment).
                self.ctx
                    .ensure_loaded(canonical_id)
                    .then(|| self.ctx.prepared_type_decl(canonical_id, symbol_name))
                    .flatten()
            });
        self.prepared_type_decls.insert(key, resolved.clone());
        resolved
    }

    /// Single accessor returning the engine's resolver
    /// context. Replaces the legacy `ctx()` accessor (which returned
    /// `&VerterHost`) now that the engine field is `&dyn ResolverContext`.
    /// Out-of-seal-scope callers (`host_manage/*`) accept the trait
    /// object because every method they reach (project_type_store,
    /// prepared_decl_bundle, dispatch, etc.) is on the trait surface.
    pub(crate) fn ctx(&self) -> &dyn crate::resolver_core::ResolverContext {
        self.ctx
    }

    fn semantic_dispatch(&self) -> ProjectSemanticDispatch<'_> {
        ProjectSemanticDispatch::new(self.ctx)
    }

    fn dispatch_root_instantiated(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<SemanticNodeId> {
        // Resolve the root identity via
        // `bare_name_resolve::resolve_bare_name_in_scope` directly —
        // no `SessionSolverHost` construction. Matches the dispatch
        // lowering path in `shallow_lower_type_expr`.
        let scope_payload_arc = self.scope_payload_for_scope(scope_canonical_id);
        let resolved_root = crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
            self.ctx,
            scope_canonical_id,
            scope_payload_arc.as_deref(),
            symbol_name,
        )
        .map(|root| (root.canonical_id, root.symbol_name))
        .unwrap_or_else(|| (scope_canonical_id.to_string(), symbol_name.to_string()));
        let dispatch = self.semantic_dispatch();
        let anchor = match dispatch.execute(SemanticQueryKey::ResolveDecl(resolve_decl_key(
            resolved_root.0.as_str(),
            resolved_root.1.as_str(),
        ))) {
            QueryResult::Value(id) => id,
            _ => return None,
        };
        // C16: Instantiate.base is DeclIdentity. Build from resolved root +
        // shallow state whole_hash.
        let whole_hash = self
            .ctx
            .shallow_file_state(resolved_root.0.as_str())
            .map(|s| s.whole_hash)
            .unwrap_or_default();
        let identity = crate::semantic_query::DeclIdentity {
            canonical_id: std::sync::Arc::from(resolved_root.0.as_str()),
            whole_hash,
            decl_name: std::sync::Arc::from(resolved_root.1.as_str()),
        };
        match dispatch.execute(SemanticQueryKey::Instantiate {
            base: identity,
            args: empty_semantic_args(),
            // `dispatch_root_instantiated` feeds
            // `projected_surface_from_semantic_node` which reads the
            // root's surface members, call/construct lists, etc. Expanded
            // is required so the surface is interpretable; Navigate
            // would yield the lazy shell with no readable view.
            body_mode: crate::semantic_query::ProjectionMode::Expanded,
        }) {
            QueryResult::Value(id) => Some(id),
            _ => Some(anchor),
        }
    }

    pub(crate) fn dispatch_projected_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        let root = self.dispatch_root_instantiated(scope_canonical_id, symbol_name)?;
        projected_surface_from_semantic_node(self.ctx, root)
    }

    pub(crate) fn dispatch_projected_member(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
        member_name: &str,
    ) -> Option<ProjectedMember> {
        self.dispatch_projected_surface(scope_canonical_id, symbol_name)?
            .members
            .into_iter()
            .find(|member| member.name == member_name)
    }

    /// Clippy cleanup — paired with `dispatch_projected_member`
    /// and `dispatch_projected_surface` as part of the ComponentMetaQueryEngine
    /// surface contract. No call site in the landed tree, but the helper is
    /// retained for symmetry with the projection/keyspace surface API; the
    /// dispatch path uses keyspace shape directly via `surface.members`
    /// elsewhere. `#[allow(dead_code)]` keeps the API symmetry without
    /// triggering the unused-method lint.
    #[allow(dead_code)]
    pub(crate) fn dispatch_projected_keyspace(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedKeyspace> {
        let surface = self.dispatch_projected_surface(scope_canonical_id, symbol_name)?;
        let mut members = surface
            .members
            .iter()
            .map(|member| member.name.clone())
            .collect::<Vec<_>>();
        members.sort();
        members.dedup();
        Some(ProjectedKeyspace {
            members,
            has_index_signature: surface.has_index_signature,
        })
    }

    pub(crate) fn dispatch_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<TypeExpr> {
        match route {
            RouteDemand::Whole => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| projected_surface_to_type_expr(&surface))
                .filter(dispatch_route_expr_is_materialized),
            RouteDemand::MemberPath(path) if !path.is_empty() => {
                let root = self.dispatch_root_instantiated(scope_canonical_id, root_symbol)?;
                let query_path: std::sync::Arc<[PathSegment]> = std::sync::Arc::from(
                    path.iter()
                        .map(|segment| PathSegment::Member(std::sync::Arc::from(segment.as_str())))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let dispatch = self.semantic_dispatch();
                match dispatch.execute(SemanticQueryKey::ProjectPath {
                    base: root,
                    path: query_path,
                    mode: ProjectionMode::Expanded,
                }) {
                    QueryResult::Value(node) => dispatch
                        .raise_node_to_type_expr(node)
                        .filter(dispatch_route_expr_is_materialized),
                    _ => None,
                }
            }
            RouteDemand::Pick(members) if !members.is_empty() => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| {
                    projected_surface_to_type_expr(&filtered_projected_surface(surface, |name| {
                        members.iter().any(|member| member == name)
                    }))
                })
                .filter(dispatch_route_expr_is_materialized),
            RouteDemand::Omit(members) if !members.is_empty() => self
                .dispatch_projected_surface(scope_canonical_id, root_symbol)
                .and_then(|surface| {
                    projected_surface_to_type_expr(&filtered_projected_surface(surface, |name| {
                        !members.iter().any(|member| member == name)
                    }))
                })
                .filter(dispatch_route_expr_is_materialized),
            _ => None,
        }
    }
}
