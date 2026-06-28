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
//!   `pub fn named_decl_body`, `pub fn prepared_member_raw_type` — all `pub`
//!   on the engine, callable from outside the crate.
//! - `pub(crate) fn materialize_member_surface_expr`,
//!   `pub(crate) fn prepared_type_decl`, `pub(crate) fn ctx`,
//!   `pub(crate) fn dispatch_projected_surface`,
//!   `pub(crate) fn dispatch_routed_expr_surface_expr` — crate-visible
//!   helpers used by `meta_resolve` and other engine impl methods.
//! - Private methods (`semantic_dispatch`, `dispatch_root_instantiated`)
//!   stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.
//!
//! The engine's fuse / fanout-budget / cache-length / debug accessors live in
//! the sibling `engine_accessors` module.

use verter_semantic::analysis::type_solver::query_engine::ProjectedSurface;
use verter_type_expr::TypeExpr;

use super::helpers::{
    is_builtin_name, resolve_imported_registry_symbol_with_budget, ImportedRegistrySymbolResolution,
};
use super::surface::{
    project_admitted_route_node_to_expanded_object_shape,
    projected_compound_root_surface_via_dispatch, projected_surface_from_semantic_node,
    projected_surface_to_expanded_shape, projected_surface_to_type_expr,
};
use super::AdmittedRouteProjectionNode;
use super::{
    empty_semantic_args, engine_fact_signature_for_exported_type,
    local_type_symbol_metadata_for_known_source, ComponentMetaQueryEngine,
    DirectPreparedDeclarationResolver, ResolvedImportedRegistrySymbol, ResolvedTypeDeclaration,
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
    pub fn resolve_imported_registry_symbol(
        &mut self,
        canonical_id: &str,
        exported_name: &str,
    ) -> Option<ResolvedImportedRegistrySymbol> {
        let key = (canonical_id.to_string(), exported_name.to_string());
        if let Some(cached) = self.imported_registry_symbols.borrow().get(&key).cloned() {
            return cached;
        }
        // Route through the ctx-owned `ImportedRegistryDb`. The local
        // RefCell view above is non-authoritative scratch; the
        // DashMap-backed DB is the authoritative cross-request cache.
        //
        // Singleflight shape: peek the shared DB first, and on a miss
        // run the resolution INSIDE the cold-build `compute` closure that
        // `ImportedRegistryDb::get_or_compute_admit` drives through the
        // query-identity `query::lookup` split-publish path.
        // `resolve_imported_registry_symbol_with_budget` consumes the
        // wildcard-route fuse (`allow_wildcard_route()` /
        // `wildcard_route_fanout`) on the slow lane — a side-effecting,
        // per-request budget. Running it inside the cooperative flight
        // slot is what bounds that cost to ONE winner: when several
        // requests miss the same key concurrently, exactly one runs the
        // closure and joiners block on the slot condvar and reuse its
        // value. The closure returns `ComputeAdmission::Cacheable` when
        // the provenance-pure signature builds and
        // `ComputeAdmission::ReturnOnly` when it cannot — `ReturnOnly`
        // still returns (and broadcasts) the freshly-resolved value
        // without admitting the cache and without re-running the
        // resolution.
        let arc_key = (
            std::sync::Arc::<str>::from(canonical_id),
            std::sync::Arc::<str>::from(exported_name),
        );
        // Bind the resolver context to a local `Copy` reference so the
        // request-local view inserts before and after the cooperative
        // admission call below borrow `self` only through its `RefCell`
        // fields, never through the `ctx` field.
        let ctx = self.ctx;
        let host_db = ctx.project_type_store().imported_registry_db();
        if let Some(opt_arc) = host_db.peek(&arc_key, ctx) {
            let cached = opt_arc.as_deref().cloned();
            // Per-request audit attribution: imported-registry-symbol
            // served from a host-cache peek. Differentiate warm
            // positive from warm negative (`None`) so the audit
            // reflects how many of the warm hits were actually
            // "this symbol is known unresolvable".
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRegistryWarm);
                if cached.is_none() {
                    obs.record_event(verter_audit::AuditEvent::ImportedRegistryNegative);
                }
            }
            self.imported_registry_symbols
                .borrow_mut()
                .insert(key, cached.clone());
            return cached;
        }
        // Cross-thread singleflight rendezvous seam: when the
        // imported-registry post-peek barrier is armed for this keyed
        // canonical, block here so every contending thread is past its
        // `peek` miss before any enters cooperative admission. A no-op
        // in production and whenever the gate is unarmed.
        #[cfg(test)]
        super::await_imported_registry_post_peek_barrier_for_tests(canonical_id);
        // Cold path — observe the keyed canonical's content version
        // ONCE here, before the value is computed, through the
        // view-aware `authoritative_current_content_hash` oracle —
        // under a `SessionResolverContext` this resolves the overlay
        // content hash for an overlay-bearing session, so an
        // overlay-derived entry roots on the overlay version (and a
        // later base request mismatches it instead of reusing it). The
        // signature builder is provenance-pure: it roots the entry's
        // self-root on this observed hash, never a current-content
        // re-read inside the cooperative-admission closure. `None`
        // (canonical has no authoritative current content) refuses
        // shared-cache admission — but the freshly-computed value is
        // still returned via `ComputeAdmission::ReturnOnly`. The
        // observed hash is captured HERE, before the closure, and
        // `move`-captured in, so provenance purity holds regardless of
        // which thread wins the singleflight.
        let observed_keyed_hash = ctx.authoritative_current_content_hash(canonical_id);
        // Test-only injection: simulate a concurrent request that
        // validated-and-published this key into the shared DB inside
        // this request's cold window — after the `peek` miss above,
        // before the `get_or_compute_admit` call below.
        // `get_or_compute_admit` then takes its warm-hit `validate` arm
        // and returns the injected value without running the compute
        // closure, exactly as it would under a real concurrent publish.
        #[cfg(test)]
        super::INJECT_IMPORTED_REGISTRY_CONCURRENT_PUBLISH.with(|slot| {
            if let Some(symbol) = slot.borrow().clone() {
                if let crate::cache_runtime::SignatureAdmission::Cacheable(sig) =
                    engine_fact_signature_for_exported_type(
                        ctx,
                        canonical_id,
                        exported_name,
                        observed_keyed_hash.expect(
                            "concurrent-publish injection fixture requires an observed keyed hash",
                        ),
                    )
                {
                    host_db.insert_for_test(
                        arc_key.clone(),
                        std::sync::Arc::new(crate::component_meta_caches::ImportedRegistryEntry {
                            value: Some(std::sync::Arc::new(symbol)),
                            fact_dep_signature: sig.facts,
                            // A simulated concurrent publish stamps the
                            // live project generation, exactly as the
                            // real cold-compute path does.
                            validated_at_generation: ctx
                                .project_type_store()
                                .current_project_generation(),
                        }),
                    );
                }
            }
        });
        // Cooperative-admission cold compute. The expensive,
        // fuse-consuming `resolve_imported_registry_symbol_with_budget`
        // resolution runs INSIDE the `compute` closure, so it runs
        // exactly ONCE per key across all concurrent waiters: the
        // `InflightTable` singleflight elects one winner to run the
        // closure while joiners block on the slot condvar and reuse the
        // winner's value. `allow_wildcard_route()` — and therefore the
        // `wildcard_route_fanout` fuse — is consumed only by the
        // winner.
        //
        // The closure returns a `ComputeAdmission`:
        //
        // - `Cacheable` — the provenance-pure fact signature built; the
        //   entry is admitted and joiners re-read it.
        // - `ReturnOnly` — the resolution produced a valid value but
        //   the signature could not be built (no observed shallow
        //   state for the keyed canonical) or the test refusal hook
        //   fired; the value is still returned to this caller and
        //   broadcast to joiners, the cache stays empty, and the
        //   resolution is NOT re-run (no second fuse consumption).
        //
        // `get_or_compute_admit` returns `Option<Option<Arc<_>>>`:
        //
        // - `Some(cached)` — a validated value is authoritative: this
        //   request's own freshly-computed `Cacheable`/`ReturnOnly`
        //   outcome, OR an entry a CONCURRENT request published into
        //   the DB between the `peek` miss above and this call (the
        //   warm-hit `validate` arm returns it without running the
        //   closure).
        // - `None` — `compute` returned `Failed`, or post-compute
        //   revalidation rejected the freshly-built entry (a file
        //   mutated mid-compute). The request resolves to a transient
        //   miss; the next request cold-recomputes. The resolution is
        //   never re-run on this path.
        let host_value = host_db.get_or_compute_admit(&arc_key, ctx, || {
            #[cfg(test)]
            super::IMPORTED_REGISTRY_RESOLVE_INVOCATIONS.with(|n| n.set(n.get().saturating_add(1)));
            // Cross-thread singleflight slot-coalescing rendezvous seam:
            // when the imported-registry winner-park gate is armed for
            // this keyed canonical, the cold winner blocks here — AFTER
            // it has claimed the in-flight slot (so `claimed == true` is
            // already published and every later arrival is forced onto
            // the joiner branch) and BEFORE it runs the fuse-consuming
            // resolution / publishes / retires the slot. The test
            // releases the winner only once it has proven every joiner
            // has coalesced onto this slot, so no worker is left
            // mid-flight between its `map.get` miss and its slot claim —
            // closing the window in which a descheduled worker would
            // form a second cold winner and tick the wildcard-route fuse
            // again. A no-op in production and whenever the gate is
            // unarmed.
            #[cfg(test)]
            super::await_imported_registry_winner_park_for_tests(canonical_id);
            // Per-request audit attribution: cold path running the
            // expensive `resolve_imported_registry_symbol_with_budget`
            // resolution. Joiners that block on this closure do NOT
            // re-enter — so the counter reflects unique cold work,
            // not per-waiter overhead.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRegistryCold);
            }
            // Snapshot the project generation BEFORE the resolution
            // dispatches any work. The `fact_dep_signature` carrier
            // validates only file-content whole-hashes; a
            // `ProjectGeneration` reset (tsconfig / path-alias / SDK /
            // workspace-folder change) bumps no file content, so the
            // entry carries its compute-time generation explicitly. The
            // read-side gates reject the entry once the live generation
            // moves past this snapshot.
            let validated_at_generation = ctx.project_type_store().current_project_generation();
            // The single, side-effecting resolution: the wildcard-route
            // fuse is consumed here at most once per key.
            let resolved: Option<ResolvedImportedRegistrySymbol> =
                match resolve_imported_registry_symbol_with_budget(
                    ctx,
                    canonical_id,
                    exported_name,
                    || self.allow_wildcard_route(),
                ) {
                    ImportedRegistrySymbolResolution::Resolved(opt) => opt,
                    ImportedRegistrySymbolResolution::FuseTripped => {
                        // The wildcard route was needed but the
                        // per-request fuse was exhausted, so the symbol
                        // was NEVER looked up. This `None` is a GENUINE
                        // PARTIAL — admitting it as a warm negative would
                        // poison subsequent identical requests that DO
                        // have budget. Mark the request partial sticky so
                        // the whole component-meta result refuses to warm,
                        // and route the absent value through
                        // `ReturnOnly(None)` (NOT a cacheable negative).
                        crate::request_context::mark_request_materialization_cache_suppress();
                        let _reason_guard = crate::cache_runtime::SetReasonGuard::arm(
                            crate::cache_runtime::NonAdmissionReason::PartialResult,
                        );
                        return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(
                            None,
                        );
                    }
                };
            let resolved_value = resolved.map(std::sync::Arc::new);
            #[cfg(test)]
            if super::FORCE_IMPORTED_REGISTRY_ADMISSION_REFUSAL.with(|f| f.get()) {
                // Deterministically reproduce the production
                // admission-refusal contract (`engine_fact_signature_*`
                // returns `None`) so the discriminating test can drive
                // the refused-admission path without manufacturing a
                // stale observed hash. The freshly-resolved value is
                // still returned — and broadcast to joiners — via
                // `ReturnOnly`.
                let _reason_guard = crate::cache_runtime::SetReasonGuard::arm(
                    crate::cache_runtime::NonAdmissionReason::ForcedTestRefusal,
                );
                return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(
                    resolved_value,
                );
            }
            let Some(observed) = observed_keyed_hash else {
                // No authoritative current content for the keyed
                // canonical — shared-cache admission is refused, but
                // the value is still returned via `ReturnOnly`. The
                // missing current-content read means the provenance
                // could not be rooted to a self-root canonical, so
                // a cross-view joiner could never view-validate it.
                let _reason_guard = crate::cache_runtime::SetReasonGuard::arm(
                    crate::cache_runtime::NonAdmissionReason::UnresolvedProvenance,
                );
                return crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(
                    resolved_value,
                );
            };
            match engine_fact_signature_for_exported_type(
                ctx,
                canonical_id,
                exported_name,
                observed,
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    crate::cache_runtime::singleflight::ComputeAdmission::Cacheable(
                        crate::component_meta_caches::ImportedRegistryEntry {
                            value: resolved_value,
                            fact_dep_signature: sig.facts,
                            validated_at_generation,
                        },
                    )
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
                    // Pass the typed refusal reason through the TLS
                    // bridge so the downstream `CacheAdmission`
                    // lowering attributes the correct structured
                    // refusal reason instead of hard-coding
                    // `SignatureOverflow`.
                    let _reason_guard = crate::cache_runtime::SetReasonGuard::arm(reason);
                    crate::cache_runtime::singleflight::ComputeAdmission::ReturnOnly(resolved_value)
                }
            }
        });
        let result = match host_value {
            Some(cached) => cached.as_deref().cloned(),
            None => None,
        };
        // Per-request audit attribution: a `None` result on the cold
        // path indicates the imported-registry-symbol resolution
        // could not find the symbol at all from the owner. The warm
        // peek branch above handles the warm-negative case separately.
        if result.is_none() {
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRegistryNegative);
            }
        }
        self.imported_registry_symbols
            .borrow_mut()
            .insert(key, result.clone());
        result
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
    pub(crate) fn materialize_member_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
        nested_surface: bool,
    ) -> verter_type_expr::TypeExpr {
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
        self.materialize_member_surface_node_core(scope_canonical_id, base, nested_surface)
            .unwrap_or_else(|| expr.clone())
    }

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
    fn materialize_member_surface_to_node(
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

    /// TEST-ONLY node-input reach for the handle-capable equivalence fixtures,
    /// which lower a fixture `TypeExpr` to a node IN THE TEST and assert the
    /// node-core produces the same surface the `TypeExpr` arm
    /// ([`Self::materialize_member_surface_expr`]) yields. Named `_for_test` so
    /// it can never masquerade as a production node-input API; gated
    /// `#[cfg(test)]` so the forgeable-`SemanticNodeId`-input surface has ZERO
    /// footprint outside test builds (the production node-core stays
    /// module-private, and out-of-subtree production callers reach the surface
    /// only through the demand APIs `materialize_pick_member_surface` /
    /// `project_expr_to_surface_shape`).
    #[cfg(test)]
    pub(crate) fn materialize_member_surface_node_for_test(
        &mut self,
        scope_canonical_id: &str,
        base: crate::semantic_query::SemanticNodeId,
        nested_surface: bool,
    ) -> Option<verter_type_expr::TypeExpr> {
        self.materialize_member_surface_node_core(scope_canonical_id, base, nested_surface)
    }

    /// Resolve a type declaration, cached per query.
    pub fn resolve_type_declaration(
        &mut self,
        canonical_source: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        let key = (canonical_source.to_string(), requested_name.to_string());
        if let Some(cached) = self.declarations.borrow().get(&key).cloned() {
            return cached;
        }
        // Step 3 closure: route through ctx-owned DeclarationLookupDb.
        //
        // Observe the keyed canonical's content version ONCE here,
        // before the value is computed, through the view-aware
        // `authoritative_current_content_hash` oracle (overlay-correct
        // under a `SessionResolverContext`). The signature builder is
        // provenance-pure: it roots the entry's self-root on this
        // observed hash, never a current-content re-read inside the
        // closure. `None` (canonical has no authoritative current
        // content) refuses shared-cache admission; the `None`
        // host-value arm below still produces the value via the cold
        // resolver.
        let observed_keyed_hash = self
            .ctx
            .authoritative_current_content_hash(canonical_source);
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
            let observed = observed_keyed_hash?;
            match engine_fact_signature_for_exported_type(
                self.ctx,
                canonical_source,
                requested_name,
                observed,
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    Some((computed, sig.facts))
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
            }
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
        if let Some(cached) = self.resolvable.borrow().get(&key).copied() {
            return cached;
        }
        // Step 3 closure: route through ctx-owned ResolvabilityDb.
        //
        // Observe the keyed canonical's content version ONCE here,
        // before the value is computed, through the view-aware
        // `authoritative_current_content_hash` oracle (overlay-correct
        // under a `SessionResolverContext`). The signature builder is
        // provenance-pure: it roots the entry's self-root on this
        // observed hash, never a current-content re-read inside the
        // closure. `None` (canonical has no authoritative current
        // content) refuses shared-cache admission; the `None`
        // host-value arm below still produces the value by recomputing.
        let observed_keyed_hash = self.ctx.authoritative_current_content_hash(source_key);
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
            // If the imported-registry resolution above tripped the
            // wildcard-route fuse (which marked the request-result
            // completeness partial), the derived `false` is NOT an authoritative
            // "unresolvable" verdict — the symbol was never looked up.
            // Refuse to admit it into `ResolvabilityDb`; the caller still
            // recomputes the bool below so it never sees a spurious cached
            // `false`. The `ResolvabilityDb` rail has no per-value partial
            // flag, so it supplies the request-result completeness (one
            // request resolves one component's meta) to the pure gate.
            if crate::cache_runtime::refuse_result_cache_admission_if_partial(
                crate::request_context::current_materialization_cache_suppress(),
            ) {
                return None;
            }
            let observed = observed_keyed_hash?;
            match engine_fact_signature_for_exported_type(
                self.ctx,
                source_key,
                exported_name,
                observed,
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    Some((computed, sig.facts))
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
            }
        });
        // A `None` host-value means the signature builder refused
        // admission (or post-compute revalidation failed). Shared-cache
        // admission is forgone, but the boolean is still recomputed so
        // the caller never sees a spurious `false`.
        let resolved = match host_value {
            Some(value) => value,
            None => {
                if self.prepared_type_decl(source_key, exported_name).is_some() {
                    true
                } else {
                    self.resolve_imported_registry_symbol(source_key, exported_name)
                        .is_some()
                }
            }
        };
        self.resolvable.borrow_mut().insert(key, resolved);
        resolved
    }

    /// Get the owner's collection expression for a name, cached per query.
    pub fn owner_collection_expr(
        &mut self,
        owner_canonical: &str,
        name: &str,
    ) -> Option<verter_type_expr::TypeExpr> {
        if let Some(cached) = self.owner_collection_exprs.borrow().get(name).cloned() {
            return cached;
        }

        // Step 3 closure: route through ctx-owned OwnerCollectionDb.
        //
        // Observe the owner canonical's prepared decl AND the content
        // version it was materialised from from ONE prepared-decl
        // bundle via `observed_prepared_type_decl`. The cache value
        // (`prepared.body`) and the entry's fact-signature self-root
        // therefore root on a single, provably-consistent content
        // version — they cannot tear against a racing `upsert`, and the
        // observed hash is view-correct (the bundle is fetched through
        // the view-aware `prepared_decl_bundle` accessor). The signature
        // builder is provenance-pure: it never re-reads current content
        // inside the closure. `None` (owner canonical has no
        // prepared-decl bundle) refuses shared-cache admission; the
        // `None` host-value arm below still produces the body by
        // recomputing.
        let observed = self.observed_prepared_type_decl(owner_canonical, name);
        let arc_key = (
            std::sync::Arc::<str>::from(owner_canonical),
            std::sync::Arc::<str>::from(name),
        );
        let host_db = self.ctx.project_type_store().owner_collection_db();
        let host_value = host_db.get_or_compute(&arc_key, self.ctx, || {
            let observed = observed.as_ref()?;
            let computed = observed.decl.as_ref().map(|prepared| prepared.body.clone());
            // Root the signature on the canonical AND content version
            // the observation recorded — the value and the self-root
            // then provably agree on one content identity.
            match engine_fact_signature_for_exported_type(
                self.ctx,
                observed.canonical_id.as_str(),
                name,
                observed.whole_hash,
            ) {
                crate::cache_runtime::SignatureAdmission::Cacheable(sig) => {
                    Some((computed, sig.facts))
                }
                crate::cache_runtime::SignatureAdmission::NonCacheable(_) => None,
            }
        });
        let body: Option<verter_type_expr::TypeExpr> = match host_value {
            Some(opt_arc) => opt_arc.map(|arc_expr| arc_expr.as_ref().clone()),
            // The signature builder refused admission (or post-compute
            // revalidation failed). Shared-cache admission is forgone;
            // the body is still produced from a fresh prepared-decl
            // read so the caller gets the correct result.
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

    /// Resolve a prepared type declaration AND observe the content
    /// version it was materialised from — both sourced from the SAME
    /// prepared-decl bundle.
    ///
    /// A query-identity cache producer whose value is built from a
    /// `prepared_type_decl` read must root its fact signature on the
    /// content version the value was actually built from — never a
    /// later current-content re-read, which would let an `upsert`
    /// landing in the publish-race window admit a stale value under a
    /// fresh signature.
    ///
    /// This accessor fetches `canonical_id`'s prepared-decl bundle once
    /// through [`crate::resolver_core::ResolverContext::prepared_decl_bundle`]
    /// — which, under a `SessionResolverContext`, routes to the
    /// view-aware `prepared_decl_bundle_with_context` so an
    /// overlay-bearing session observes the overlay's bundle. The
    /// returned `decl` is the bundle's prepared decl for `symbol_name`;
    /// the returned `whole_hash` is
    /// [`crate::resolver_core::prepared_decl::PreparedTypeDeclCache::defining_content_hash`]
    /// — the `whole_hash` of the very `ShallowFileState` that bundle's
    /// prepared decls are built from. One bundle ⇒ the decl and the
    /// hash are provably the same content version (untorn against a
    /// racing `upsert`) AND the hash is view-correct (it reflects
    /// whatever view the bundle was materialised from). The producer
    /// threads this ONE observation into both the value and the
    /// provenance-pure signature builder.
    ///
    /// `None` when `canonical_id` has no prepared-decl bundle (unloaded
    /// / evicted); the producer then refuses shared-cache admission.
    /// The `decl` field is `Option` because a prepared decl may
    /// legitimately be absent for a bundled canonical (the requested
    /// symbol does not exist), but the absence is still rooted on the
    /// observed hash so a later declaration is detected.
    pub(crate) fn observed_prepared_type_decl(
        &mut self,
        canonical_id: &str,
        symbol_name: &str,
    ) -> Option<crate::resolver_core::component_meta_query_engine::ObservedPreparedTypeDecl> {
        let bundle = self.ctx.prepared_decl_bundle(canonical_id)?;
        let whole_hash = bundle.prepared_type_decls.defining_content_hash();
        let decl = bundle.prepared_type_decls.get(symbol_name);
        // Mirror the bundle decl into the engine's per-request
        // read-through cache so a later `prepared_type_decl` call for
        // the same `(canonical_id, symbol_name)` hits the warm scratch
        // entry instead of re-resolving the bundle.
        self.prepared_type_decls.insert(
            (canonical_id.to_string(), symbol_name.to_string()),
            decl.clone(),
        );
        Some(
            crate::resolver_core::component_meta_query_engine::ObservedPreparedTypeDecl {
                decl,
                canonical_id: canonical_id.to_string(),
                whole_hash,
            },
        )
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
        .unwrap_or_else(|| (scope_canonical_id.to_string(), symbol_name.to_string()));
        let dispatch = self.semantic_dispatch();
        match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
            resolved_root.0.as_str(),
            resolved_root.1.as_str(),
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
        .unwrap_or_else(|| (scope_canonical_id.to_string(), symbol_name.to_string()));
        let dispatch = self.semantic_dispatch();
        let anchor = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(
            resolve_decl_key(resolved_root.0.as_str(), resolved_root.1.as_str()),
        )) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => id,
            _ => return None,
        };
        // R6: Instantiate.base is the env-bearing content-free
        // `ResolvedDeclSlotIdentity` slot (built via `type_slot_for`); the
        // cold build re-sources the live whole_hash from
        // `ensure_indexed_ready_serve`.
        let root_canonical: std::sync::Arc<str> = std::sync::Arc::from(resolved_root.0.as_str());
        let base = dispatch.type_slot_for(
            std::sync::Arc::clone(&root_canonical),
            std::sync::Arc::from(resolved_root.1.as_str()),
        );
        let root_inst_ctx = dispatch.instantiate_context_for(
            &root_canonical,
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Shallow,
            ),
        );
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base,
            args: empty_semantic_args(),
            // `dispatch_root_instantiated` feeds
            // `projected_surface_from_semantic_node` which reads the
            // root's surface members, call/construct lists, etc. Shallow
            // yields the interpretable one-level surface (member names +
            // shallow carrier values) — decl-body lowering under Shallow
            // is carrier-preserving and the shallow-surface synthesiser
            // materialises exactly the demanded composition spine.
            context: root_inst_ctx,
        }) {
            QueryResult::Value(SemanticQueryOutput { value: id, .. }) => Some(id),
            _ => Some(anchor),
        }
    }

    pub(crate) fn dispatch_projected_surface(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<ProjectedSurface> {
        self.dispatch_projected_surface_with_node(scope_canonical_id, symbol_name)
            .map(|(surface, _node)| surface)
    }

    /// As [`Self::dispatch_projected_surface`], but also returns the graph node
    /// whose raised surface IS the projected surface: the instantiated root
    /// (whose own `Object` surface the projector read), or — when the
    /// compound-root composition fallback fires — the terminal `Object` node the
    /// shallow walker COMPOSED (NOT the carrier-intact decl anchor). The Whole
    /// route's node-domain materializedness gate reads that node's raised-shape
    /// facts directly instead of materializing the surface and inspecting it, so
    /// for BOTH cases the gate folds over the exact surface being published.
    fn dispatch_projected_surface_with_node(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<(ProjectedSurface, SemanticNodeId)> {
        let root = self.dispatch_root_instantiated(scope_canonical_id, symbol_name)?;
        if let Some(surface) = projected_surface_from_semantic_node(self.ctx, root) {
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
            projected_compound_root_surface_via_dispatch(self.ctx, anchor)?;
        Some((surface, composed_surface_node))
    }

    /// Node-domain registry route projection: resolve a route to its admitted
    /// surface NODE, gated on the node-domain `RaisedShapeFacts.materialized`
    /// fact (the typed equivalent of the former
    /// `.filter(dispatch_route_expr_is_materialized)` over the materialised
    /// route TypeExpr). The MemberPath / Pick / Omit routes carry a single
    /// admitted node; the Whole route projects a SurfaceView (no single node)
    /// and is served by [`Self::dispatch_routed_expr_surface_expr`] directly, so
    /// it returns `None` here. The route fixpoint's registry fast-path projects
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
                        // `.filter(dispatch_route_expr_is_materialized)`); admit
                        // the node WITHOUT materialising it.
                        node_raised_shape_facts_with_dispatch(&dispatch, node)
                            .filter(|facts| facts.materialized)
                            .map(|_| AdmittedRouteProjectionNode::new(node))
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
                    members,
                ),
            RouteDemand::Omit(members) if !members.is_empty() => self
                .dispatch_routed_pick_omit_via_shared_engine_node(
                    scope_canonical_id,
                    root_symbol,
                    "Omit",
                    members,
                ),
            _ => None,
        }
    }

    /// Publication terminal over [`Self::dispatch_routed_expr_surface_node`]:
    /// the Whole route projects a SurfaceView (gated on the node-domain
    /// materializedness of the surface's producing node, then published), while
    /// the MemberPath / Pick / Omit routes materialise their admitted node ONCE
    /// at the registry sink (the sealed [`MetaQueryRegistryOutputCap`]). No
    /// semantic decision is made on the materialised value — the acceptance gate
    /// is the node-domain fact in the node form above.
    pub(crate) fn dispatch_routed_expr_surface_expr(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        route: &RouteDemand,
    ) -> Option<TypeExpr> {
        match route {
            RouteDemand::Whole => {
                let (surface, surface_node) =
                    self.dispatch_projected_surface_with_node(scope_canonical_id, root_symbol)?;
                // Node-domain materializedness gate on the surface's producing
                // node (the instantiated root, or — for the compound-root
                // fallback — the composed surface's terminal `Object` node, not
                // the carrier-intact decl anchor) — the typed equivalent of the
                // former `.filter(dispatch_route_expr_is_materialized)` over the
                // materialised surface TypeExpr.
                let materialized =
                    node_raised_shape_facts_with_dispatch(&self.semantic_dispatch(), surface_node)
                        .is_some_and(|facts| facts.materialized);
                materialized
                    .then(|| projected_surface_to_type_expr(&surface))
                    .flatten()
            }
            _ => {
                let node =
                    self.dispatch_routed_expr_surface_node(scope_canonical_id, root_symbol, route)?;
                // Publication sink: materialize the admitted node into a sealed
                // carrier and unwrap via the query-engine registry output
                // capability — ONCE, with no decision on the result.
                let dispatch = self.semantic_dispatch();
                let cap = MetaQueryRegistryOutputCap::new(&dispatch);
                cap.materialize_output_type_expr(node.node())
                    .map(|raised| raised.into_type_expr(&cap))
            }
        }
    }

    /// Project a root symbol's surface to its whole-surface [`TypeExpr`].
    ///
    /// The sink-local composition of [`Self::dispatch_projected_surface`] +
    /// `projected_surface_to_type_expr`. Out-of-subtree callers (the
    /// `dispatch_helpers` host-threaded wrappers) reach this engine method rather
    /// than naming the subtree-confined raw `SurfaceView` / `SemanticNodeId`
    /// projection helpers — the forgeable-input projection stays inside the
    /// query-engine sink.
    pub(crate) fn dispatch_projected_surface_to_type_expr(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<TypeExpr> {
        let surface = self.dispatch_projected_surface(scope_canonical_id, symbol_name)?;
        projected_surface_to_type_expr(&surface)
    }

    /// Project an already-resolved surface node to its
    /// [`ExpandedObjectShape`](verter_semantic::analysis::type_expand::ExpandedObjectShape).
    ///
    /// The sink-local composition of `projected_surface_from_semantic_node` +
    /// `projected_surface_to_expanded_shape`. MODULE-PRIVATE node-core: the
    /// `node` is forgeable in safe Rust, so this never crosses the query-engine
    /// boundary. Out-of-subtree callers reach the shape through the demand API
    /// [`Self::project_expr_to_surface_shape`], which resolves `expr` to a node
    /// INTERNALLY.
    fn projected_expanded_shape_from_node_core(
        &self,
        node: SemanticNodeId,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        let surface = projected_surface_from_semantic_node(self.ctx, node)?;
        Some(projected_surface_to_expanded_shape(&surface))
    }

    /// Demand-based surface-shape API: project a root/surface EXPRESSION to its
    /// [`ExpandedObjectShape`](verter_semantic::analysis::type_expand::ExpandedObjectShape).
    ///
    /// The OUT-OF-SUBTREE entry point (the `dispatch_helpers` host-threaded
    /// wrapper): the caller passes a scope + `&TypeExpr`, never a resolved node.
    /// This resolves the expression to a surface node through the shared
    /// dispatch INTERNALLY, then projects via the private
    /// [`Self::projected_expanded_shape_from_node_core`] — the forgeable
    /// `SemanticNodeId` never leaves the query-engine sink.
    ///
    /// Three resolution arms, in priority order (matching the publication
    /// surface-gate contract):
    /// 1. a registry public-indexed-access / public-utility ROUTE
    ///    (`Foo['a']` / `Pick<Foo, …>`) projects through
    ///    [`Self::dispatch_routed_expr_surface_expr`] then to an object shape;
    /// 2. a direct utility surface ([`Self::project_direct_utility_surface_shape`]);
    /// 3. the general path — lower at `Navigate` (intermediate-base), run the
    ///    terminal empty-path `ProjectPath { .., Shallow }` (the publication
    ///    demand), then project the resolved node to its expanded shape.
    pub(crate) fn project_expr_to_surface_shape(
        &mut self,
        scope_canonical_id: &str,
        expr: &verter_type_expr::TypeExpr,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedObjectShape> {
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::resolver_core::component_meta_registry::{
            component_meta_registry_public_indexed_access_route,
            component_meta_registry_public_utility_route,
        };
        use crate::semantic_query::{
            PathSegment, ProjectionMode, QueryResult, SemanticQueryApi, SemanticQueryKey,
            SemanticQueryOutput,
        };

        // (1) Registry public-indexed-access / public-utility route — projected in
        // NODE DOMAIN: resolve the admitted route NODE (routes here are only
        // MemberPath / Pick / Omit, never `Whole`), then build the shape from its
        // SurfaceView; a non-object node → empty shape.
        //
        // `unwrap_or_else(empty)` is this arm's correct terminal, not a swallow of
        // arms 2/3: the admitted node already passed the route's node-domain
        // `materialized` gate, so the route DID resolve. A non-`Object` terminal (a
        // primitive / function leaf) projects to no one-level object surface and
        // yields `None`, which this arm publishes as the empty shape — a
        // resolved-but-non-object route has an empty object surface; it does not
        // fall through. The arm is only ENTERED when the route admits a materialised
        // node: a route that admits no node (`dispatch_routed_expr_surface_node ==
        // None`) skips this arm and still reaches arms 2/3.
        if let Some((root_symbol, route)) =
            component_meta_registry_public_indexed_access_route(expr)
                .or_else(|| component_meta_registry_public_utility_route(expr))
        {
            if let Some(node) =
                self.dispatch_routed_expr_surface_node(scope_canonical_id, &root_symbol, &route)
            {
                return Some(
                    project_admitted_route_node_to_expanded_object_shape(self.ctx, &node)
                        .unwrap_or_else(
                            verter_semantic::analysis::type_expand::ExpandedObjectShape::empty,
                        ),
                );
            }
        }
        // (2) Direct utility surface.
        if let Some(shape) = self.project_direct_utility_surface_shape(scope_canonical_id, expr) {
            return Some(shape);
        }
        // (3) General path: resolve the surface node through the shared dispatch
        // (intermediate-base lowering is `Navigate`; the terminal empty-path
        // `ProjectPath { .., Shallow }` carries the publication demand), then
        // project it via the private node-core — the raw `SemanticNodeId` never
        // leaves the sink.
        let node = {
            let dispatch = ProjectSemanticDispatch::new(self.ctx);
            let base = dispatch.lower_type_expr_in_scope_with_mode(
                scope_canonical_id,
                expr,
                ProjectionMode::Navigate,
            )?;
            let QueryResult::Value(SemanticQueryOutput { value: node, .. }) = dispatch
                .execute_type_node(SemanticQueryKey::ProjectPath {
                    base,
                    path: std::sync::Arc::from(Vec::<PathSegment>::new().into_boxed_slice()),
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Shallow,
                    ),
                })
            else {
                return None;
            };
            node
        };
        let shape = self.projected_expanded_shape_from_node_core(node)?;
        // An index-signature-only surface (`{ [k: string]: string }`) is a
        // genuine props surface — `defineProps<{ [k: string]: string }>()`
        // admits every string key. Admitting it here lets the owner-local root
        // gate (which already counts index signatures) see a non-empty shape;
        // gating on properties / call-signatures alone would drop an
        // index-sig-only root.
        (!shape.properties.is_empty()
            || !shape.call_signatures.is_empty()
            || !shape.index_signatures.is_empty())
        .then_some(shape)
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
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base: dispatch.builtin_type_slot(builtin_name),
            args: std::sync::Arc::from(vec![body_id, keys_node].into_boxed_slice()),
            context: dispatch.instantiate_context_for(
                "__builtin__",
                crate::semantic_query::ProjectionReductionContext::published(
                    ProjectionMode::Navigate,
                ),
            ),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => {
                // Node-domain materializedness gate (the typed equivalent of the
                // former `.filter(dispatch_route_expr_is_materialized)`); admit
                // the node WITHOUT materialising it — the publication wrapper
                // materialises once at the registry sink.
                node_raised_shape_facts_with_dispatch(&dispatch, node)
                    .filter(|facts| facts.materialized)
                    .map(|_| AdmittedRouteProjectionNode::new(node))
            }
            _ => None,
        }
    }

    // ===================================================================
    // Node-first registry-candidate materialisation siblings.
    //
    // Each returns the published surface PAIRED with the node-domain
    // object-surface fact, decided off the PRODUCING node — so the host-side
    // registry loop carries a precomputed fact instead of inspecting the
    // materialised value. Member surfaces materialise through the first-pass
    // `MaterializeStructureDb` node (`materialize_member_surface_to_node`) and,
    // where the old path stabilised, REDUCE that node through the `ShapeCacheDb`
    // member-node slot (the node-first second pass) — never a raise-then-re-lower
    // of a materialised value to recover facts.
    // ===================================================================

    /// Whole-surface registry candidate for `symbol` in `scope`: the node-domain
    /// replacement for the former `project_type_surface_expr_via_host_threaded`
    /// bridge. Projects the symbol's whole surface (the same budget-gated
    /// `dispatch_projected_surface_with_node`), returns its `TypeExpr` plus the
    /// producing node's object-surface fact.
    pub(crate) fn materialize_registry_whole_surface_candidate(
        &mut self,
        scope_canonical_id: &str,
        symbol_name: &str,
    ) -> Option<(TypeExpr, bool)> {
        if self.projection_op_budget_exhausted() {
            return None;
        }
        let (surface, node) =
            self.dispatch_projected_surface_with_node(scope_canonical_id, symbol_name)?;
        let type_expr = projected_surface_to_type_expr(&surface)?;
        let is_object = component_meta_registry_node_has_explicit_object_surface(self.ctx, node);
        Some((type_expr, is_object))
    }

    /// Owner-local generic-alias substituted registry candidate: the relocated body
    /// of the former host-side `owner_local_generic_alias_substituted_body_via_dispatch`.
    /// Lowers the generic ref (Navigate), gates on the owner-local
    /// `InstantiationRef` carrier + the prepared-decl reach constraints, runs the
    /// shared `Instantiate` query, and gates the result NODE on raising EXACTLY to
    /// an object surface (the node-domain replacement for
    /// `matches!(raised, TypeExpr::Object(_))`) before materialising it ONCE.
    pub(crate) fn owner_local_generic_alias_candidate(
        &mut self,
        scope_canonical_id: &str,
        raw: &TypeExpr,
    ) -> Option<(TypeExpr, bool)> {
        use crate::project_semantic_dispatch::node_data_for;
        use crate::semantic_query::{ProjectionReductionContext, SemanticNodeData};

        let TypeExpr::Ref { type_arguments, .. } = raw else {
            return None;
        };
        if type_arguments.is_empty() {
            return None;
        }
        let ctx = self.ctx;
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let navigate_context =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let lowered = dispatch.lower_type_expr_in_scope_with_context(
            scope_canonical_id,
            raw,
            navigate_context,
        )?;
        let lowered_data = node_data_for(ctx, lowered)?;
        let SemanticNodeData::InstantiationRef { base, args } = lowered_data.as_ref() else {
            return None;
        };
        if base.canonical_id.as_ref() != scope_canonical_id {
            return None;
        }
        let prepared =
            self.prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())?;
        if prepared.type_parameters.len() < args.len() {
            return None;
        }
        if !matches!(prepared.body, TypeExpr::Object(_)) {
            return None;
        }
        let instantiate_prc =
            ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
        let node = match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base: dispatch.type_slot_for(
                std::sync::Arc::clone(&base.canonical_id),
                std::sync::Arc::clone(&base.decl_name),
            ),
            context: dispatch.instantiate_context_for(&base.canonical_id, instantiate_prc),
            args: std::sync::Arc::clone(args),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => node,
            QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
        };
        if !node_raises_to_object_surface(ctx, node) {
            return None;
        }
        let admitted = AdmittedRouteProjectionNode::new(node);
        let type_expr = super::surface::materialize_route_projection_node(ctx, &admitted)?;
        Some((type_expr, true))
    }

    /// Routed registry MEMBER surface (the per-member arm of a `Pick<…>` / member
    /// path route): project `route_expr` to its surface NODE through the shared
    /// class-A node dispatch, materialise its structure to the first-pass node, then
    /// reduce that node ONCE through the `ShapeCacheDb` member-node stabiliser. The
    /// no-poison selection is decided in the stabiliser on node-domain
    /// `!RaisedShapeFacts.materialized` facts. Returns the chosen value paired with
    /// its object-surface fact, carried untainted.
    pub(crate) fn materialize_registry_routed_member_surface(
        &mut self,
        scope_canonical_id: &str,
        route_expr: &TypeExpr,
    ) -> RegistryMemberSurface {
        let ctx = self.ctx;
        let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            Some(self),
            scope_canonical_id,
            route_expr,
        );
        let base_node = match projected {
            Some(admitted) => Some(admitted.node()),
            None => {
                let dispatch = ProjectSemanticDispatch::new(ctx);
                dispatch.lower_type_expr_in_scope_with_mode(
                    scope_canonical_id,
                    route_expr,
                    ProjectionMode::Navigate,
                )
            }
        };
        let first_node = base_node.and_then(|base| {
            self.materialize_member_surface_to_node(scope_canonical_id, base, true)
        });
        let Some(first_node) = first_node else {
            // Neither projection nor a raw lowering yields a base, or the structure
            // materialisation errored: best-effort single-pass route surface.
            return self.materialize_registry_member_value(scope_canonical_id, route_expr);
        };
        let stabilized = crate::meta_resolve::materialize::stabilize_registry_member_surface_node_with_shape_cache(
            ctx,
            scope_canonical_id,
            first_node,
            ProjectionMode::Navigate,
        );
        self.registry_member_surface_from_stabilized(stabilized)
    }

    /// Unwrap a [`crate::meta_resolve::materialize::RegistryMemberStabilizedValue`]
    /// into the published value + its object-surface fact: raise the chosen NODE
    /// (the first-pass node for `First`, the stabilised node for `Stable`) ONCE at
    /// the registered terminal sink, and read the object-surface fact off that SAME
    /// node — no decision rides the raised value.
    fn registry_member_surface_from_stabilized(
        &self,
        stabilized: crate::meta_resolve::materialize::RegistryMemberStabilizedValue,
    ) -> RegistryMemberSurface {
        use crate::meta_resolve::materialize::RegistryMemberStabilizedValue;
        let ctx = self.ctx;
        let node = match stabilized {
            RegistryMemberStabilizedValue::First { node }
            | RegistryMemberStabilizedValue::Stable { node } => node,
        };
        let value = materialize_member_node_to_type_expr(ctx, node).unwrap_or_else(|| {
            TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                properties: Vec::new(),
            }))
        });
        let explicit_object_surface =
            component_meta_registry_node_has_explicit_object_surface(ctx, node);
        RegistryMemberSurface {
            value,
            explicit_object_surface,
        }
    }

    /// Single-pass registry member value: lower `expr` (Navigate), materialise its
    /// structure to the first-pass node, raise it, and pair it with its
    /// object-surface fact (off the first-pass node). The Pick callable-descent-skip
    /// path projects a package-backed raw leaf directly through this (no route
    /// re-projection / stabilisation), and the routed sibling reuses it as the
    /// best-effort fallback.
    pub(crate) fn materialize_registry_member_value(
        &mut self,
        scope_canonical_id: &str,
        expr: &TypeExpr,
    ) -> RegistryMemberSurface {
        let ctx = self.ctx;
        let base = {
            let dispatch = ProjectSemanticDispatch::new(ctx);
            dispatch.lower_type_expr_in_scope_with_mode(
                scope_canonical_id,
                expr,
                ProjectionMode::Navigate,
            )
        };
        match base
            .and_then(|b| self.materialize_member_surface_to_node(scope_canonical_id, b, true))
        {
            Some(first_node) => {
                let value = materialize_member_node_to_type_expr(ctx, first_node)
                    .unwrap_or_else(|| expr.clone());
                let explicit_object_surface =
                    component_meta_registry_node_has_explicit_object_surface(ctx, first_node);
                RegistryMemberSurface {
                    value,
                    explicit_object_surface,
                }
            }
            None => RegistryMemberSurface {
                value: expr.clone(),
                explicit_object_surface: false,
            },
        }
    }

    /// Builtin-Pick registry candidate fallback through the shared `Pick<base, keys>`
    /// dispatch (the same single-dispatch path as [`Self::materialize_pick_member_surface`]):
    /// resolve the Pick result node, materialise its structure to the first-pass
    /// node, raise it, and pair it with the producing node's object-surface fact.
    pub(crate) fn materialize_pick_member_surface_candidate(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        members: &[String],
    ) -> Option<RegistryMemberSurface> {
        let pick_node = {
            let dispatch = ProjectSemanticDispatch::new(self.ctx);
            let symbol_ref = TypeExpr::Ref {
                name: std::sync::Arc::from(root_symbol),
                type_arguments: std::sync::Arc::from(Vec::new().into_boxed_slice()),
            };
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
        let first_node =
            self.materialize_member_surface_to_node(scope_canonical_id, pick_node, true)?;
        let value = materialize_member_node_to_type_expr(self.ctx, first_node)?;
        let explicit_object_surface =
            component_meta_registry_node_has_explicit_object_surface(self.ctx, first_node);
        Some(RegistryMemberSurface {
            value,
            explicit_object_surface,
        })
    }

    /// Project a member-path route expression to its leaf value PLUS the node-domain
    /// reject/accept facts the registry member-path arm decides on
    /// (`explicit_object_surface` / `non_object_top_level_surface` /
    /// `is_indexed_access_shell`). The leaf is the former
    /// `project_expr_class_a_via_dispatch(...).unwrap_or(route_expr)`; the three
    /// facts replace the host-side `matches!` / `has_*_surface` decisions on the
    /// materialised leaf, read off the leaf's projected node (no re-lower).
    pub(crate) fn project_member_path_leaf_facts(
        &mut self,
        scope_canonical_id: &str,
        route_expr: &TypeExpr,
    ) -> (TypeExpr, bool, bool, bool) {
        let ctx = self.ctx;
        // The member-path leaf mirrors the NON-threaded `project_expr_class_a_via_dispatch`
        // (engine = None / transient), so the node projection threads no engine.
        let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
            ctx,
            None,
            scope_canonical_id,
            route_expr,
        );
        match projected {
            Some(admitted) => {
                let node = admitted.node();
                let is_object = component_meta_registry_node_has_explicit_object_surface(ctx, node);
                let non_object_top =
                    component_meta_registry_node_has_non_object_top_level_surface(ctx, node);
                let is_indexed = node_is_indexed_access_shell(ctx, node);
                let leaf = super::surface::materialize_route_projection_node(ctx, &admitted)
                    .unwrap_or_else(|| route_expr.clone());
                (leaf, is_object, non_object_top, is_indexed)
            }
            None => {
                // Projection failed: the leaf IS the raw `route_expr` (the original
                // `.unwrap_or(route_expr)`). Lower it (symbolic-input pipeline feed)
                // for facts.
                let dispatch = ProjectSemanticDispatch::new(ctx);
                let leaf_node = dispatch.lower_type_expr_in_scope_with_mode(
                    scope_canonical_id,
                    route_expr,
                    ProjectionMode::Navigate,
                );
                let (is_object, non_object_top, is_indexed) = leaf_node
                    .map(|node| {
                        (
                            component_meta_registry_node_has_explicit_object_surface(ctx, node),
                            component_meta_registry_node_has_non_object_top_level_surface(
                                ctx, node,
                            ),
                            node_is_indexed_access_shell(ctx, node),
                        )
                    })
                    .unwrap_or((false, false, false));
                (route_expr.clone(), is_object, non_object_top, is_indexed)
            }
        }
    }

    /// Refine an imported generic-alias Object surface member-by-member: the
    /// relocated body of the former host-side `maybe_refine_imported_generic_alias_object`
    /// closure. Each property re-projects `Ref{symbol}["<prop>"]` through the shared
    /// class-A node dispatch in the OWNER scope (keeping only a node-materialised
    /// projection with no semantic miss), raises it so the alias body's helper
    /// carriers re-resolve in the DEFINING scope, then materialises + stabilises it
    /// (node-domain no-poison). Returns the refined Object (a transformer — `source`
    /// is returned unchanged when it is not an Object).
    pub(crate) fn refine_imported_generic_alias_object_surface(
        &mut self,
        owner_scope: &str,
        materialize_scope: &str,
        symbol_name: &str,
        source: &TypeExpr,
    ) -> TypeExpr {
        use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty};
        let TypeExpr::Object(object) = source else {
            return source.clone();
        };
        let ctx = self.ctx;
        let mut properties: Vec<ObjectMember> = Vec::with_capacity(object.properties.len());
        for member in object.properties.iter() {
            let ObjectMember::Property(property) = member else {
                properties.push(member.clone());
                continue;
            };
            let route_expr =
                registry_indexed_access_expr(symbol_name, std::slice::from_ref(&property.name));
            // OWNER-scope projection NODE, kept only when it carries no semantic
            // miss (node fact, mirroring the former `.filter(!contains_semantic_miss)`).
            let projected = crate::meta_resolve::project_expr_class_a_node_via_dispatch_threaded(
                ctx,
                Some(self),
                owner_scope,
                &route_expr,
            );
            let refine_input: TypeExpr = match projected {
                Some(admitted) => {
                    let node = admitted.node();
                    let has_miss = node_raised_shape_facts_with_dispatch(
                        &ProjectSemanticDispatch::new(ctx),
                        node,
                    )
                    .map(|facts| !facts.materialized)
                    .unwrap_or(false);
                    if has_miss {
                        property.ty.clone()
                    } else {
                        // Raise the owner-scope projection (= the former
                        // `project_class_a` `typed_projected`) through the registered
                        // sink so the defining-scope pass re-resolves the alias body's
                        // helper carriers.
                        materialize_member_node_to_type_expr(ctx, node)
                            .unwrap_or_else(|| property.ty.clone())
                    }
                }
                None => property.ty.clone(),
            };
            // Pass 1 + pass 2 in the DEFINING (materialize) scope — the remaining
            // carriers are the alias body's helper references, which resolve there.
            let base = {
                let dispatch = ProjectSemanticDispatch::new(ctx);
                dispatch.lower_type_expr_in_scope_with_mode(
                    materialize_scope,
                    &refine_input,
                    ProjectionMode::Navigate,
                )
            };
            let ty = match base
                .and_then(|b| self.materialize_member_surface_to_node(materialize_scope, b, true))
            {
                Some(first_node) => {
                    let stabilized =
                        crate::meta_resolve::materialize::stabilize_registry_member_surface_node_with_shape_cache(
                            ctx,
                            materialize_scope,
                            first_node,
                            ProjectionMode::Navigate,
                        );
                    self.registry_member_surface_from_stabilized(stabilized)
                        .value
                }
                None => refine_input,
            };
            properties.push(ObjectMember::Property(ObjectProperty::with_visibility(
                property.name.clone(),
                ty,
                property.optional,
                property.readonly,
                property.visibility,
                property.spans,
            )));
        }
        TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
    }
}

