//! Private source-request cache coordination. Storage retains only owned facts.

use crate::resolver_core::imported_root_db::{
    ImportedRootDb, ImportedRootFlightOutcome, ImportedRootResult,
};
use crate::resolver_core::route_db::{
    emit_export_route_resolved_event, RouteDb, RouteFlightOutcome,
};
use crate::resolver_core::{
    ResolverContext, RouteNameKey, RouteResult, SingleflightRole, SingleflightRunResult,
};
use std::sync::Arc;
use verter_session_query::facts::fact_cache::FactVersionRef;
use verter_session_query::facts::store_view::StoreView;

pub(crate) struct ImportedRootRequestDriver<'a> {
    db: &'a ImportedRootDb,
}
impl<'a> ImportedRootRequestDriver<'a> {
    pub(crate) fn new(db: &'a ImportedRootDb) -> Self {
        Self { db }
    }
    pub(crate) fn get_or_resolve_returning_facts_with_context<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        ctx: &dyn ResolverContext,
        resolve: F,
    ) -> Option<(Arc<ImportedRootResult>, Arc<[FactVersionRef]>)>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            |probe| {
                self.get_or_resolve_returning_facts_in_scope(
                    provider_canonical,
                    imported_name,
                    view,
                    probe,
                    resolve,
                )
            },
        )
        .0
    }

    fn get_or_resolve_returning_facts_in_scope<V, F>(
        &self,
        provider_canonical: &str,
        imported_name: &str,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<(Arc<ImportedRootResult>, Arc<[FactVersionRef]>)>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        let key = (provider_canonical.to_owned(), imported_name.to_owned());

        if let Some(hit) = self.db.roots.get_if_valid_with_facts(&key, view) {
            // Per-request audit attribution: warm-cache hit. The
            // closure body did not run — this is a true reuse.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRootWarm);
            }
            return Some(hit);
        }

        let run_result = self.resolve_root_singleflight_inner(key, view, probe, resolve)?;
        Some((
            Arc::clone(&run_result.value.root),
            Arc::clone(&run_result.value.facts),
        ))
    }

    /// Shared singleflight orchestrator for the cold-path root resolve.
    ///
    /// Retention MIRRORS admission (the same bounded re-validation loop the
    /// route lane uses): an ADMITTED outcome is retained as a joinable
    /// rendezvous for the burst; an UNADMITTED outcome serves only the LEADER,
    /// while a FOLLOWER re-runs `resolve` against fresh state on a fresh lane.
    ///
    /// EVERY unadmitted outcome — leader-produced or follower-adopted — marks
    /// the non-cacheability rail of the thread it is served to. Both refusal
    /// reasons need it, for different halves of the same hazard:
    ///
    /// - `probe.non_cacheable()` (fenced serve / broken lease / unrootable
    ///   route): the reads that set it already fanned out to every tracer on
    ///   the LEADER's stack, so the leader's re-mark is a harmless no-op — but
    ///   an ADOPTING FOLLOWER never ran that walk, and nothing has marked its
    ///   tracers.
    /// - `facts.is_empty()`: the RESULT is unrootable and NO non-cacheable read
    ///   need have occurred at all, so NEITHER thread is marked. An empty
    ///   signature also FANS NOTHING, so an enclosing traced compute observes
    ///   no fact for the root, warm-admits a result folding a root it cannot
    ///   root, and revalidates against the live view forever.
    ///
    /// Marking on `!admitted` — rather than per reason — is the structural
    /// floor: no unadmitted value leaves this funnel without marking the thread
    /// that receives it, whatever refused it and whichever producer supplied
    /// it. The mark is cache non-admission only, never request partiality: the
    /// value served is VALID (Complete).
    fn resolve_root_singleflight_inner<V, F>(
        &self,
        key: (String, String),
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<crate::resolver_core::SingleflightRunResult<ImportedRootFlightOutcome>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(ImportedRootResult, Vec<FactVersionRef>)>,
    {
        let flight_body = || {
            if let Some(hit) = self.db.roots.get_if_valid_with_facts(&key, view) {
                return Ok(ImportedRootFlightOutcome {
                    root: hit.0,
                    facts: hit.1,
                    admitted: true,
                });
            }
            // Per-request audit attribution: cold path running the
            // expensive `resolve()` closure. Joiners that block on
            // this singleflight do NOT re-enter — the counter
            // reflects unique cold work, not per-waiter overhead.
            if let Some(obs) = verter_audit::current_observer() {
                obs.record_event(verter_audit::AuditEvent::ImportedRootCold);
            }
            match resolve() {
                Some((result, facts)) => {
                    let arc = Arc::new(result);
                    // Admission is TWO independent gates, both fail-closed:
                    //
                    // - a non-empty fact signature (an empty one has nothing a
                    //   warm read could validate against);
                    // - the cacheability verdict of the scope enclosing this
                    //   resolve, sampled AFTER it ran. A fenced serve, a broken
                    //   decl-body lease, an unrootable route or an unobservable
                    //   contributor source env consumed anywhere in the walk
                    //   means the value's basis cannot be soundly rooted — and
                    //   three of those four are CONTENT-NEUTRAL, so the entry
                    //   would root on the LIVE hash and validate forever.
                    //
                    // The route surface is still returned to the caller either
                    // way; only the persist is refused.
                    let admitted = !facts.is_empty() && !probe.non_cacheable();
                    if admitted {
                        self.db.roots.insert_arc_with_kind(
                            key.clone(),
                            arc.clone(),
                            facts.clone(),
                            "imported_root_db.roots",
                        );
                    }
                    Ok(ImportedRootFlightOutcome {
                        root: arc,
                        facts: Arc::from(facts),
                        admitted,
                    })
                }
                None => Err(()),
            }
        };
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_unadmitted: Option<
            crate::resolver_core::SingleflightRunResult<ImportedRootFlightOutcome>,
        > = None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result = self
                .db
                .singleflight
                .run_retaining(key.clone(), view.compat_token(), flight_body, |outcome| {
                    outcome.admitted
                })
                .ok()?;
            if run_result.value.admitted {
                return Some(run_result);
            }
            if matches!(
                run_result.role,
                crate::resolver_core::SingleflightRole::Leader
            ) {
                // Unadmitted leader: serve its own caller, and carry the
                // non-cacheability onto that caller's rails.
                //
                // The mark is NOT redundant with "the resolve ran on this
                // thread". That reasoning covers only ONE of the two refusal
                // reasons. `admitted = !facts.is_empty() && !probe.non_cacheable()`:
                //
                // - `probe.non_cacheable()` — the walk consumed a fenced serve /
                //   broken lease / unrootable route. Each of those fanned out to
                //   EVERY tracer on this thread's stack at the point of the read,
                //   before the funnel ever sampled the probe. Re-marking here is a
                //   harmless no-op (the rail is a bool).
                // - `facts.is_empty()` — the RESULT is unrootable. NO non-cacheable
                //   read need have occurred: a route walk whose participants yield
                //   neither a whole-hash nor a route-surface hash (an evicted
                //   provider with no resolvable surface) returns a real route under
                //   an EMPTY signature, and an empty signature FANS NOTHING. Without
                //   this mark the enclosing traced compute observes no fact for the
                //   route at all, warm-admits a result folding a route it cannot
                //   root, and revalidates against the live view forever — nothing
                //   moved.
                //
                // Marking on `!admitted` (rather than on the empty-facts reason
                // alone) is the structural floor: no unadmitted value leaves this
                // funnel without marking the thread that receives it, whatever
                // reason refused it and whichever producer supplied it. This is a
                // VALID (Complete) root, NOT a partial result — cache non-admission
                // only, never request partiality.
                crate::fact_tracing::note_non_cacheable_read_fan_out(
                    verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
                );
                return Some(run_result);
            }
            last_unadmitted = Some(run_result);
        }
        if last_unadmitted.is_some() {
            // Sustained-churn bounded fallback (FOLLOWER adoption): the adopted
            // root is unadmitted — derived from a basis that cannot be rooted —
            // and this thread never ran the resolve that produced it. Carry the
            // non-cacheability by hand so an enclosing traced cold compute
            // refuses shared-cache admission of any result folding a root it
            // cannot root. This is a VALID (Complete) adopted root, NOT a
            // partial result — cache non-admission only, never request
            // partiality.
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
        }
        last_unadmitted
    }
}

