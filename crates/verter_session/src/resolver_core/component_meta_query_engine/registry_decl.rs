//! Direct prepared-declaration access, the member-surface node-core and its
//! demand APIs, prepared-target resolution, and the ctx/dispatch entry helpers
//! for `ComponentMetaQueryEngine<'a>`.
//!
//! Inherent methods defined in a sibling `impl<'a>` block; they read
//! the engine's private read-through caches and dispatch to the ctx
//! store, then return resolved declarations or materialised surfaces.
//!
//! The engine's four SHARED-CACHE producers (`ImportedRegistryDb`,
//! `DeclarationLookupDb`, `ResolvabilityDb`, `OwnerCollectionDb`) live in the
//! sibling `registry_cache_producers` module: they share one admission
//! discipline (a cacheability tracer scope bracketing the whole cold path) and
//! are read together. This module keeps the reads that admit into NO shared
//! cache, plus the output-capability-minting node-core.
//!
//! Visibility:
//! - `pub fn resolve_direct_prepared_type_declaration`, `pub fn
//!   resolve_direct_prepared_type_declaration_metadata`, `pub fn
//!   resolve_final_prepared_type_target`, `pub fn named_decl_body` — all `pub`
//!   on the engine, callable from outside the crate.
//! - `pub(crate) fn materialize_member_surface_expr`,
//!   `pub(crate) fn prepared_type_decl`, `pub(crate) fn ctx`,
//!   `pub(crate) fn dispatch_routed_expr_surface_node` — crate-visible
//!   helpers used by `meta_resolve` and other engine impl methods.
//! - `pub(super) fn prepared_decl_authored_body_locator` — the ONE locator
//!   mint, shared with the owner-collection producer next door.
//! - Private methods (`semantic_dispatch`, `dispatch_root_instantiated`)
//!   stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.
//!
//! The engine's fuse / fanout-budget / cache-length / debug accessors live in
//! the sibling `engine_accessors` module.

use super::route_admission::{self, AdmittedRouteProjectionNode};
use super::surface::{compound_root_surface_view_via_dispatch, surface_view_from_semantic_node};
use super::{
    empty_semantic_args, local_type_symbol_metadata_for_known_source, ComponentMetaQueryEngine,
    DirectPreparedDeclarationResolver, ResolvedTypeDeclaration,
};
use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::raise::node_raised_shape_facts_with_dispatch;
use crate::project_semantic_dispatch::{resolve_decl_key, ProjectSemanticDispatch};
use crate::resolver_core::RouteDemand;
use crate::semantic_query::{
    PathSegment, ProjectionMode, QueryResult, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The component-meta query-engine REGISTRY-DECL materialiser's output-sink
    /// capability. The registry-decl materialiser here holds this to
    /// materialize a graph node into a sealed output carrier and unwrap it.
    /// Its constructor is visible ONLY within
    /// `crate::resolver_core::component_meta_query_engine::registry_decl` — NOT
    /// the whole query-engine subtree — so no query-engine sibling can mint it
    /// (planted `MetaQueryRegistryOutputCap::new` outside this leaf is
    /// `E0624`).
    pub(crate) struct MetaQueryRegistryOutputCap;
    mint: pub(in crate::resolver_core::component_meta_query_engine::registry_decl)
}