/// Module-private registry member-surface carrier: a materialised member value
/// PAIRED with its node-domain object-surface fact (decided off the producing
/// node). The ONLY value-bearing carrier crossing toward `host_manage` — no raw
/// `SemanticNodeId` / `AdmittedRouteProjectionNode` leaves the query engine.
pub(crate) struct RegistryMemberSurface {
    pub(crate) value: TypeExpr,
    pub(crate) explicit_object_surface: bool,
}

/// Build the registry indexed-access route expression `symbol['p0']['p1']…` from a
/// member-name path — the module-local node-engine copy of the host-side
/// `build_registry_indexed_access_expr` (pure `TypeExpr` construction).
fn registry_indexed_access_expr(symbol_name: &str, path: &[String]) -> TypeExpr {
    path.iter()
        .fold(TypeExpr::named(symbol_name), |object, member| {
            TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(object),
                index: std::sync::Arc::new(TypeExpr::string_literal(member.clone())),
            }
        })
}

/// Node-domain mirror of `component_meta_registry_has_explicit_object_surface`
/// applied to a node's RAISED root: an `Object` / `MergedDecl` / `VueMacroElements`
/// (all raise to `TypeExpr::Object`), or a `Union` / `Intersection` carrying such
/// an arm, following the `Alias` identity hop. Read off the producing node so a
/// candidate's object-surface fact is decided in node domain instead of inspecting
/// the materialised `TypeExpr`.
pub(crate) fn component_meta_registry_node_has_explicit_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    fn walk(
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
        depth: u32,
    ) -> bool {
        const MAX_DEPTH: u32 = 32;
        if depth >= MAX_DEPTH {
            return false;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Object(_)
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::VueMacroElements(_) => true,
            SemanticNodeData::Alias(inner) => walk(ctx, *inner, depth + 1),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                arms.iter().any(|arm| walk(ctx, *arm, depth + 1))
            }
            _ => false,
        }
    }
    walk(ctx, node, 0)
}

