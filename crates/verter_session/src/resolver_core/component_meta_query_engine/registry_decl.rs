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
//!   `pub fn allow_wildcard_route`,
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
//!   `pub(crate) fn dispatch_routed_expr_surface_expr` — crate-visible
//!   helpers used by `meta_resolve` and other engine impl methods.
//! - Private methods (`semantic_dispatch`, `dispatch_root_instantiated`)
//!   stay private and are visible inside the
//!   `component_meta_query_engine` folder via parent-private locality.

use verter_semantic::analysis::type_solver::query_engine::ProjectedSurface;
use verter_type_expr::TypeExpr;

use super::helpers::{
    is_builtin_name, resolve_imported_registry_symbol_with_budget, ImportedRegistrySymbolResolution,
};
use super::surface::{
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
use crate::resolver_core::{FuseTrip, RouteDemand};
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
        use crate::component_meta_materialize::{
            materialize_component_meta_structure, MaterializationScope, MaterializeOutcome,
            MaterializeRuntimeKey,
        };
        use crate::project_semantic_dispatch::ProjectSemanticDispatch;
        use crate::semantic_query::ProjectionMode;

        let dispatch = ProjectSemanticDispatch::new(self.ctx);
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
        let materialised_id = match read.value {
            MaterializeOutcome::Value(id)
            | MaterializeOutcome::Miss(id)
            | MaterializeOutcome::Recursive(id)
            | MaterializeOutcome::Tainted(id) => id,
            MaterializeOutcome::Error(_) => return None,
        };
        // Publication sink: materialize into a sealed carrier and unwrap via
        // the query-engine output capability.
        let cap = MetaQueryRegistryOutputCap::new(&dispatch);
        cap.materialize_output_type_expr(materialised_id)
            .map(|raised| raised.into_type_expr(&cap))
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

    pub fn enter_member_surface(&mut self) -> bool {
        self.fuse_state.push_member_recursion();
        !self
            .fuse_state
            .check_member_recursion_depth(&self.fuse_budgets)
    }

    pub fn exit_member_surface(&mut self) {
        self.fuse_state.pop_member_recursion();
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

    /// Pre-consume the wildcard-route fuse so exactly `remaining`
    /// further `allow_wildcard_route()` calls stay within budget. With
    /// `remaining == 1`, the next slow-lane resolution is permitted and
    /// a second would trip `wildcard_route_fanout` — the near-fanout
    /// boundary that discriminates the imported-registry recompute bug.
    #[cfg(test)]
    pub(crate) fn prime_wildcard_route_fuse_for_tests(&mut self, remaining: usize) {
        self.fuse_state.wildcard_sources_processed = self
            .fuse_budgets
            .wildcard_route_fanout
            .saturating_sub(remaining);
    }

    /// Number of `allow_wildcard_route()` calls observed so far — the
    /// live wildcard-route fuse consumption count.
    #[cfg(test)]
    pub(crate) fn wildcard_route_fuse_consumed_for_tests(&self) -> usize {
        self.fuse_state.wildcard_sources_processed
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
    /// assertions / ctx `materialize_structure_db().live_count()` checks.
    /// Field + accessor retained until the broader counter cleanup.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_prepared_type_decl_query_count(&self) -> usize {
        self.prepared_type_decl_query_count
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

        // (1) Registry public-indexed-access / public-utility route.
        if let Some((root_symbol, route)) =
            component_meta_registry_public_indexed_access_route(expr)
                .or_else(|| component_meta_registry_public_utility_route(expr))
        {
            if let Some(projected) =
                self.dispatch_routed_expr_surface_expr(scope_canonical_id, &root_symbol, &route)
            {
                return Some(
                    verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected),
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
}