pub(crate) struct RouteRequestDriver<'a> {
    db: &'a RouteDb,
}
impl<'a> RouteRequestDriver<'a> {
    pub(crate) fn new(db: &'a RouteDb) -> Self {
        Self { db }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn get_or_resolve_route_with_facts_with_context<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        ctx: &dyn ResolverContext,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            |probe| self.get_or_resolve_route_with_facts_in_scope(key, view, probe, resolve),
        )
        .0
    }

    #[cfg(any(test, feature = "test-support"))]
    fn get_or_resolve_route_with_facts_in_scope<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        if let Some(result) = self.db.routes.get_if_valid(&key, view) {
            return Some(result);
        }

        let run_result = self.resolve_route_singleflight_inner(key, view, probe, resolve)?;
        Some(Arc::clone(&run_result.value.route))
    }

    pub(crate) fn get_or_resolve_route_observing_facts_with_context<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        ctx: &dyn ResolverContext,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        crate::fact_signature_helpers::with_cacheability_scope(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            |probe| self.get_or_resolve_route_observing_facts_in_scope(key, view, probe, resolve),
        )
        .0
    }

    fn get_or_resolve_route_observing_facts_in_scope<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<Arc<RouteResult>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        // Warm-hit fast path: validated cache lookup with fact bubbling.
        if let Some((value, facts)) = self.db.get_route_with_facts(&key, view) {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, feature = "test-support"))]
            self.db
                .route_warm_fact_bubble_emissions
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Some(value);
        }

        // Cold path: delegate to the shared singleflight helper, then
        // observe the leader / follower role and bump the matching
        // provenance counter on the post-admission re-read.
        let run_result =
            self.resolve_route_singleflight_inner(key.clone(), view, probe, resolve)?;

        // Post-admission re-read: fan the just-stored facts into the
        // current thread's tracer stack. Leader: the closure ran here
        // and admitted; the re-read finds the freshly-stored entry.
        // Follower: another thread won the singleflight and admitted;
        // this thread's re-read picks up the admitted entry and the
        // bubble fans the leader's facts into this thread's outer
        // tracer scope.
        if let Some((_value, facts)) = self.db.get_route_with_facts(&key, view) {
            crate::fact_signature_helpers::observe_fact_signature(&facts);
            #[cfg(any(test, feature = "test-support"))]
            match run_result.role {
                SingleflightRole::Leader => {
                    self.db
                        .route_cold_fact_bubble_emissions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                SingleflightRole::Follower => {
                    self.db
                        .route_coalesced_fact_bubble_emissions
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        Some(Arc::clone(&run_result.value.route))
    }

    /// Shared singleflight orchestrator for the cold-path route resolve used
    /// by both [`Self::get_or_resolve_route_with_facts`] and
    /// [`Self::get_or_resolve_route_observing_facts`].
    ///
    /// Runs the caller's `resolve` closure under the [`Self::route_singleflight`]
    /// group so concurrent cold lookups for the same `(provider, exported_name)`
    /// key coalesce onto a single materialization. The closure inside the
    /// singleflight first re-checks the validated cache (to absorb races where
    /// another path warmed the entry between the caller's pre-check and
    /// admission), then invokes `resolve()`. On success, the entry is admitted
    /// to [`Self::routes`] under strict admission rules (non-empty fact
    /// signatures only — empty-signature resolves are the never-persisted
    /// carrier: a frontier walk that consumed a fenced (ReturnOnly) serve, or
    /// the negative-cache pattern surfaced from [`Self::get_or_resolve_route`]
    /// — and are returned to the caller without being persisted as a
    /// fact-validated cache hit).
    ///
    /// Retention mirrors admission (the bounded re-validation loop the
    /// IndexedReady and prepared-decl-bundle lanes use): an ADMITTED
    /// outcome is retained as a joinable rendezvous for the burst; an
    /// UNADMITTED outcome serves only the LEADER (ReturnOnly); a FOLLOWER
    /// receives the unadmitted outcome by value and re-runs `resolve`
    /// against fresh state on a fresh lane. Under sustained churn the
    /// bounded fallback adopts the last unadmitted outcome ReturnOnly.
    ///
    /// EVERY unadmitted outcome — leader-produced or follower-adopted —
    /// marks the non-cacheability rail of the thread it is served to.
    /// Both refusal reasons need it, for different halves of the same
    /// hazard:
    ///
    /// - `probe.non_cacheable()` (fenced serve / broken lease /
    ///   unrootable route): the reads that set it already fanned out to
    ///   every tracer on the LEADER's stack, so the leader's re-mark is a
    ///   harmless no-op — but an ADOPTING FOLLOWER never ran that walk,
    ///   and nothing has marked its tracers.
    /// - `facts.is_empty()`: the RESULT is unrootable and NO
    ///   non-cacheable read need have occurred at all, so NEITHER thread
    ///   is marked. `build_named_type_export_route_entry` hand-marks its
    ///   fenced and unrootable-wildcard exits, but its NORMAL exit
    ///   returns whatever the participant walk produced — EMPTY when no
    ///   participant yields a whole-hash or a route-surface hash. An
    ///   empty signature also FANS NOTHING, so an enclosing traced
    ///   compute observes no fact for the route, warm-admits a result
    ///   folding a route it cannot root, and revalidates against the live
    ///   view forever.
    ///
    /// Marking on `!admitted` — rather than per reason — is the
    /// structural floor: no unadmitted value leaves this funnel without
    /// marking the thread that receives it, whatever refused it and
    /// whichever producer supplied it. The producer-side empty-facts
    /// convention is a discipline; this is the floor that does not depend
    /// on a producer remembering it. The mark is cache non-admission
    /// only, never request partiality: the value served is VALID
    /// (Complete).
    ///
    /// Returns `Some(SingleflightRunResult { value, role, .. })` on success
    /// (callers that need to discriminate leader vs follower for provenance
    /// counter bumps inspect `role`), or `None` when the resolve closure
    /// returns `None`.
    fn resolve_route_singleflight_inner<V, F>(
        &self,
        key: RouteNameKey,
        view: &V,
        probe: &crate::fact_signature_helpers::CacheabilityProbe<'_>,
        resolve: F,
    ) -> Option<SingleflightRunResult<RouteFlightOutcome>>
    where
        V: StoreView + ?Sized,
        F: Fn() -> Option<(RouteResult, Vec<FactVersionRef>)>,
    {
        let flight_body = || {
            if let Some(result) = self.db.routes.get_if_valid(&key, view) {
                return Ok(RouteFlightOutcome {
                    route: result,
                    admitted: true,
                });
            }
            match resolve() {
                Some((result, facts)) => {
                    let arc = Arc::new(result);
                    // Admission is TWO independent gates, both fail-closed:
                    //
                    // - a non-empty fact signature (an empty one gives a warm
                    //   read nothing to validate against);
                    // - the cacheability verdict of the scope enclosing this
                    //   resolve, sampled AFTER the walk ran. A fenced serve, a
                    //   broken decl-body lease, an unrootable route or an
                    //   unobservable contributor source env consumed anywhere in
                    //   the walk means the route's basis cannot be soundly
                    //   rooted — and three of those four are CONTENT-NEUTRAL, so
                    //   the entry would root on the LIVE hash and validate on
                    //   every warm read forever. The empty-facts convention is a
                    //   producer-side discipline; this gate is the structural
                    //   floor that does not depend on a producer remembering it.
                    //
                    // The route surface is still returned to the caller either
                    // way; only the persist is refused.
                    let admitted = !facts.is_empty() && !probe.non_cacheable();
                    if admitted {
                        self.db.routes.insert_arc_with_kind(
                            key.clone(),
                            arc.clone(),
                            facts,
                            "route_db.routes",
                        );
                    }
                    // R23 typed event: cold-path route admission.
                    // Fires once per `(provider, exported_name)`
                    // resolution. The `augmented` field is `false`
                    // for the bare-route resolution path; the
                    // post-augmentation-stitched path emits its own
                    // `ExportRouteResolved` with `augmented: true`
                    // when consumers walk its entries.
                    emit_export_route_resolved_event(
                        &key.provider_canonical,
                        &key.exported_name,
                        arc.as_ref(),
                        /* augmented = */ false,
                    );
                    Ok(RouteFlightOutcome {
                        route: arc,
                        admitted,
                    })
                }
                None => Err(()),
            }
        };
        const MAX_FLIGHT_ATTEMPTS: usize = 3;
        let mut last_unadmitted: Option<SingleflightRunResult<RouteFlightOutcome>> = None;
        for _attempt in 0..MAX_FLIGHT_ATTEMPTS {
            let run_result = self
                .db
                .route_singleflight
                .run_retaining(key.clone(), view.compat_token(), flight_body, |outcome| {
                    outcome.admitted
                })
                .ok()?;
            if run_result.value.admitted {
                return Some(run_result);
            }
            if matches!(run_result.role, SingleflightRole::Leader) {
                // Unadmitted leader: serve its own caller, and carry the
                // non-cacheability onto that caller's rails.
                //
                // The mark is NOT redundant with "the resolve ran on this
                // thread". That reasoning covers only ONE of the two refusal
                // reasons. `admitted = !facts.is_empty() && !probe.non_cacheable()`:
                //
                // - `probe.non_cacheable()` — the walk consumed a fenced serve /
                //   broken lease / unrootable route. Each of those fanned out to
                //   EVERY tracer on this thread's stack at the point of the read,
                //   before the funnel ever sampled the probe. Re-marking here is a
                //   harmless no-op (the rail is a bool).
                // - `facts.is_empty()` — the RESULT is unrootable. NO non-cacheable
                //   read need have occurred: `build_named_type_export_route_entry`
                //   marks its fenced and unrootable-wildcard exits by hand, but its
                //   NORMAL exit returns whatever `append_route_participant_fact_versions`
                //   produced — and that is EMPTY when no participant yields either a
                //   whole-hash or a route-surface hash (an evicted provider with no
                //   resolvable surface). An empty signature FANS NOTHING, so the
                //   enclosing traced compute observes no fact for the route at all,
                //   warm-admits a result folding a route it cannot root, and
                //   revalidates against the live view forever — nothing moved.
                //
                // Marking on `!admitted` (rather than on the empty-facts reason
                // alone) is the structural floor: no unadmitted value leaves this
                // funnel without marking the thread that receives it, whatever
                // reason refused it and whichever producer supplied it — the
                // producer-side empty-facts convention is a discipline, this is the
                // floor that does not depend on a producer remembering it. This is a
                // VALID (Complete) route, NOT a partial result — cache non-admission
                // only, never request partiality.
                crate::fact_tracing::note_non_cacheable_read_fan_out(
                    verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
                );
                return Some(run_result);
            }
            last_unadmitted = Some(run_result);
        }
        if last_unadmitted.is_some() {
            // Sustained-churn bounded fallback (FOLLOWER adoption): the
            // adopted route is unadmitted — fenced-derived or unrootable
            // — and this thread never ran the resolve that produced it.
            // Carry the non-cacheability by hand so an enclosing traced
            // cold compute refuses shared-cache admission of any result
            // folding a route it cannot root. This is a VALID (Complete)
            // adopted route, NOT a partial result — cache non-admission
            // only, never request partiality.
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnrootableRoute,
            );
        }
        last_unadmitted
    }
}