/// Whether `node` raises EXACTLY to a `TypeExpr::Object` (an `Object` /
/// `MergedDecl` / `VueMacroElements`, following the `Alias` hop) — the node-domain
/// mirror of `matches!(raise(node), TypeExpr::Object(_))`. Unlike
/// [`component_meta_registry_node_has_explicit_object_surface`], a `Union` /
/// `Intersection` (which raises to a `Union` / `Intersection`, not a plain `Object`)
/// is NOT an object root here.
pub(crate) fn node_raises_to_object_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    fn walk(
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
        depth: u32,
    ) -> bool {
        const MAX_DEPTH: u32 = 32;
        if depth >= MAX_DEPTH {
            return false;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Object(_)
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::VueMacroElements(_) => true,
            SemanticNodeData::Alias(inner) => walk(ctx, *inner, depth + 1),
            _ => false,
        }
    }
    walk(ctx, node, 0)
}

/// Node-domain mirror of `component_meta_registry_has_non_object_top_level_surface`
/// applied to a node's RAISED root: a `Ref`-carrier (`DeclRef` / `InstantiationRef`
/// / `BareRef`, raising to `TypeExpr::Ref`) / `IndexedAccess` / `Conditional` /
/// `Mapped`, OR a `Union` / `Intersection` where any arm recursively qualifies or
/// any arm does NOT raise to a plain `Object`. Arm-for-arm with the `TypeExpr`
/// predicate (`KeyOf` / `TypeOf` and primitives/objects are NOT non-object roots).
pub(crate) fn component_meta_registry_node_has_non_object_top_level_surface(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    fn walk(
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
        depth: u32,
    ) -> bool {
        const MAX_DEPTH: u32 = 32;
        if depth >= MAX_DEPTH {
            return false;
        }
        let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Alias(inner) => walk(ctx, *inner, depth + 1),
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                arms.iter().any(|arm| walk(ctx, *arm, depth + 1))
                    || arms
                        .iter()
                        .any(|arm| !node_raises_to_object_surface(ctx, *arm))
            }
            SemanticNodeData::DeclRef { .. }
            | SemanticNodeData::InstantiationRef { .. }
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::Conditional { .. }
            | SemanticNodeData::Mapped { .. } => true,
            _ => false,
        }
    }
    walk(ctx, node, 0)
}

