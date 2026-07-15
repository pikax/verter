use rustc_hash::FxHashSet;

use crate::fact_signature_helpers::CacheabilityProbe;
use crate::request_context::ColdComputeCompletenessScope;
use crate::resolver_core::{
    fallthrough_cache_key, run_stable_request, FallthroughNodeKey, FallthroughPropOverrideSet,
    RequestRunResult, RequestSource, ResolverContext, SingleflightGroup, StableExecutionValue,
    StableRequestExecutor, StoreView,
};
use crate::semantic_query::ResultCompleteness;

/// Owner-minted evidence that a stable fallthrough result was computed inside
/// the request driver's cacheability scope. Only the request driver can
/// construct it, so a producer cannot compute first and open an empty tracer
/// only at the store.
pub(crate) struct FallthroughStableAdmission<'t> {
    probe: &'t CacheabilityProbe<'t>,
}

impl FallthroughStableAdmission<'_> {
    #[inline]
    pub(crate) fn non_cacheable(&self) -> bool {
        self.probe.non_cacheable()
    }

    #[cfg(test)]
    pub(crate) fn from_test_scope<'t>(
        probe: &'t CacheabilityProbe<'t>,
    ) -> FallthroughStableAdmission<'t> {
        FallthroughStableAdmission { probe }
    }
}

pub(crate) trait FallthroughRequestHost {
    type View: StoreView + Clone;
    type Resolution: Clone;

    /// Host whose fact-tracer stack owns this request. The request driver opens
    /// the outermost cacheability scope itself; callers never supply a probe.
    fn cacheability_context(&self) -> &dyn ResolverContext;

    fn generic_root_propagation(&self) -> bool;
    /// Snapshot a base store view together with the manager's currentness
    /// proof: `true` iff the underlying `StoreViewManager` proved the view
    /// current (`StoreViewRead::Current`) at handoff, `false` for a
    /// known-stale `StoreViewRead::ReturnOnly` snapshot taken under
    /// sustained churn. The executor refuses to serve a warm preflight hit
    /// against a non-current snapshot.
    ///
    /// REQUIRED (no default): the `true` arm is a soundness claim — a
    /// defaulted `true` would let a host that owns a churn-prone manager
    /// but forgets the override silently launder every snapshot as
    /// proven-current. Hosts that own no manager and never churn
    /// (test/stub hosts) state that explicitly with
    /// `(self.snapshot_store_view(), true)`.
    fn snapshot_store_view_read(&self) -> (Self::View, bool);
    #[cfg(test)]
    fn snapshot_store_view(&self) -> Self::View;
    /// Live `u64` fold of the host's EXTERNAL-supersession token
    /// dimensions (epoch / project-generation / env-hash / identity /
    /// overlay — the set
    /// `StoreViewValidationToken::externally_superseded_by` compares,
    /// EXCLUDING the compute's own artifact / load
    /// generations). Captured at snapshot time and re-read at
    /// stable-promotion time; a mismatch means an external mutation that
    /// `store_view_epoch` alone does NOT track (e.g. a
    /// `set_default_resolve_extensions` env-hash shift) advanced
    /// mid-compute, so the result MUST NOT be promoted.
    fn current_view_supersession_fingerprint(&self) -> u64;
    fn try_get_cached_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FallthroughPropOverrideSet>,
        store_view: &Self::View,
    ) -> Option<Self::Resolution>;
    /// Run the cold fallthrough compute against `store_view`.
    ///
    /// `base_is_current` is the manager's currentness proof for the
    /// snapshot the executor took (`StableRequestExecutor::
    /// snapshot_view_is_current`). The implementor MUST thread it into the
    /// request-bound resolver context (via `from_cold_seed`) so that, on a
    /// non-current seed, the fallthrough resolver's per-element /
    /// per-child / per-root node-cache validation (which reads through
    /// `ctx.store_view()`) MISSES rather than consuming a stale warm hit.
    /// The fenced cold builder still computes from the seed (the outer
    /// `is_stable` / publish fence rejects promotion of a non-current
    /// result); only its nested probes fail closed.
    fn compute_fallthrough_surface_uncached(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FallthroughPropOverrideSet>,
        visiting: &mut FxHashSet<String>,
        store_view: &Self::View,
        base_is_current: bool,
    ) -> Option<Self::Resolution>;
    /// Admit `result` into the shared fallthrough caches.
    ///
    /// `admission` is minted only by [`run_fallthrough_request`] after its
    /// owner-opened scope has enclosed the complete request compute.
    fn store_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FallthroughPropOverrideSet>,
        result: &Self::Resolution,
        admission: &FallthroughStableAdmission<'_>,
    );
}

