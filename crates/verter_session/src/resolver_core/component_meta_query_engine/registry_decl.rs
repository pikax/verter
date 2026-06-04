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

use super::helpers::{is_builtin_name, resolve_imported_registry_symbol_with_budget};
use super::surface::{
    dispatch_route_expr_is_materialized, projected_compound_root_surface_via_dispatch,
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
    SemanticQueryOutput,
};

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
                resolve_imported_registry_symbol_with_budget(
                    ctx,
                    canonical_id,
                    exported_name,
                    || self.allow_wildcard_route(),
                );
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
        // R6: Instantiate.base is content-free `DeclKey`; the
        // cold build re-sources the live whole_hash from
        // `ensure_indexed_ready`.
        let base = crate::semantic_query::DeclKey {
            canonical_id: std::sync::Arc::from(resolved_root.0.as_str()),
            decl_name: std::sync::Arc::from(resolved_root.1.as_str()),
        };
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base,
            args: empty_semantic_args(),
            // `dispatch_root_instantiated` feeds
            // `projected_surface_from_semantic_node` which reads the
            // root's surface members, call/construct lists, etc. Expanded
            // is required so the surface is interpretable; Navigate
            // would yield the lazy shell with no readable view.
            context: crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
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
        let root = self.dispatch_root_instantiated(scope_canonical_id, symbol_name)?;
        if let Some(surface) = projected_surface_from_semantic_node(self.ctx, root) {
            return Some(surface);
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
        let anchor = self.dispatch_decl_anchor(scope_canonical_id, symbol_name)?;
        projected_compound_root_surface_via_dispatch(self.ctx, anchor)
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
                match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
                    base: root,
                    path: query_path,
                    context: crate::semantic_query::ProjectionReductionContext::published(
                        ProjectionMode::Expanded,
                    ),
                }) {
                    QueryResult::Value(SemanticQueryOutput { value: node, .. }) => dispatch
                        .raise_node_to_type_expr(node)
                        .filter(dispatch_route_expr_is_materialized),
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
                .dispatch_routed_pick_omit_via_shared_engine(
                    scope_canonical_id,
                    root_symbol,
                    crate::semantic_query::DeclKey::builtin("Pick"),
                    members,
                ),
            RouteDemand::Omit(members) if !members.is_empty() => self
                .dispatch_routed_pick_omit_via_shared_engine(
                    scope_canonical_id,
                    root_symbol,
                    crate::semantic_query::DeclKey::builtin("Omit"),
                    members,
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
    fn dispatch_routed_pick_omit_via_shared_engine(
        &mut self,
        scope_canonical_id: &str,
        root_symbol: &str,
        builtin_identity: crate::semantic_query::DeclKey,
        keys: &[String],
    ) -> Option<TypeExpr> {
        // Step A: instantiate the route root to a projectable body. Navigate
        // keeps generic carriers intact (the builtin engine re-projects in the
        // caller's mode), mirroring the materialiser's Step A.
        let body_id = self.dispatch_root_instantiated(scope_canonical_id, root_symbol)?;
        let dispatch = self.semantic_dispatch();
        let keys_node = crate::meta_resolve::build_keys_union_node(dispatch.graph(), keys);
        // Step B: instantiate the shared builtin Pick/Omit carrier on
        // `[body, keys]` in the publication Expanded mode — the same path as a
        // userland `Pick<…>` / `Omit<…>`, so fix-#1's public gate applies.
        match dispatch.execute_type_node(SemanticQueryKey::Instantiate {
            base: builtin_identity,
            args: std::sync::Arc::from(vec![body_id, keys_node].into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                ProjectionMode::Expanded,
            ),
        }) {
            QueryResult::Value(SemanticQueryOutput { value: node, .. }) => dispatch
                .raise_node_to_type_expr(node)
                .filter(dispatch_route_expr_is_materialized),
            _ => None,
        }
    }
}