/// Whether `node`'s top-level data is a deferred `IndexedAccess` shell — the
/// node-domain mirror of `matches!(leaf, TypeExpr::IndexedAccess { .. })`.
pub(crate) fn node_is_indexed_access_shell(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    use crate::semantic_query::SemanticNodeData;
    crate::project_semantic_dispatch::node_data_for(ctx, node)
        .is_some_and(|data| matches!(data.as_ref(), SemanticNodeData::IndexedAccess { .. }))
}

/// Raise a member-surface NODE to its published `TypeExpr` through the registered
/// `materialize_route_projection_node` terminal sink — the materialisation happens
/// INSIDE the sink (`materialize_published_node`), so the registry siblings hold no
/// `into_type_expr` / `materialize_output_type_expr` mint of their own. The object
/// fact is read off the node separately, so no semantic decision rides the raised
/// value.
fn materialize_member_node_to_type_expr(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    super::surface::materialize_route_projection_node(ctx, &AdmittedRouteProjectionNode::new(node))
}

#[cfg(test)]
mod node_predicate_parity_tests {
    use super::{
        component_meta_registry_node_has_explicit_object_surface,
        component_meta_registry_node_has_non_object_top_level_surface,
        materialize_member_node_to_type_expr, node_raises_to_object_surface,
    };
    use crate::meta::MetaProject;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::component_meta_registry::{
        component_meta_registry_has_explicit_object_surface,
        component_meta_registry_has_non_object_top_level_surface,
    };
    use crate::semantic_query::ProjectionMode;
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::VerterHost;
    use std::sync::Arc as StdArc;
    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