struct FallthroughRequestExecutor<'a, 'b, 'p, H: FallthroughRequestHost> {
    host: &'a H,
    canonical_id: String,
    prop_type_overrides: Option<&'a FallthroughPropOverrideSet>,
    visiting: &'b mut FxHashSet<String>,
    /// The request OWNER's cacheability probe. Its scope encloses every
    /// `compute` attempt
    /// this executor drives — and `store_stable` samples it AFTER the compute
    /// it is admitting. That ordering is the whole rail: a scope that started
    /// at the store site would never see the compute's fenced serve / broken
    /// lease / unrootable route.
    probe: &'p CacheabilityProbe<'p>,
    fixed_store_view: Option<H::View>,
    /// EXTERNAL-supersession fingerprint of the snapshot the current
    /// attempt was taken under. Gates stable promotion on the COMPLETE
    /// external-token dimensions (epoch / project-generation / env /
    /// identity / overlay), NOT `store_view_epoch` alone.
    last_snapshot_supersession_fp: Option<u64>,
    /// Set when the post-loop FALLBACK snapshot could NOT be proven
    /// coherent — the live external-supersession fingerprint moved
    /// between the fallback `snapshot_store_view()` and the post-snapshot
    /// re-read, so the captured view describes an OLD host state while the
    /// fingerprint describes a NEWER one. A fallback taken because the
    /// coherence retry loop never obtained a stable snapshot must NEVER be
    /// promoted to the shared/stable cache: `is_stable` returns FALSE
    /// unconditionally while this is set, so the leader hands the value
    /// back (return-only) without warming the cache with a result computed
    /// under an unprovable snapshot.
    fallback_snapshot_incoherent: bool,
    /// `true` iff the view returned by the current attempt's
    /// [`StableRequestExecutor::snapshot_view`] was proven current by the
    /// `StoreViewManager` (`StoreViewRead::Current`). A non-current
    /// (`ReturnOnly`) snapshot taken under sustained churn MUST NOT serve a
    /// warm preflight hit. Gates [`Self::snapshot_view_is_current`].
    snapshot_view_current: bool,
    /// Per-attempt cold-compute completeness scope, ENTERED in
    /// [`StableRequestExecutor::compute`] and HELD through `store_stable`
    /// and [`StableRequestExecutor::capture_completeness`] so the fallthrough
    /// admission gate (`cache_fallthrough_result` / `admit_stable_node`) AND the
    /// leader's completeness snapshot both read THIS attempt's partiality —
    /// not a parent's, and not a stale prior attempt's. On `compute` the
    /// prior attempt's scope is DISCARDED first (popped WITHOUT bubbling,
    /// LIFO: it is the stack top), then a fresh one is entered, so a
    /// discarded retry's partiality taints neither the new attempt's gate
    /// nor the enclosing scope. The held scope is likewise DISCARDED on
    /// executor drop: the FINAL attempt's completeness travels out via
    /// `RequestRunResult.completeness` (the `capture_completeness` snapshot)
    /// and folds ONCE at the surface boundary (`fold_result_completeness`),
    /// which is the SOLE propagation into the enclosing cold-compute scope —
    /// bubbling the held scope too would double-propagate it and, on a
    /// retried / cache-served-final path, leak a discarded attempt's
    /// partiality into the enclosing scope.
    compute_completeness_scope: Option<ColdComputeCompletenessScope>,
    max_attempts: usize,
}

impl<'a, 'b, 'p, H: FallthroughRequestHost> FallthroughRequestExecutor<'a, 'b, 'p, H> {
    fn new(
        host: &'a H,
        canonical_id: String,
        prop_type_overrides: Option<&'a FallthroughPropOverrideSet>,
        visiting: &'b mut FxHashSet<String>,
        probe: &'p CacheabilityProbe<'p>,
        max_attempts: usize,
    ) -> Self {
        Self {
            host,
            canonical_id,
            prop_type_overrides,
            visiting,
            probe,
            fixed_store_view: None,
            last_snapshot_supersession_fp: None,
            fallback_snapshot_incoherent: false,
            snapshot_view_current: true,
            compute_completeness_scope: None,
            max_attempts,
        }
    }

    fn with_fixed_view(mut self, store_view: Option<&H::View>) -> Self {
        self.fixed_store_view = store_view.cloned();
        self
    }
}

impl<H> StableRequestExecutor<FallthroughNodeKey, Option<H::Resolution>>
    for FallthroughRequestExecutor<'_, '_, '_, H>