impl<'a> ComponentMetaQueryEngine<'a> {
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
    /// `SemanticNodeId` via Navigate, then delegates to the shared
    /// node-core [`Self::materialize_member_surface_node`].
    ///
    /// This is the `TypeExpr`-input arm of the member-surface seam: it
    /// lowers ONCE through the single dispatch, then routes the lowered
    /// node into the same core the handle-input arm uses. A consumer
    /// that already holds a settled graph node (a [`HotTypeRef`]) skips
    /// the lowering and calls `materialize_member_surface_node`
    /// directly — both arms reduce the SAME node through the SAME
    /// dispatch (read-compat, one resolver), never a reverse
    /// materialize-then-re-lower bridge.
    ///
    /// [`HotTypeRef`]: crate::semantic_query::HotTypeRef
    /// Demand-based member-surface API for a `Pick<Root, members…>` route.
    ///
    /// The OUT-OF-SUBTREE entry point for the routed-Pick member surface
    /// (`host_manage::component_meta_methods`): the caller passes the
    /// pre-resolution demand it already holds — a scope, the route ROOT
    /// symbol, and the picked member keys — and this method resolves the
    /// `Pick` node through the shared dispatch and materialises it via the
    /// private [`Self::materialize_member_surface_node_core`] INTERNALLY.
    /// No `SemanticNodeId` crosses the boundary: the forgeable node never
    /// leaves the query-engine sink.
    ///
    /// The Pick resolution is the same single-dispatch path the
    /// materialiser's Pick/Omit arm uses: lower the bare-`Ref` root at
    /// `Navigate` (an intermediate hop), then `execute_pick` on the picked
    /// keys at `Expanded` (the terminal demand). `None` on a recursive /
    /// error Pick OR a node-core materialisation error — the caller then
    /// falls through to its registry-candidate path.
    ///
    /// The fact-bearing registry path now publishes through
    /// [`Self::materialize_pick_member_surface_candidate`] (which also carries the
    /// object-surface fact); this bare-`TypeExpr` demand form is retained as the
    /// boundary-contract surface the `dispatch_helpers` demand-API contract test
    /// pins (`(scope, symbol, members, nested) -> Option<TypeExpr>`, no node leaks).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn materialize_pick_member_surface(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
        nested_surface: bool,
    ) -> Option<verter_type_expr::TypeExpr> {
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::semantic_query::{ProjectionMode, QueryResult};

        let pick_node = {
            let dispatch = ProjectSemanticDispatch::new(self.ctx);
            let symbol_ref = verter_type_expr::TypeExpr::Ref {
                name: std::sync::Arc::from(root_symbol),
                type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            };
            // Bare-Ref base for the Pick builtin is an intermediate hop;
            // the Pick result is the terminal demand.
            let base = dispatch.lower_type_expr_in_scope_with_mode(
                scope_canonical_id,
                &symbol_ref,
                ProjectionMode::Navigate,
            )?;
            let members_arc: Vec<std::sync::Arc<str>> = members
                .iter()
                .map(|s| std::sync::Arc::from(s.as_str()))
                .collect();
            match dispatch.execute_pick(base, &members_arc, ProjectionMode::Expanded) {
                QueryResult::Value(id) => id,
                QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
            }
        };
        self.materialize_member_surface_node_core(scope_canonical_id, pick_node, nested_surface)
    }

    /// PRIVATE node-core of the member-surface seam: materialise an
    /// ALREADY-LOWERED graph node (`base`) through the single dispatch,
    /// returning the same public surface the `TypeExpr` arm
    /// ([`Self::materialize_member_surface_expr`]) produces for the
    /// node it lowers `expr` to.
    ///
    /// This is the shared node-core the in-subtree arms route through. It
    /// NEVER materialises `base` back to a `TypeExpr` to re-lower it — it
    /// reduces the node directly. `None` signals a materialisation error
    /// (the `TypeExpr` arm falls back to its input clone).
    ///
    /// MODULE-PRIVATE (a `SemanticNodeId` is forgeable in safe Rust, so
    /// the node-input core is never exposed beyond the query-engine
    /// subtree). Out-of-subtree callers reach the surface through a demand
    /// API that resolves the node INTERNALLY — see
    /// [`Self::materialize_pick_member_surface`].
    fn materialize_member_surface_node_core(
        &mut self,
        scope_canonical_id: &str,
        base: crate::semantic_query::SemanticNodeId,
        nested_surface: bool,
    ) -> Option<verter_type_expr::TypeExpr> {
        let materialised_id =
            self.materialize_member_surface_to_node(scope_canonical_id, base, nested_surface)?;
        // Publication sink: materialize into a sealed carrier and unwrap via
        // the query-engine output capability.
        let dispatch = ProjectSemanticDispatch::new(self.ctx);
        let cap = MetaQueryRegistryOutputCap::new(&dispatch);
        cap.materialize_output_type_expr(materialised_id)
            .map(|raised| raised.into_type_expr(&cap))
    }

    /// First-pass (`MaterializeStructureDb`) node: the producing
    /// `SemanticNodeId` the member surface materialises to, BEFORE the raise
    /// to a `TypeExpr`. The node-domain shared core of
    /// [`Self::materialize_member_surface_node_core`] (which raises this node)
    /// AND the registry member-surface stabiliser (which REDUCES this node
    /// directly through the `ShapeCacheDb` member-node slot — the node-first
    /// second pass that never pays the raise + re-lower round-trip).
    pub(super) fn materialize_member_surface_to_node(
        &mut self,
        scope_canonical_id: &str,
        base: crate::semantic_query::SemanticNodeId,
        nested_surface: bool,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        use crate::component_meta_materialize::{
            materialize_component_meta_structure, MaterializationScope, MaterializeOutcome,
            MaterializeRuntimeKey,
        };

        // Publication demand per scope axis, shallow-by-default. The
        // TOP-LEVEL registry-symbol surface materialises at `Shallow` —
        // the interpretable one-level surface whose heritage arms merge
        // into a single Object (member names + shallow carrier values),
        // the same contract `dispatch_root_instantiated` reads — so a
        // registry consumer re-resolving `interface Extended extends
        // Base` sees the flattened key set, not the raw heritage
        // intersection. NESTED member surfaces materialise at
        // `Navigate`: member values stay carriers the consumer
        // re-resolves on demand. Open carriers survive either mode
        // through the shared L1 carrier-stop predicates (no
        // registry-local pre-walk runs here).
        let key = MaterializeRuntimeKey {
            scope_canonical_id: std::sync::Arc::from(scope_canonical_id),
            base,
            scope_axis: if nested_surface {
                MaterializationScope::Nested
            } else {
                MaterializationScope::TopLevel
            },
            mode: if nested_surface {
                ProjectionMode::Navigate
            } else {
                ProjectionMode::Shallow
            },
        };
        let read = materialize_component_meta_structure(self.ctx, key);
        // Dual-emit dispatch facts into BOTH downstream channels so
        // the legacy `state.fact_versions` curated signature and the
        // outer `with_fact_tracer` scope both observe the
        // materialiser's dep graph.
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        match read.value {
            MaterializeOutcome::Value(id)
            | MaterializeOutcome::Miss(id)
            | MaterializeOutcome::Recursive(id)
            | MaterializeOutcome::Tainted(id) => Some(id),
            MaterializeOutcome::Error(_) => None,
        }
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

    /// The named declaration's authored body LOCATOR (content-free) —
    /// consumers lower it through the ONE shared dispatch on demand.
    pub fn named_decl_body(
        &mut self,
        canonical_id: &str,
        name: &str,
    ) -> Option<verter_type_expr::locators::AuthoredBodyLocator> {
        self.prepared_type_decl(canonical_id, name)
            .map(|prepared| prepared_decl_authored_body_locator(&prepared))
    }

    /// The prepared type declaration for `(canonical_id, symbol_name)`,
    /// memoized in the engine's per-request scratch.
    ///
    /// **A degraded `None` is never memoized.** The read's `None` has TWO
    /// causes: an honest absence (the symbol is not declared in the keyed
    /// canonical), and a BROKEN DECL-BODY LEASE — `PreparedDeclBundle::get`
    /// fans [`NonCacheableReadReason::LeaseMiss`] and returns `None` while
    /// leaving its write-once slot VACANT, precisely so a later demand under a
    /// live lease RECOVERS the declaration. The decl-body memo evicts its
    /// poisoned cell for the same reason. A scratch memo that persists the
    /// degraded `None` undoes that care for the engine's whole scope: the
    /// recoverable declaration becomes a permanent absence for every later
    /// lookup in the request.
    ///
    /// The two causes are told apart by the CACHEABILITY RAIL, never by the
    /// value: the read runs inside its own cacheability scope, and a `None`
    /// whose read consumed a non-cacheable signal leaves the scratch slot
    /// VACANT (the next lookup retries), mirroring the layers below. The
    /// nested scope observes only; every fact and every non-cacheability mark
    /// still fans out to each enclosing tracer, so a producer bracketing this
    /// read still refuses its own shared-cache admission.
    ///
    /// [`NonCacheableReadReason::LeaseMiss`]: crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss
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

        let ctx = self.ctx;
        let (resolved, non_cacheable) = crate::fact_signature_helpers::with_cacheability_scope(
            ctx.host_for_fact_tracer_install(),
            |_probe| {
                ctx.prepared_type_decl(canonical_id, symbol_name)
                    .or_else(|| {
                        // Lazy first-time loading (see scope_payload_for_scope comment).
                        ctx.ensure_loaded(canonical_id)
                            .then(|| ctx.prepared_type_decl(canonical_id, symbol_name))
                            .flatten()
                    })
            },
        );
        if resolved.is_none() && non_cacheable {
            // Degraded miss (broken decl-body lease / fenced serve): the symbol
            // may well exist. Leave the slot VACANT so a later lookup — after
            // the transient clears — reaches the declaration.
            return None;
        }
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

    pub(super) fn semantic_dispatch(&self) -> ProjectSemanticDispatch<'_> {
        ProjectSemanticDispatch::new(self.ctx)
    }

    /// Resolve the decl-anchor node for `(scope, symbol)` — the
    /// `ResolveDecl` placeholder for the root declaration BEFORE any
    /// `Instantiate` step. This node still carries the declaration's
    /// heritage / `Omit` carrier intact (the post-`Published(Expanded)`
    /// instantiated root can collapse a generic carrier arm to
    /// `Opaque(Miss)`), so it is the correct base for the shared
    /// empty-path Shallow surface walker when composing a compound root.
    fn dispatch_decl_anchor(
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
        .unwrap_or_else(|| {
            let interner = self.ctx.project_type_store().identity_interner();
            (
                interner.intern(scope_canonical_id),
                interner.intern(symbol_name),
            )
        });
        let dispatch = self.semantic_dispatch();
        match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
            &resolved_root.0,
            &resolved_root.1,
        ))) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => Some(id),
            _ => None,
        }
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
        .unwrap_or_else(|| {
            let interner = self.ctx.project_type_store().identity_interner();
            (
                interner.intern(scope_canonical_id),
                interner.intern(symbol_name),
            )
        });
        let dispatch = self.semantic_dispatch();
        let anchor = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(
            resolve_decl_key(&resolved_root.0, &resolved_root.1),
        )) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
            _ => return None,
        };
        // R6: Instantiate.base is the env-bearing content-free
        // `ResolvedDeclSlotIdentity` slot (built via `type_slot_for`); the
        // cold build re-sources the live whole_hash from
        // `ensure_indexed_ready_serve`.
        let root_canonical: std::sync::Arc<str> = std::sync::Arc::clone(&resolved_root.0);
        let base = dispatch.type_slot_for(
            std::sync::Arc::clone(&root_canonical),
            std::sync::Arc::clone(&resolved_root.1),
        );
        let root_inst_ctx = dispatch.instantiate_context_for(
            &root_canonical,
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Shallow,
            ),
        );
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                base,
                empty_semantic_args(),
                // `dispatch_root_instantiated` feeds
                // `surface_view_from_semantic_node` which reads the
                // root's surface members, call/construct lists, etc. Shallow
                // yields the interpretable one-level surface (member names +
                // shallow carrier values) — decl-body lowering under Shallow
                // is carrier-preserving and the shallow-surface synthesiser
                // materialises exactly the demanded composition spine.
                root_inst_ctx,
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => Some(id),
            _ => Some(anchor),
        }
    }

    /// Project a root symbol's whole one-level [`SurfaceView`] AND return the
    /// graph node whose raised surface IS the projected surface: the
    /// instantiated root (whose own `Object` surface the projector read), or —
    /// when the compound-root composition fallback fires — the terminal
    /// `Object` node the shallow walker COMPOSED (NOT the carrier-intact decl
    /// anchor). The Whole route's node-domain materializedness gate reads that
    /// node's raised-shape facts directly instead of materializing the surface
    /// and inspecting it, so for BOTH cases the gate folds over the exact
    /// surface being published. The view stays node-native — the ONE registry
    /// publication materialisation happens at the terminal
    /// `surface_view_to_registry_type_expr` sink.
    pub(super) fn dispatch_projected_surface_with_node(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<(crate::semantic_query::SurfaceView, SemanticNodeId)> {
        let root = self.dispatch_root_instantiated(scope_canonical_id, symbol_name)?;
        if let Some(surface) = surface_view_from_semantic_node(self.ctx, root) {
            return Some((surface, root));
        }
        // The post-`Published(Expanded)` instantiated root did not yield a
        // complete surface — for a compound root that carries a generic
        // heritage / `Omit` carrier the instantiation can collapse an arm
        // to `Opaque(Miss)`, which the shared shallow walker cannot
        // re-resolve from the already-collapsed node. Compose the surface
        // from the decl anchor (carrier intact) through the SAME shared
        // empty-path Shallow surface walker
        // (`ProjectPath { [], MacroObjectSurface(Shallow) }`). This is the
        // one shared resolver driven from the non-lossy base — not a
        // parallel walker. Returns `None` when the anchor is unresolved or
        // the composed surface is empty.
        //
        // The gate node is the COMPOSED surface's terminal `Object` node (the
        // surface being published), NOT the carrier-intact anchor: the anchor's
        // own raise keeps heritage / import carriers unresolved (materialized)
        // and would admit a partial composed surface the surface-materialization
        // filter rejects.
        let anchor = self.dispatch_decl_anchor(scope_canonical_id, symbol_name)?;
        let (surface, composed_surface_node) =
            compound_root_surface_view_via_dispatch(self.ctx, anchor)?;
        Some((surface, composed_surface_node))
    }

    /// Heritage composition at the registry SURFACE-OBSERVATION point: when an
    /// owner-local declaration's raised body root is the lone-`extends`
    /// heritage intersection (`Intersection([DeclRef{Base}.., Object{own}])`),
    /// the registry consumer's one-level member surface composes through the
    /// SHARED empty-path Shallow surface walker (memoized under the existing
    /// structural `ProjectPath` query key — heritage-shadow precedence is
    /// decided THERE, never re-derived here) and publishes as a
    /// members-only [`ProjectedSurfaceFact`]: base + own members exactly once
    /// each, every member value a SHALLOW content-free slot resolved from its
    /// declaring contributor's prepared member facts.
    ///
    /// The raw `Intersection` STAYS the graph carrier for the declaration
    /// itself and for every non-surface observation — this method only
    /// projects the observed one-level surface into the entry's published
    /// SOURCE. Declines (`None`, caller keeps the raw carrier) for: a
    /// non-heritage body root, a generic heritage arm (`extends Base<T>` —
    /// slot facts cannot carry the substitution), a signature-bearing
    /// composed surface, or any composed member whose declaring contributor
    /// exposes no prepared member slot (no fabricated stand-ins).
    ///
    /// [`ProjectedSurfaceFact`]: verter_type_expr::facts::ProjectedSurfaceFact
    pub(crate) fn heritage_merged_surface_fact(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<verter_type_expr::facts::ProjectedSurfaceFact> {
        use crate::project_semantic_dispatch::node_data_for;
        use crate::semantic_query::SemanticNodeData;
        use verter_type_expr::facts::{ProjectedMemberFact, ProjectedSurfaceFact};

        if self.projection_op_budget_exhausted() {
            return None;
        }
        // The declaration's resolved root identity (the same bare-name
        // resolution the decl-anchor dispatch performs).
        let scope_payload_arc = self.scope_payload_for_scope(scope_canonical_id);
        let (own_canonical, own_name) =
            crate::resolver_core::bare_name_resolve::resolve_bare_name_in_scope(
                self.ctx,
                scope_canonical_id,
                scope_payload_arc.as_deref(),
                symbol_name,
            )
            .map(|root| (root.canonical_id, root.symbol_name))
            .unwrap_or_else(|| {
                let interner = self.ctx.project_type_store().identity_interner();
                (
                    interner.intern(scope_canonical_id),
                    interner.intern(symbol_name),
                )
            });
        // SHAPE classification runs on the raised DeclBody carrier (the
        // lowered body root) — the decl-anchor node is an identity
        // placeholder, not the body.
        let body_locator = self.named_decl_body(&own_canonical, &own_name)?;
        let body_root = {
            let dispatch = self.semantic_dispatch();
            dispatch
                .raise_authored_locator_to_hot(
                    &body_locator,
                    crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                        ProjectionMode::Navigate,
                    ),
                )
                .map(|hot| hot.node())?
        };
        let peel_alias = |mut node: SemanticNodeId| {
            while let Some(SemanticNodeData::Alias(inner)) =
                node_data_for(self.ctx, node).as_deref()
            {
                node = *inner;
            }
            node
        };
        // Lone-`extends` heritage shape only: heritage `DeclRef` arms plus
        // exactly ONE own-member `Object` arm.
        let arms = match node_data_for(self.ctx, peel_alias(body_root)).as_deref() {
            Some(SemanticNodeData::Intersection(arms)) => arms.clone(),
            _ => return None,
        };
        let mut heritage: Vec<(std::sync::Arc<str>, std::sync::Arc<str>)> = Vec::new();
        let mut own_object_arms = 0usize;
        for arm in arms.iter() {
            match node_data_for(self.ctx, peel_alias(*arm)).as_deref() {
                Some(SemanticNodeData::DeclRef { identity }) => heritage.push((
                    std::sync::Arc::clone(&identity.canonical_id),
                    std::sync::Arc::clone(&identity.decl_name),
                )),
                Some(SemanticNodeData::Object(_)) => own_object_arms += 1,
                _ => return None,
            }
        }
        if heritage.is_empty() || own_object_arms != 1 {
            return None;
        }
        // The one-level MERGED view through the shared empty-path Shallow
        // surface walker (arm roots only; member values stay shallow nodes).
        let (view, _surface_node) = compound_root_surface_view_via_dispatch(self.ctx, body_root)?;
        if !view.call_signatures.is_empty()
            || !view.construct_signatures.is_empty()
            || !view.index_signatures.is_empty()
            || view.has_index_signature
        {
            return None;
        }
        // Slot lookup mirrors the walker's heritage-shadow precedence: the own
        // declaration's prepared member facts win; heritage members resolve
        // from their declaring contributor in arm order.
        let own_prepared = self.prepared_type_decl(own_canonical.as_ref(), own_name.as_ref());
        let heritage_prepared: Vec<_> = heritage
            .iter()
            .filter_map(|(canonical, name)| {
                self.prepared_type_decl(canonical.as_ref(), name.as_ref())
            })
            .collect();
        if heritage_prepared.len() != heritage.len() {
            return None;
        }
        let mut members: Vec<ProjectedMemberFact> = Vec::with_capacity(view.members.len());
        for member in view.members.iter() {
            let fact = own_prepared
                .as_ref()
                .and_then(|prepared| prepared.member_index.get(member.name.as_ref()))
                .or_else(|| {
                    heritage_prepared
                        .iter()
                        .find_map(|prepared| prepared.member_index.get(member.name.as_ref()))
                })?;
            members.push(ProjectedMemberFact {
                name: member.name.as_ref().to_string(),
                optional: fact.optional,
                readonly: fact.readonly,
                is_method: fact.is_method,
                visibility: fact.visibility,
                declared_in_macro_type_arg: false,
                declaration_origin: fact.declaration_origin.clone(),
                ty: fact.ty.clone(),
                span_origin: fact.span_origin.clone(),
            });
        }
        Some(ProjectedSurfaceFact {
            members: std::sync::Arc::from(members.into_boxed_slice()),
            call_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            has_index_signature: false,
        })
    }

    /// Node-domain registry route projection: resolve a route to its admitted
    /// surface NODE, gated on the node-domain `RaisedShapeFacts.materialized`
    /// fact (the typed equivalent of the former
    /// `.filter(dispatch_route_expr_is_materialized)` over the materialised
    /// route TypeExpr). The MemberPath / Pick / Omit routes carry a single
    /// admitted node; the Whole route projects a SurfaceView (no single node),
    /// so it returns `None` here (a whole surface is served by the registry
    /// whole-surface candidate). The route fixpoint's registry fast-path projects
    /// through THIS node-returning form so no per-iteration materialisation
    /// happens; the publication wrapper below materialises the accepted node
    /// once at the registry sink.
    pub(crate) fn dispatch_routed_expr_surface_node(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<AdmittedRouteProjectionNode> {
        match route {
            RouteDemand::MemberPath(path) if !path.is_empty() => {
                let root = self.dispatch_root_instantiated(scope_canonical_id, root_symbol)?;
                let query_path: std::sync::Arc<[PathSegment]> = std::sync::Arc::from(
                    path.iter()
                        .map(|segment| PathSegment::Member(std::sync::Arc::from(segment.as_str())))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let dispatch = self.semantic_dispatch();
                match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
                    base: root,
                    path: query_path,
                    // Publication caller: intermediate hops navigate, the
                    // terminal hop publishes shallow-by-default.
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Navigate,
                    ),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: node, .. }) => {
                        // Node-domain materializedness gate (the typed
                        // equivalent of the former
                        // `.filter(dispatch_route_expr_is_materialized)`); the
                        // gated mint is `route_admission::admit_materialized`,
                        // which admits the node WITHOUT materialising it.
                        node_raised_shape_facts_with_dispatch(&dispatch, node)
                            .and_then(|witness| route_admission::admit_materialized(&witness))
                    }
                    _ => None,
                }
            }
            // Pick/Omit route through the SHARED semantic builtin engine
            // (`build_builtin_utility`'s Pick/Omit arms), NOT a name-only hand
            // filter over the projected surface. Routing through the shared
            // engine inherits its public-keyspace gate: `Pick<C,K>` / `Omit<C,K>`
            // over a class never re-mint a non-public member (the same
            // visibility filter the typed-IR derivation applies). A bare
            // name-only filter over the projected surface would bypass that
            // gate and leak protected/private members.
            RouteDemand::Pick(members) if !members.is_empty() => self
                .dispatch_routed_pick_omit_via_shared_engine_node(
                    scope_canonical_id,
                    root_symbol,
                    "Pick",
                    members.as_slice(),
                ),
            RouteDemand::Omit(members) if !members.is_empty() => self
                .dispatch_routed_pick_omit_via_shared_engine_node(
                    scope_canonical_id,
                    root_symbol,
                    "Omit",
                    members.as_slice(),
                ),
            _ => None,
        }
    }

    /// Route a `RouteDemand::Pick` / `RouteDemand::Omit` through the SHARED
    /// semantic builtin engine, exactly like the materialiser's Pick/Omit arm
    /// (`component_meta_materialize.rs`): a two-step dispatch that (A)
    /// instantiates the route ROOT to a projectable body, then (B) instantiates
    /// the `Pick` / `Omit` builtin carrier on `[body, keys]` in the caller's
    /// publication mode. The builtin engine's public-keyspace gate
    /// (`build_builtin_utility`) is therefore the single owner of the Pick/Omit
    /// projection — no second name-only hand filter, so non-public class members
    /// can never be re-minted onto the routed surface.
    fn dispatch_routed_pick_omit_via_shared_engine_node(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        builtin_name: &str,
        keys: &[String],
    ) -> Option<AdmittedRouteProjectionNode> {
        // Step A: instantiate the route root to a projectable body. Navigate
        // keeps generic carriers intact (the builtin engine re-projects in the
        // caller's mode), mirroring the materialiser's Step A.
        let body_id = self.dispatch_root_instantiated(scope_canonical_id, root_symbol)?;
        let dispatch = self.semantic_dispatch();
        let keys_node = crate::meta_resolve::build_keys_union_node(dispatch.graph(), keys);
        // Step B: instantiate the shared builtin Pick/Omit carrier on
        // `[body, keys]` in the publication Navigate mode — the same path as
        // a userland `Pick<…>` / `Omit<…>`, so fix-#1's public gate applies
        // and the L1 reducer decides closed→materialise path-precisely.
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                dispatch.builtin_type_slot(builtin_name),
                std::sync::Arc::from(vec![body_id, keys_node].into_boxed_slice()),
                dispatch.instantiate_context_for(
                    "__builtin__",
                    crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Navigate,
                    ),
                ),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => {
                // Node-domain materializedness gate (the typed equivalent of the
                // former `.filter(dispatch_route_expr_is_materialized)`); the gated
                // mint is `route_admission::admit_materialized`, which admits the
                // node WITHOUT materialising it — the publication wrapper
                // materialises once at the registry sink.
                node_raised_shape_facts_with_dispatch(&dispatch, node)
                    .and_then(|witness| route_admission::admit_materialized(&witness))
            }
            _ => None,
        }
    }
}

/// Mint the content-free authored-body locator for a prepared declaration:
/// the prepared decl's own `body_facts.body_slot` (anchored on the producing
/// canonical + symbol by the prepared-decl producer). This is the ONE
/// locator mint for the owner-collection / named-decl-body read surfaces, so
/// the cache value and every fallback arm publish an identical
/// representation.
pub(super) fn prepared_decl_authored_body_locator(
    prepared: &verter_semantic::analysis::type_solver::PreparedTypeDecl,
) -> verter_type_expr::locators::AuthoredBodyLocator {
    verter_type_expr::locators::AuthoredBodyLocator::DeclBody(prepared.body_facts.body_slot.clone())
}