    fn one_prop_object(name: &str) -> TypeExpr {
        TypeExpr::Object(StdArc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                name.to_string(),
                TypeExpr::string_literal("x"),
                false,
                false,
            ))],
        }))
    }

    fn open_host() -> StdArc<MetaProject> {
        let host = VerterHost::new_standalone(HostConfig {
            analysis_level: AnalysisLevel::Full,
            ..HostConfig::default()
        });
        let project = MetaProject::new(host);
        project
            .upsert_base("/p.ts", "export type Anchor = number\n")
            .unwrap();
        project
    }

    /// PARITY: the node-domain registry object-surface predicates answer IDENTICALLY
    /// to the `TypeExpr` predicates applied to the node's raised value (the exact
    /// value the host-side registry loop publishes). A node-fact MUTATION breaks this:
    /// dropping the `Union`/`Intersection` arm from
    /// `component_meta_registry_node_has_explicit_object_surface`, or adding `KeyOf` to
    /// `component_meta_registry_node_has_non_object_top_level_surface` (which the
    /// `TypeExpr` predicate does NOT have), flips a `Union`/operator case below.
    #[test]
    fn registry_object_surface_node_predicates_mirror_type_expr_predicates_on_raised_value() {
        let project = open_host();
        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/p.ts").unwrap();
        let host = session.host();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let dispatch = ProjectSemanticDispatch::new(ctx);

        let cases: Vec<TypeExpr> = vec![
            // Object surface.
            one_prop_object("a"),
            // Union carrying an object arm AND a non-object arm.
            TypeExpr::Union(StdArc::from(vec![
                one_prop_object("a"),
                TypeExpr::string_literal("lit"),
            ])),
            // Intersection of two object arms (still an object surface).
            TypeExpr::Intersection(StdArc::from(vec![
                one_prop_object("a"),
                one_prop_object("b"),
            ])),
            // A bare literal (non-object, non-ref).
            TypeExpr::string_literal("solo"),
            // A bare reference carrier to a missing name (raises to `Ref`).
            TypeExpr::Ref {
                name: StdArc::from("DefinitelyMissingType"),
                type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
            },
        ];

        for expr in &cases {
            let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
                "/p.ts",
                expr,
                ProjectionMode::Navigate,
            ) else {
                continue;
            };
            let Some(raised) = materialize_member_node_to_type_expr(ctx, node) else {
                continue;
            };
            assert_eq!(
                component_meta_registry_node_has_explicit_object_surface(ctx, node),
                component_meta_registry_has_explicit_object_surface(&raised),
                "explicit-object-surface NODE predicate must mirror the TypeExpr predicate on the \
                 raised value for {expr:?} (raised={raised:?})",
            );
            assert_eq!(
                component_meta_registry_node_has_non_object_top_level_surface(ctx, node),
                component_meta_registry_has_non_object_top_level_surface(&raised),
                "non-object-top-level NODE predicate must mirror the TypeExpr predicate on the \
                 raised value for {expr:?} (raised={raised:?})",
            );
        }
    }

    /// DISCRIMINATION: a `Union[Object, literal]` node exercises BOTH arms of the
    /// node predicates — the object arm (`explicit_object_surface == true`) and the
    /// non-object arm (`non_object_top_level == true`) — while
    /// `node_raises_to_object_surface` is FALSE on the `Union` root (a `Union` does
    /// not raise to a plain `Object`, the property that gates the owner-local arm). An
    /// inline `Object` root inverts the non-object arm and IS an object-raising root.
    /// Removing any arm flips one of these assertions.
    #[test]
    fn registry_node_predicates_discriminate_union_and_object_roots() {
        let project = open_host();
        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/p.ts").unwrap();
        let host = session.host();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let dispatch = ProjectSemanticDispatch::new(ctx);

        let union = TypeExpr::Union(StdArc::from(vec![
            one_prop_object("a"),
            TypeExpr::string_literal("lit"),
        ]));
        let union_node = dispatch
            .lower_type_expr_in_scope_with_mode("/p.ts", &union, ProjectionMode::Navigate)
            .expect("union lowers");
        assert!(
            component_meta_registry_node_has_explicit_object_surface(ctx, union_node),
            "a Union with an Object arm IS an explicit object surface",
        );
        assert!(
            component_meta_registry_node_has_non_object_top_level_surface(ctx, union_node),
            "a Union with a non-object arm HAS a non-object top-level surface",
        );
        assert!(
            !node_raises_to_object_surface(ctx, union_node),
            "a Union root does NOT raise to a plain Object",
        );

        let object_node = dispatch
            .lower_type_expr_in_scope_with_mode(
                "/p.ts",
                &one_prop_object("a"),
                ProjectionMode::Navigate,
            )
            .expect("object lowers");
        assert!(component_meta_registry_node_has_explicit_object_surface(
            ctx,
            object_node
        ));
        assert!(!component_meta_registry_node_has_non_object_top_level_surface(ctx, object_node));
        assert!(
            node_raises_to_object_surface(ctx, object_node),
            "an inline Object root DOES raise to a plain Object",
        );
    }

    /// PARITY (§6 published-operator-root trap): the node-domain second-pass
    /// reduction context must EQUAL the `TypeExpr`-start context the former
    /// `materialize_component_meta_type_expr_until_stable` computed on the SAME
    /// surface (the node's raised value). A `node_root_is_published_operator`
    /// mis-classification (e.g. treating a `Union`/`Object` root as a published
    /// operator, or missing the `Ref`/`Mapped`/`IndexedAccess` carriers) silently
    /// flips `StructuralTransit(Navigate)` ↔ `Published(Navigate)` and is caught here.
    #[test]
    fn node_reduction_context_mirrors_type_expr_reduction_context_on_raised_value() {
        use crate::meta_resolve::materialize::{
            node_materialize_reduction_context, type_expr_materialize_reduction_context,
        };

        let project = open_host();
        let session = project.open_session_batch().unwrap();
        let _ = session.evaluate_types("/p.ts").unwrap();
        let host = session.host();
        let ctx: &dyn crate::resolver_core::ResolverContext = host;
        let dispatch = ProjectSemanticDispatch::new(ctx);

        let cases: Vec<TypeExpr> = vec![
            one_prop_object("a"),
            TypeExpr::Union(StdArc::from(vec![
                one_prop_object("a"),
                TypeExpr::string_literal("lit"),
            ])),
            TypeExpr::string_literal("solo"),
            TypeExpr::Ref {
                name: StdArc::from("DefinitelyMissingType"),
                type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
            },
        ];

        for expr in &cases {
            let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
                "/p.ts",
                expr,
                ProjectionMode::Navigate,
            ) else {
                continue;
            };
            let Some(raised) = materialize_member_node_to_type_expr(ctx, node) else {
                continue;
            };
            assert_eq!(
                node_materialize_reduction_context(ctx, node, ProjectionMode::Navigate),
                type_expr_materialize_reduction_context(&raised, ProjectionMode::Navigate),
                "node reduction context must mirror the TypeExpr reduction context on the raised \
                 value for {expr:?} (raised={raised:?})",
            );
        }
    }
}