where
    H: FallthroughRequestHost,
{
    type View = H::View;
    type Error = ();

    fn cache_key(&self) -> FallthroughNodeKey {
        fallthrough_cache_key(
            &self.canonical_id,
            self.host.generic_root_propagation(),
            self.prop_type_overrides,
        )
    }

    fn snapshot_view(&mut self) -> Self::View {
        // Each attempt's stability must reflect ONLY that attempt's
        // snapshot. The driver's outer loop calls `snapshot_view()` up to
        // `max_attempts` times on the SAME executor instance: if an earlier
        // attempt's coherence loop exhausted and latched the fallback
        // incoherent, a LATER attempt whose inner loop (or fixed view) DOES
        // obtain a coherent snapshot must not inherit that stale latch.
        // Reset BEFORE any early-return so the flag can only be `true` if
        // THIS attempt's fallback was incoherent — mirroring how
        // `last_snapshot_supersession_fp` is re-stamped on every path.
        self.fallback_snapshot_incoherent = false;
        // A fixed view is supplied by a caller that already owns a current
        // request-bound snapshot; treat it as current for the warm peek.
        self.snapshot_view_current = true;

        if let Some(view) = self.fixed_store_view.as_ref() {
            self.last_snapshot_supersession_fp =
                Some(self.host.current_view_supersession_fingerprint());
            return view.clone();
        }

        for _ in 0..self.max_attempts {
            // Capture the live external-supersession fingerprint BEFORE
            // building the view, then re-read it AFTER: an unchanged
            // fingerprint proves no external mutation (epoch / project /
            // env / identity) straddled the build, so the snapshot is
            // coherent and the recorded fingerprint describes exactly the
            // returned view. Gating on the COMPLETE external token (not
            // `store_view_epoch` alone) rejects a mid-build env-hash shift
            // that moves no epoch. The manager's own currentness proof
            // (`is_current`) is the second gate: a `ReturnOnly` snapshot
            // under sustained churn is never accepted as a coherent
            // warm-peek snapshot even if the external fingerprint matches.
            let snapshot_fp = self.host.current_view_supersession_fingerprint();
            let (view, is_current) = self.host.snapshot_store_view_read();
            if is_current && self.host.current_view_supersession_fingerprint() == snapshot_fp {
                self.last_snapshot_supersession_fp = Some(snapshot_fp);
                self.snapshot_view_current = true;
                return view;
            }
        }

        // Coherence retries exhausted — take the fallback snapshot under
        // the SAME pre/post fingerprint discipline. The fingerprint
        // recorded for promotion is the PRE-snapshot capture (coherent with
        // the returned view iff unchanged after the build), NEVER a live
        // post-snapshot read that can describe a different host state than
        // the view. If an external mutation straddles the fallback build —
        // OR the manager could not prove the snapshot current — the
        // snapshot is not provably coherent, so mark the attempt incoherent:
        // `is_stable` then returns FALSE and the result is returned-only,
        // never promoted. The same condition forbids the warm preflight hit.
        let snapshot_fp = self.host.current_view_supersession_fingerprint();
        let (view, is_current) = self.host.snapshot_store_view_read();
        self.last_snapshot_supersession_fp = Some(snapshot_fp);
        let coherent =
            is_current && self.host.current_view_supersession_fingerprint() == snapshot_fp;
        self.fallback_snapshot_incoherent = !coherent;
        self.snapshot_view_current = coherent;
        view
    }

    fn snapshot_view_is_current(&self) -> bool {
        self.snapshot_view_current
    }

    fn try_get_cached(&mut self, view: &Self::View) -> Option<Option<H::Resolution>> {
        self.host
            .try_get_cached_fallthrough(&self.canonical_id, self.prop_type_overrides, view)
            .map(Some)
    }

    fn compute(&mut self, view: &Self::View) -> Result<Option<H::Resolution>, Self::Error> {
        // Per-attempt cold-compute completeness scope, HELD through
        // `store_stable` + `capture_completeness`. DISCARD any prior
        // attempt's scope WITHOUT bubbling (it is the stack top, LIFO) so a
        // discarded (unstable) retry's partiality taints neither the new
        // attempt's gate nor the enclosing scope. The FINAL attempt's
        // completeness reaches the enclosing scope SOLELY via
        // `RequestRunResult.completeness` + the surface-boundary
        // `fold_result_completeness`, never via an attempt-scope bubble.
        if let Some(prior) = self.compute_completeness_scope.take() {
            prior.discard();
        }
        self.compute_completeness_scope = Some(ColdComputeCompletenessScope::enter());
        Ok(self.host.compute_fallthrough_surface_uncached(
            &self.canonical_id,
            self.prop_type_overrides,
            self.visiting,
            view,
            // Thread the snapshot's currentness so the cold compute's
            // request-bound context fails its nested fallthrough-node
            // cache validation closed on a non-current (`ReturnOnly`) seed.
            self.snapshot_view_current,
        ))
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        if self.fixed_store_view.is_some() {
            return true;
        }
        // A fallback snapshot whose coherence could not be proven (the
        // external fingerprint moved across the fallback build) is NEVER
        // stable: the returned view and the recorded fingerprint may
        // describe different host states, so promoting it would warm the
        // shared cache with a fallthrough surface computed under an
        // unprovable snapshot.
        if self.fallback_snapshot_incoherent {
            return false;
        }
        // Promotion is gated on the COMPLETE external-supersession token,
        // NOT `store_view_epoch` alone: an env-hash change mid-compute
        // (e.g. `set_default_resolve_extensions`) moves no epoch but
        // invalidates the result, so the leader must NOT promote a
        // now-stale fallthrough surface to the shared cache.
        self.last_snapshot_supersession_fp
            .is_some_and(|fp| self.host.current_view_supersession_fingerprint() == fp)
    }

    fn store_stable(
        &mut self,
        value: &Option<H::Resolution>,
        _admission: crate::resolver_core::StableAdmission,
    ) {
        if let Some(result) = value.as_ref() {
            // Minted AFTER the compute from the owner-opened scope, whose
            // verdict covers every read the value was built from.
            let admission = FallthroughStableAdmission { probe: self.probe };
            self.host.store_fallthrough_result(
                &self.canonical_id,
                self.prop_type_overrides,
                result,
                &admission,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    fn capture_completeness(&self) -> ResultCompleteness {
        // The fallthrough cold compute folds budget trips / fatal reads into
        // the per-attempt held scope (entered in `compute`); read it back so
        // the LEADER publishes its COMPUTE completeness with the value, and
        // the off-lane / fallback paths carry it out. The held scope is still
        // active here (it lives until executor drop), so this reads THIS
        // attempt's partiality.
        crate::request_context::current_cold_compute_completeness()
    }

    fn fold_follower_completeness(&self, joined: ResultCompleteness) {
        // A FOLLOWER that coalesced onto a leader's fallthrough lane folds
        // the leader's EXACT partiality into its own active cold-compute
        // scope + request suppress flag BEFORE returning — so the follower's
        // own owner / payload / node admission downstream refuses to warm a
        // surface built on a leader's partial child (the no-poison fence).
        crate::request_context::fold_result_completeness(joined);
    }
}

impl<H: FallthroughRequestHost> Drop for FallthroughRequestExecutor<'_, '_, '_, H> {
    fn drop(&mut self) {
        // DISCARD the FINAL attempt's held scope (or the held scope left by a
        // prior attempt on a cache-served-final path) WITHOUT bubbling. The
        // final completeness is already carried out via `capture_completeness`
        // -> `RequestRunResult.completeness` and folded ONCE at the surface
        // boundary (`fold_result_completeness`); bubbling here would
        // double-propagate it, and on a retried / cache-served-final path it
        // would leak a discarded attempt's partiality into the enclosing
        // cold-compute scope (over-suppressing a later complete promotion).
        if let Some(scope) = self.compute_completeness_scope.take() {
            scope.discard();
        }
    }
}

/// Drive one fallthrough request (warm peek → singleflight → cold compute →
/// stable admission).
///
/// The driver owns the cacheability scope around the entire request and hands
/// only sealed stable-admission evidence to publishers. Callers cannot supply a
/// late scope or a raw probe.
pub(crate) fn run_fallthrough_request<H>(
    host: &H,
    singleflight: &SingleflightGroup<
        FallthroughNodeKey,
        StableExecutionValue<Option<H::Resolution>>,
        (),
    >,
    canonical_id: &str,
    prop_type_overrides: Option<&FallthroughPropOverrideSet>,
    visiting: &mut FxHashSet<String>,
    fixed_store_view: Option<&H::View>,
    max_attempts: usize,
) -> RequestRunResult<Option<H::Resolution>>
where
    H: FallthroughRequestHost,
{
    let tracer_host = host.cacheability_context().host_for_fact_tracer_install();
    crate::fact_signature_helpers::with_cacheability_scope(tracer_host, |probe| {
        run_fallthrough_request_in_scope(
            host,
            singleflight,
            canonical_id,
            prop_type_overrides,
            visiting,
            fixed_store_view,
            probe,
            max_attempts,
        )
    })
    .0
}

fn run_fallthrough_request_in_scope<H>(
    host: &H,
    singleflight: &SingleflightGroup<
        FallthroughNodeKey,
        StableExecutionValue<Option<H::Resolution>>,
        (),
    >,
    canonical_id: &str,
    prop_type_overrides: Option<&FallthroughPropOverrideSet>,
    visiting: &mut FxHashSet<String>,
    fixed_store_view: Option<&H::View>,
    probe: &CacheabilityProbe<'_>,
    max_attempts: usize,
) -> RequestRunResult<Option<H::Resolution>>
where
    H: FallthroughRequestHost,
{
    let mut executor = FallthroughRequestExecutor::new(
        host,
        canonical_id.to_string(),
        prop_type_overrides,
        visiting,
        probe,
        max_attempts,
    )
    .with_fixed_view(fixed_store_view);

    // Central fail-closed enforcement: an override-bearing key is wholesale
    // uncacheable, and a non-cacheable key MUST skip warm lookup, cache
    // admission, AND singleflight. Compute it cold, OFF the shared lane, and
    // return-only. Every caller of this owner-layer entry inherits the
    // guarantee — no caller relies on a host-side special case.
    if !executor.cache_key().is_cacheable() {
        let store_view = executor.snapshot_view();
        let value = executor
            .compute(&store_view)
            .expect("fallthrough request execution is infallible");
        let completeness = executor.capture_completeness();
        return RequestRunResult {
            value,
            source: RequestSource::Fallback,
            attempts: 1,
            completeness,
        };
    }

    run_stable_request(singleflight, &mut executor)
        .expect("fallthrough request execution is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{SingleflightRole, StoreViewCompatToken};
    use std::cell::Cell;

    /// Shared tracer host for request-driver unit-test hosts. The driver, not
    /// the test caller, still opens and owns every scope.
    fn test_cacheability_host() -> &'static crate::VerterHost {
        static HOST: std::sync::OnceLock<crate::VerterHost> = std::sync::OnceLock::new();
        HOST.get_or_init(|| crate::VerterHost::new_standalone(crate::HostConfig::default()))
    }

    /// Validation-trivial view: the executor's stability gate reads
    /// `current_view_supersession_fingerprint()` from the HOST, not the
    /// view, so the view only needs a stub `compat_token` for lane
    /// identity.
    #[derive(Clone)]
    struct StubView;

    impl StoreView for StubView {
        fn compat_token(&self) -> StoreViewCompatToken {
            StoreViewCompatToken {
                epoch: 1,
                session: None,
                validity_fingerprint: 0,
            }
        }

        fn validates(&self, _fact: &crate::resolver_core::FactVersionRef) -> bool {
            false
        }
    }

    /// Mock fallthrough host whose live external-supersession fingerprint
    /// can be advanced DURING `snapshot_store_view()` — modelling a
    /// concurrent external mutation (e.g. an env-hash shift) that lands
    /// while the view is being taken, so a pre/post comparison straddling
    /// the build diverges.
    struct MockHost {
        /// Live external-supersession fingerprint. `snapshot_view` captures
        /// it; `is_stable` re-reads it.
        live_fp: Cell<u64>,
        /// When `true`, EVERY `snapshot_store_view()` call advances
        /// `live_fp` (increments it) as a side effect — so the coherence
        /// retry loop never proves a coherent snapshot and the post-loop
        /// FALLBACK branch is taken, where the same straddling mutation
        /// leaves the captured view OLD relative to the post-snapshot read.
        flip_on_every_snapshot: Cell<bool>,
        /// Churn-then-settle budget. When `> 0`, each `snapshot_store_view()`
        /// call advances `live_fp` AND decrements the budget; once it reaches
        /// `0`, snapshots stop straddling. Sized to cover exactly the FIRST
        /// outer attempt's snapshots (its inner coherence loop plus the
        /// fallback) so that attempt latches the fallback incoherent, then
        /// SETTLES so a LATER outer attempt's coherence loop obtains a clean
        /// snapshot — the churn-then-settle case the per-attempt reset must
        /// handle.
        snapshot_flip_budget: Cell<usize>,
        /// Records every `store_fallthrough_result` call — i.e. every
        /// PROMOTION into the shared cache.
        promotions: std::cell::RefCell<Vec<String>>,
        /// Records every `try_get_cached_fallthrough` call — i.e. every
        /// WARM LOOKUP attempt.
        lookups: std::cell::RefCell<Vec<String>>,
        mark_hazard: Cell<bool>,
    }

    impl FallthroughRequestHost for MockHost {
        type View = StubView;
        type Resolution = usize;

        fn cacheability_context(&self) -> &dyn ResolverContext {
            test_cacheability_host()
        }

        fn generic_root_propagation(&self) -> bool {
            false
        }

        fn snapshot_store_view(&self) -> Self::View {
            // Model a concurrent external mutation landing DURING the
            // snapshot: the live fingerprint advances as the view is taken,
            // so any pre/post comparison straddling this build diverges.
            if self.flip_on_every_snapshot.get() {
                self.live_fp.set(self.live_fp.get().wrapping_add(1));
            }
            // Churn-then-settle: straddle only for the budgeted leading
            // snapshots, then settle.
            let budget = self.snapshot_flip_budget.get();
            if budget > 0 {
                self.live_fp.set(self.live_fp.get().wrapping_add(1));
                self.snapshot_flip_budget.set(budget - 1);
            }
            StubView
        }

        // Owns no `StoreViewManager` and never churns: every snapshot is
        // current by construction.
        fn snapshot_store_view_read(&self) -> (Self::View, bool) {
            (self.snapshot_store_view(), true)
        }

        fn current_view_supersession_fingerprint(&self) -> u64 {
            self.live_fp.get()
        }

        fn try_get_cached_fallthrough(
            &self,
            canonical_id: &str,
            _prop_type_overrides: Option<&FallthroughPropOverrideSet>,
            _store_view: &Self::View,
        ) -> Option<Self::Resolution> {
            self.lookups.borrow_mut().push(canonical_id.to_string());
            None
        }

        fn compute_fallthrough_surface_uncached(
            &self,
            _canonical_id: &str,
            _prop_type_overrides: Option<&FallthroughPropOverrideSet>,
            _visiting: &mut FxHashSet<String>,
            _store_view: &Self::View,
            _base_is_current: bool,
        ) -> Option<Self::Resolution> {
            if self.mark_hazard.get() {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                );
            }
            Some(42)
        }

        fn store_fallthrough_result(
            &self,
            canonical_id: &str,
            _prop_type_overrides: Option<&FallthroughPropOverrideSet>,
            _result: &Self::Resolution,
            admission: &FallthroughStableAdmission<'_>,
        ) {
            if !admission.non_cacheable() {
                self.promotions.borrow_mut().push(canonical_id.to_string());
            }
        }
    }

    #[test]
    fn incoherent_fallback_snapshot_blocks_stable_promotion() {
        // Snapshot-coherence soundness: the fallthrough executor MUST NOT
        // read the live external-supersession fingerprint AFTER taking the
        // snapshot (no pre/post discipline at all). A host mutation between
        // `snapshot_store_view()` and that live read would make the recorded
        // fingerprint describe the NEW host state while the returned view
        // is the OLD snapshot — and `is_stable` (re-reading the settled
        // live fingerprint that matches the recorded one) would then promote
        // a fallthrough surface computed under that stale snapshot.
        //
        // `flip_on_every_snapshot` advances the live fingerprint DURING
        // each `snapshot_store_view()`, so the coherence retry loop never
        // proves a coherent snapshot (pre != post on every attempt) and the
        // FALLBACK branch is taken. The fallback suffers the same straddle.
        // The fallback applies pre/post discipline, detects the straddle,
        // marks the attempt incoherent, and `is_stable` returns FALSE → no
        // promotion. The unsound shape this guards against would promote the
        // stale fallback result.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_on_every_snapshot: Cell::new(true),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            lookups: std::cell::RefCell::new(Vec::new()),
            mark_hazard: Cell::new(false),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        // `max_attempts = 1`: the internal coherence loop runs once
        // (diverges), then the FALLBACK branch is taken; the outer loop
        // runs once, so the single stability check observes the incoherent
        // fallback attempt.
        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            None,
            &mut visiting,
            None,
            1,
        );

        // The computed surface is still HANDED to the caller (return-only)…
        assert_eq!(
            result.value,
            Some(42),
            "the fallback fallthrough surface is still returned to the caller"
        );
        // …but it MUST NOT have been promoted: the fallback snapshot's
        // coherence was never provably established.
        assert!(
            host.promotions.borrow().is_empty(),
            "an incoherent FALLBACK snapshot (external fingerprint moved \
             across the fallback build) MUST block stable promotion — the \
             leader must NOT warm the shared cache with a fallthrough \
             surface computed under an unprovable snapshot taken because \
             the coherence loop could not obtain a stable one"
        );
    }

    #[test]
    fn coherent_snapshot_promotes_result() {
        // Positive counterpart: when NO external mutation straddles the
        // snapshot, the external fingerprint is unchanged across snapshot →
        // is_stable, so the fallthrough surface IS promoted. Proves the
        // gate is not blanket-suppressing promotion.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            lookups: std::cell::RefCell::new(Vec::new()),
            mark_hazard: Cell::new(false),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            None,
            &mut visiting,
            None,
            3,
        );

        assert_eq!(result.value, Some(42));
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/Child.vue".to_string()],
            "a coherent snapshot (no straddling external mutation) MUST \
             promote the fallthrough surface to the shared cache exactly once"
        );
    }

    #[test]
    fn request_owner_scope_covers_compute_and_refuses_transitive_hazard() {
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            lookups: std::cell::RefCell::new(Vec::new()),
            mark_hazard: Cell::new(true),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            None,
            &mut visiting,
            None,
            3,
        );

        assert_eq!(
            result.value,
            Some(42),
            "the return-only value is still served"
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "a transitive hazard emitted during compute must reach the owner-minted stable-admission evidence"
        );
    }

    #[test]
    fn churn_then_settle_promotes_later_coherent_attempt() {
        // Per-attempt latch reset: `fallback_snapshot_incoherent` is latched
        // `true` by the post-loop fallback path, so it MUST be reset at the
        // top of each attempt. The driver's outer loop calls `snapshot_view()`
        // up to `max_attempts` times on the SAME executor instance; without
        // the reset, a stale latch from an EARLIER attempt would wrongly
        // suppress promotion of a genuinely coherent LATER attempt's
        // fallthrough surface.
        //
        // Drive: `max_attempts = 2`. The first outer attempt's inner
        // coherence loop straddles every snapshot (2 inner + 1 fallback = 3
        // snapshots) → its fallback latches incoherent → `is_stable` false →
        // not retained → outer loop continues. The 4th snapshot onward
        // settles, so the SECOND outer attempt's inner coherence loop
        // obtains a clean snapshot and early-returns coherent. Budget = 3
        // covers exactly the first attempt's snapshots.
        //
        // Without the reset, the second attempt's coherent early-return would
        // inherit the first attempt's stale `fallback_snapshot_incoherent =
        // true` → `is_stable` returns false → the coherent surface is NOT
        // promoted → `promotions` stays empty. With the reset at the top of
        // `snapshot_view()`, the latch is cleared for the second attempt →
        // `is_stable` returns true → the surface is promoted.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_on_every_snapshot: Cell::new(false),
            // First-attempt snapshots: 2 inner coherence-loop iterations
            // (`0..max_attempts`) + 1 fallback snapshot.
            snapshot_flip_budget: Cell::new(3),
            promotions: std::cell::RefCell::new(Vec::new()),
            lookups: std::cell::RefCell::new(Vec::new()),
            mark_hazard: Cell::new(false),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            None,
            &mut visiting,
            None,
            2,
        );

        assert_eq!(
            result.value,
            Some(42),
            "the computed fallthrough surface is returned to the caller"
        );
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/Child.vue".to_string()],
            "a LATER outer attempt that obtains a coherent snapshot MUST \
             promote its fallthrough surface even though an EARLIER attempt's \
             fallback latched incoherent — each attempt's stability must \
             reflect ONLY that attempt's snapshot (per-attempt reset of \
             `fallback_snapshot_incoherent`)"
        );
    }

    #[test]
    fn uncacheable_override_request_skips_lookup_admission_and_singleflight() {
        // Central fail-closed enforcement: an override-bearing request (a
        // non-empty override set ⇒ an `Uncacheable` key) must compute cold OFF
        // the shared singleflight lane and return-only — NO warm lookup, NO
        // admission (store), NO singleflight participation.
        //
        // Discriminates Part 2's central gate: without it, the request would
        // flow into `run_stable_request`, which would `try_get_cached` (a
        // recorded lookup), join the singleflight lane (source `Flight`), and —
        // on this coherent snapshot — `store_stable` (a recorded promotion).
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            lookups: std::cell::RefCell::new(Vec::new()),
            mark_hazard: Cell::new(false),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        let overrides = FallthroughPropOverrideSet {
            entries: vec![crate::resolver_core::FallthroughPropOverride {
                name: "p".to_string(),
                node: crate::semantic_query::SemanticNodeId(1),
            }],
        };

        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            Some(&overrides),
            &mut visiting,
            None,
            3,
        );

        assert_eq!(
            result.value,
            Some(42),
            "the cold compute result is still returned to the caller"
        );
        assert!(
            matches!(result.source, RequestSource::Fallback),
            "an uncacheable request computes off-lane (Fallback), never via the singleflight (Flight), got {:?}",
            result.source
        );
        assert!(
            host.lookups.borrow().is_empty(),
            "an uncacheable request must skip warm lookup (no try_get_cached_fallthrough call), got {:?}",
            host.lookups.borrow()
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "an uncacheable request must skip cache admission (no store_fallthrough_result call), got {:?}",
            host.promotions.borrow()
        );
    }

    /// CENTERPIECE — concurrent-follower no-poison (Finding #3).
    ///
    /// A budget-tripping `NoOverrides` child A is resolved by two concurrent
    /// callers that share one fallthrough singleflight lane. The LEADER parks
    /// mid-`compute` (the established `LeaderGate` + `test_flight_strong_count`
    /// seam — deterministic, no timing sleeps), folds a PARTIAL (a budget trip
    /// modelled by `mark_request_result_partial`), then
    /// returns. The FOLLOWER coalesces onto the in-flight lane and joins the
    /// leader's partial value.
    ///
    /// Asserts: (1) the follower's observed `RequestRunResult.completeness` is
    /// partial (the rendezvous carries COMPUTE completeness out); (2) the
    /// follower FOLDED that partiality into its OWN active cold-compute scope
    /// BEFORE returning — the discriminating fence: any warm-admission the
    /// follower performs downstream now refuses; (3) nothing partial warmed
    /// the shared fallthrough cache (the leader's `store_fallthrough_result`
    /// is gated on the same typed completeness, mirroring the production
    /// `cache_fallthrough_result` no-poison gate, and a follower never stores).
    ///
    /// DISCRIMINATES: with the follower fold reverted (the
    /// `fold_follower_completeness` override / its `run_stable_request` call
    /// removed) the follower's scope stays Complete and assertion (2) FAILS —
    /// today's no-poison hole, where the follower would warm a leader's
    /// budget-partial child surface as complete into its own owner / payload
    /// caches.
    #[test]
    fn concurrent_follower_folds_leader_partiality_before_warming() {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
        use std::sync::{Arc, Condvar, Mutex};

        /// Send leader gate: leader signals `entered`, parks on `open`.
        struct LeaderGate {
            entered: Mutex<bool>,
            entered_cv: Condvar,
            open: Mutex<bool>,
            open_cv: Condvar,
        }
        impl LeaderGate {
            fn new() -> Self {
                Self {
                    entered: Mutex::new(false),
                    entered_cv: Condvar::new(),
                    open: Mutex::new(false),
                    open_cv: Condvar::new(),
                }
            }
            fn signal_entered(&self) {
                *self.entered.lock().unwrap() = true;
                self.entered_cv.notify_all();
            }
            fn wait_entered(&self) {
                let mut e = self.entered.lock().unwrap();
                while !*e {
                    e = self.entered_cv.wait(e).unwrap();
                }
            }
            fn release(&self) {
                *self.open.lock().unwrap() = true;
                self.open_cv.notify_all();
            }
            fn wait_open(&self) {
                let mut o = self.open.lock().unwrap();
                while !*o {
                    o = self.open_cv.wait(o).unwrap();
                }
            }
        }

        /// Send fallthrough host: the LEADER parks then folds a partial; both
        /// gate `store_fallthrough_result` on the typed completeness exactly
        /// like the production `cache_fallthrough_result`, so a partial result
        /// never warms — and a follower never reaches `store` at all.
        struct PoisonGatingHost {
            gate: Arc<LeaderGate>,
            is_leader: bool,
            promotions: Arc<Mutex<Vec<String>>>,
            live_fp: AtomicU64,
        }
        impl FallthroughRequestHost for PoisonGatingHost {
            type View = StubView;
            type Resolution = usize;

            fn cacheability_context(&self) -> &dyn ResolverContext {
                test_cacheability_host()
            }

            fn generic_root_propagation(&self) -> bool {
                false
            }
            fn snapshot_store_view(&self) -> StubView {
                StubView
            }
            fn snapshot_store_view_read(&self) -> (StubView, bool) {
                (StubView, true)
            }
            fn current_view_supersession_fingerprint(&self) -> u64 {
                // Fixed: the snapshot is coherent and the leader's result is
                // stable → retained → joinable by the follower.
                self.live_fp.load(AtomicOrdering::Relaxed)
            }
            fn try_get_cached_fallthrough(
                &self,
                _canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _store_view: &StubView,
            ) -> Option<usize> {
                None
            }
            fn compute_fallthrough_surface_uncached(
                &self,
                _canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _visiting: &mut FxHashSet<String>,
                _store_view: &StubView,
                _base_is_current: bool,
            ) -> Option<usize> {
                if self.is_leader {
                    self.gate.signal_entered();
                    self.gate.wait_open();
                    // The leader's cold compute trips the projection budget —
                    // fold a PARTIAL into the executor's per-attempt held
                    // cold-compute scope (entered by `compute`).
                    crate::request_context::mark_request_result_partial();
                }
                Some(42)
            }
            fn store_fallthrough_result(
                &self,
                canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _result: &usize,
                _admission: &FallthroughStableAdmission<'_>,
            ) {
                // Production no-poison gate mirror: refuse a partial.
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return;
                }
                self.promotions
                    .lock()
                    .unwrap()
                    .push(canonical_id.to_string());
            }
        }

        let gate = Arc::new(LeaderGate::new());
        let promotions = Arc::new(Mutex::new(Vec::new()));
        let singleflight = Arc::new(SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default());
        let cache_key = fallthrough_cache_key("/proj/Child.vue", false, None);
        let token = StubView.compat_token();

        // LEADER thread: parks mid-compute, then folds a partial.
        let leader = {
            let gate = Arc::clone(&gate);
            let promotions = Arc::clone(&promotions);
            let singleflight = Arc::clone(&singleflight);
            std::thread::spawn(move || {
                let host = PoisonGatingHost {
                    gate,
                    is_leader: true,
                    promotions,
                    live_fp: AtomicU64::new(0xAAAA),
                };
                let mut visiting = FxHashSet::default();
                run_fallthrough_request(
                    &host,
                    &singleflight,
                    "/proj/Child.vue",
                    None,
                    &mut visiting,
                    None,
                    3,
                )
            })
        };

        // Wait until the leader is provably parked inside `compute`, then
        // snapshot the parked-leader strong-count baseline on the run lane.
        gate.wait_entered();
        let leader_baseline = singleflight.test_flight_strong_count(&cache_key, token);

        // FOLLOWER thread: coalesces, joins the partial, folds it into its OWN
        // cold-compute scope, and reports whether the scope went partial.
        let follower = {
            let gate = Arc::clone(&gate);
            let promotions = Arc::clone(&promotions);
            let singleflight = Arc::clone(&singleflight);
            std::thread::spawn(move || {
                let host = PoisonGatingHost {
                    gate,
                    is_leader: false,
                    promotions,
                    live_fp: AtomicU64::new(0xAAAA),
                };
                let mut visiting = FxHashSet::default();
                // The follower's OWN cold-compute scope: the fold MUST land
                // here so the follower's downstream admission refuses.
                let scope = crate::request_context::ColdComputeCompletenessScope::enter();
                let result = run_fallthrough_request(
                    &host,
                    &singleflight,
                    "/proj/Child.vue",
                    None,
                    &mut visiting,
                    None,
                    3,
                );
                let scope_partial_after_join =
                    crate::request_context::current_cold_compute_completeness().is_partial();
                drop(scope);
                (result, scope_partial_after_join)
            })
        };

        // Deterministic coalescing gate: wait until the follower holds BOTH
        // its `participate` pin AND its `run_retaining` waiter claim
        // (`leader_baseline + 2`) — i.e. it has committed as a Follower past
        // its cache peek, so releasing the leader cannot turn it into a
        // pre-flight cache hit.
        let mut spins = 0u64;
        loop {
            if singleflight.test_flight_strong_count(&cache_key, token) >= leader_baseline + 2 {
                break;
            }
            spins += 1;
            assert!(
                spins < 50_000_000,
                "follower never committed as a Follower onto the shared fallthrough lane",
            );
            std::thread::yield_now();
        }

        // Release the leader; both complete.
        gate.release();

        let leader_result = leader.join().expect("leader thread must not panic");
        let (follower_result, scope_partial_after_join) =
            follower.join().expect("follower thread must not panic");

        // The straggler Follower-joined the in-flight leader (not a second
        // leader, not a pre-flight cache hit).
        assert!(
            matches!(
                follower_result.source,
                RequestSource::Flight {
                    role: SingleflightRole::Follower,
                    ..
                }
            ),
            "the second caller must Follower-join the in-flight leader, got {:?}",
            follower_result.source,
        );
        // (1) The rendezvous carried the leader's COMPUTE completeness out.
        assert!(
            follower_result.completeness.is_partial(),
            "the follower's RequestRunResult must carry the leader's partial completeness",
        );
        // (2) DISCRIMINATING — the follower folded that partiality into its
        // OWN cold-compute scope BEFORE returning. Reverting the follower fold
        // leaves this Complete (the no-poison hole).
        assert!(
            scope_partial_after_join,
            "the follower MUST fold the leader's partiality into its own cold-compute scope \
             before any warm-admission — else it would warm a leader's budget-partial child \
             surface as complete (the concurrent-follower no-poison hole)",
        );
        // (3) Nothing partial warmed the shared fallthrough cache: the leader's
        // admission is gated on the same typed completeness, and a follower
        // never stores.
        assert!(
            leader_result.completeness.is_partial(),
            "the leader's own result is partial (it tripped the budget)",
        );
        assert!(
            promotions.lock().unwrap().is_empty(),
            "a budget-partial child surface must NEVER warm the shared fallthrough cache — \
             neither the gated leader nor the coalescing follower may promote it, got {:?}",
            promotions.lock().unwrap(),
        );
    }

    /// DISCARDED-RETRY no-over-suppression: a NON-FINAL fallthrough attempt
    /// that folds a PARTIAL and is then DISCARDED (its snapshot proved
    /// unstable, so the driver retries) must NOT bubble its partiality into
    /// the ENCLOSING cold-compute scope — only the FINAL attempt's
    /// completeness propagates, via `RequestRunResult.completeness` + the
    /// surface-boundary fold.
    ///
    /// Drive: attempt 1 folds a partial and advances the live supersession
    /// fingerprint AFTER recording it, so the executor's `is_stable` re-read
    /// diverges → the attempt is unstable → discarded → the outer loop
    /// retries. Attempt 2 leaves the fingerprint fixed and folds NOTHING →
    /// stable + complete → promoted.
    ///
    /// Asserts: (1) the final value is the COMPLETE attempt-2 value; (2) the
    /// final `RequestRunResult.completeness` is Complete (not attempt-1's
    /// discarded partial); (3) the ENCLOSING scope stays Complete — NOT
    /// poisoned by the discarded attempt-1 partiality; (4) the complete
    /// result WARMS the shared cache.
    ///
    /// DISCRIMINATES: with the per-attempt discard reverted (attempt scopes
    /// drop with the default bubble), attempt 2's `compute` drops attempt 1's
    /// held scope WITH bubbling, merging the discarded partial into the
    /// enclosing scope → assertion (3) FAILS (the over-suppression hole).
    #[test]
    fn discarded_unstable_attempt_partiality_does_not_taint_enclosing_scope() {
        use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};

        /// First compute attempt folds a PARTIAL and is UNSTABLE (discarded);
        /// the second is COMPLETE and STABLE (promoted). The unstable signal
        /// is a live-fingerprint advance recorded AFTER the partial fold, so
        /// the executor's `is_stable` re-read diverges from the snapshot
        /// fingerprint on attempt 1 only.
        struct RetryCompletenessHost {
            attempts: AtomicUsize,
            live_fp: AtomicU64,
            promotions: std::cell::RefCell<Vec<String>>,
        }
        impl FallthroughRequestHost for RetryCompletenessHost {
            type View = StubView;
            type Resolution = usize;

            fn cacheability_context(&self) -> &dyn ResolverContext {
                test_cacheability_host()
            }

            fn generic_root_propagation(&self) -> bool {
                false
            }
            fn snapshot_store_view(&self) -> StubView {
                StubView
            }
            fn snapshot_store_view_read(&self) -> (StubView, bool) {
                (StubView, true)
            }
            fn current_view_supersession_fingerprint(&self) -> u64 {
                self.live_fp.load(AtomicOrdering::Relaxed)
            }
            fn try_get_cached_fallthrough(
                &self,
                _canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _store_view: &StubView,
            ) -> Option<usize> {
                None
            }
            fn compute_fallthrough_surface_uncached(
                &self,
                _canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _visiting: &mut FxHashSet<String>,
                _store_view: &StubView,
                _base_is_current: bool,
            ) -> Option<usize> {
                let n = self.attempts.fetch_add(1, AtomicOrdering::Relaxed);
                if n == 0 {
                    // Attempt 1: fold a PARTIAL into the per-attempt held
                    // scope, then advance the fingerprint so `is_stable`
                    // diverges → unstable → discarded.
                    crate::request_context::mark_request_result_partial();
                    self.live_fp.fetch_add(1, AtomicOrdering::Relaxed);
                    Some(1)
                } else {
                    // Attempt 2: COMPLETE (no partial fold); the fingerprint
                    // stays fixed → stable → promoted.
                    Some(2)
                }
            }
            fn store_fallthrough_result(
                &self,
                canonical_id: &str,
                _overrides: Option<&FallthroughPropOverrideSet>,
                _result: &usize,
                _admission: &FallthroughStableAdmission<'_>,
            ) {
                // Production no-poison gate mirror: refuse a partial.
                if crate::request_context::current_cold_compute_completeness().is_partial() {
                    return;
                }
                self.promotions.borrow_mut().push(canonical_id.to_string());
            }
        }

        let host = RetryCompletenessHost {
            attempts: AtomicUsize::new(0),
            live_fp: AtomicU64::new(0xBEEF),
            promotions: std::cell::RefCell::new(Vec::new()),
        };
        let singleflight = SingleflightGroup::<
            FallthroughNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();
        let mut visiting = FxHashSet::default();

        // ENCLOSING cold-compute scope (the extract helper / parent
        // fallthrough compute analogue). A DISCARDED attempt's partiality
        // must never taint it.
        let enclosing = crate::request_context::ColdComputeCompletenessScope::enter();
        let result = run_fallthrough_request(
            &host,
            &singleflight,
            "/proj/Child.vue",
            None,
            &mut visiting,
            None,
            3,
        );
        // Read the enclosing scope's completeness BEFORE dropping it.
        let enclosing_partial_after =
            crate::request_context::current_cold_compute_completeness().is_partial();
        drop(enclosing);

        assert_eq!(
            host.attempts.load(AtomicOrdering::Relaxed),
            2,
            "exactly two compute attempts ran: the unstable partial (discarded) then the \
             complete stable one",
        );
        assert_eq!(
            result.value,
            Some(2),
            "the COMPLETE second attempt's value is the one returned",
        );
        assert!(
            !result.completeness.is_partial(),
            "the final RequestRunResult completeness is Complete (attempt 2), NOT the discarded \
             attempt-1 partial",
        );
        // DISCRIMINATING — reverting the per-attempt discard bubbles
        // attempt-1's partial into the enclosing scope.
        assert!(
            !enclosing_partial_after,
            "a DISCARDED unstable attempt's partiality MUST NOT taint the enclosing cold-compute \
             scope — reverting the per-attempt discard merges attempt-1's discarded partial into \
             the enclosing scope (the over-suppression hole), false-refusing a later complete \
             promotion under it",
        );
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/Child.vue".to_string()],
            "the COMPLETE second attempt MUST warm the shared fallthrough cache exactly once — its \
             `store_stable` reads a Complete scope (the discarded attempt-1 partial was popped \
             without bubbling)",
        );
    }
}
