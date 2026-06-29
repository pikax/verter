use rustc_hash::FxHashSet;

use crate::resolver_core::{
    fallthrough_cache_key, run_stable_request, FallthroughNodeKey, FallthroughPropOverrideSet,
    RequestRunResult, RequestSource, SingleflightGroup, StableExecutionValue,
    StableRequestExecutor, StoreView,
};

pub trait FallthroughRequestHost {
    type View: StoreView + Clone;
    type Resolution: Clone;

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
    fn store_fallthrough_result(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FallthroughPropOverrideSet>,
        result: &Self::Resolution,
    );
}

struct FallthroughRequestExecutor<'a, 'b, H: FallthroughRequestHost> {
    host: &'a H,
    canonical_id: String,
    prop_type_overrides: Option<&'a FallthroughPropOverrideSet>,
    visiting: &'b mut FxHashSet<String>,
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
    max_attempts: usize,
}

impl<'a, 'b, H: FallthroughRequestHost> FallthroughRequestExecutor<'a, 'b, H> {
    fn new(
        host: &'a H,
        canonical_id: String,
        prop_type_overrides: Option<&'a FallthroughPropOverrideSet>,
        visiting: &'b mut FxHashSet<String>,
        max_attempts: usize,
    ) -> Self {
        Self {
            host,
            canonical_id,
            prop_type_overrides,
            visiting,
            fixed_store_view: None,
            last_snapshot_supersession_fp: None,
            fallback_snapshot_incoherent: false,
            snapshot_view_current: true,
            max_attempts,
        }
    }

    fn with_fixed_view(mut self, store_view: Option<&H::View>) -> Self {
        self.fixed_store_view = store_view.cloned();
        self
    }
}

impl<'a, 'b, H> StableRequestExecutor<FallthroughNodeKey, Option<H::Resolution>>
    for FallthroughRequestExecutor<'a, 'b, H>
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

    fn store_stable(&mut self, value: &Option<H::Resolution>) {
        if let Some(result) = value.as_ref() {
            self.host.store_fallthrough_result(
                &self.canonical_id,
                self.prop_type_overrides,
                result,
            );
        }
    }

    fn max_attempts(&self) -> usize {
        self.max_attempts
    }
}

pub fn run_fallthrough_request<H>(
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
    let mut executor = FallthroughRequestExecutor::new(
        host,
        canonical_id.to_string(),
        prop_type_overrides,
        visiting,
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
        return RequestRunResult {
            value,
            source: RequestSource::Fallback,
            attempts: 1,
        };
    }

    run_stable_request(singleflight, &mut executor)
        .expect("fallthrough request execution is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::StoreViewCompatToken;
    use std::cell::Cell;

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
    }

    impl FallthroughRequestHost for MockHost {
        type View = StubView;
        type Resolution = usize;

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
            Some(42)
        }

        fn store_fallthrough_result(
            &self,
            canonical_id: &str,
            _prop_type_overrides: Option<&FallthroughPropOverrideSet>,
            _result: &Self::Resolution,
        ) {
            self.promotions.borrow_mut().push(canonical_id.to_string());
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
}
