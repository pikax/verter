use crate::resolver_core::{
    run_stable_request, RequestRunResult, ResolutionNodeKey, SingleflightGroup,
    StableExecutionValue, StableRequestExecutor, StoreView,
};

/// By-value result of the owner-scoped component-meta cold computation.
/// `cache_refusal` is orthogonal to typed completeness: the value remains
/// complete and returnable, but cannot be published or retained.
pub(crate) struct ComponentMetaComputeOutcome<R> {
    pub(crate) value: Option<R>,
    pub(crate) cache_refusal: Option<crate::resolver_core::fact_read_set::NonCacheablePropagation>,
}

#[cfg(test)]
impl<R> ComponentMetaComputeOutcome<R> {
    fn from_owner_scope(value: Option<R>, non_cacheable: bool) -> Self {
        Self {
            value,
            cache_refusal: non_cacheable.then_some(
                crate::resolver_core::fact_read_set::NonCacheablePropagation::Transitive,
            ),
        }
    }
}

pub(crate) trait ComponentMetaRequestHost {
    type View: StoreView + Clone;
    type Mode: Copy;
    type Resolution: Clone;
    type CapturedInputs;

    fn cache_key(&self, canonical: &str, mode: Self::Mode) -> ResolutionNodeKey;
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
    /// (test/stub hosts) state that explicitly by returning `(view, true)`.
    fn snapshot_store_view_read(&self) -> (Self::View, bool);
    /// Live `u64` fold of the host's EXTERNAL-supersession token
    /// dimensions (epoch / project-generation / env-hash / identity /
    /// overlay — the same set
    /// `StoreViewValidationToken::externally_superseded_by` compares,
    /// EXCLUDING the compute's own artifact / load
    /// generations). Captured at snapshot time and re-read at
    /// stable-promotion time: a mismatch means an external mutation that
    /// the `store_view_epoch` alone does NOT track (e.g. a
    /// `set_default_resolve_extensions` env-hash shift) advanced
    /// mid-compute, so the result MUST NOT be promoted.
    fn current_view_supersession_fingerprint(&self) -> u64;
    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        store_view: &Self::View,
    ) -> Option<Self::CapturedInputs>;
    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution>;
    /// Run the cold component-meta compute against `store_view`.
    ///
    /// `base_is_current` is the manager's currentness proof for the
    /// snapshot the executor took (`StableRequestExecutor::
    /// snapshot_view_is_current`): `true` for a proven-`Current` view,
    /// `false` for a known-stale `ReturnOnly` view taken under sustained
    /// churn. The implementor MUST thread it into the request-bound
    /// resolver context (via `from_cold_seed`) so that, on a non-current
    /// seed, every NESTED warm-cache probe inside the cold compute MISSES
    /// rather than validating a cache entry against the stale snapshot. The
    /// fenced cold builder still computes from the seed (the outer
    /// `is_stable` / publish fence rejects promotion of a non-current
    /// result); only its nested probes fail closed.
    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
        base_is_current: bool,
    ) -> ComponentMetaComputeOutcome<Self::Resolution>;
    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    );
    /// Typed structural completeness carried by `result`. Every scalar and
    /// fixed-view lane captures this immediately after compute; a partial is
    /// returned to its caller but can neither reach a publisher nor remain as
    /// a joinable rendezvous. This is deliberately required: a defaulted
    /// `Complete` would let a host silently warm a transient budget failure as
    /// a sticky degraded surface.
    fn resolution_completeness(
        &self,
        result: &Self::Resolution,
    ) -> crate::semantic_query::ResultCompleteness;
}

struct ComponentMetaRequestExecutor<'a, H: ComponentMetaRequestHost> {
    host: &'a H,
    canonical: String,
    mode: H::Mode,
    /// A caller-supplied fixed snapshot: the store view, the host's
    /// external-supersession fingerprint CAPTURED when the caller acquired
    /// the view, and whether that capture was proven CURRENT (e.g. the
    /// batch coordinator captures the one shared view for the whole batch
    /// via `capture_batch_fixed_view`).
    ///
    /// Soundness contract: the fixed view is NOT an unconditional-stable
    /// bypass. The promotion fence (`is_stable`) gates on TWO conditions,
    /// mirroring the cold-path publish fence:
    ///
    /// 1. the capture was proven current — a non-current
    ///    (`ReturnOnly`-derived) capture is return-only by the
    ///    `StoreViewRead` contract and is NEVER promoted; and
    /// 2. no external mutation (epoch / project-generation / env-hash /
    ///    identity / overlay) landed between the caller's capture and the
    ///    compute's completion — the captured fingerprint still equals the
    ///    LIVE fingerprint.
    ///
    /// A failed gate means the fixed view is stale: the result is returned
    /// to the caller (return-only) but NEVER warmed into the shared cache.
    /// The captured currentness also drives `snapshot_view_current`, so a
    /// non-current fixed view fails the warm preflight peek and the cold
    /// compute's nested warm-cache probes close.
    fixed_store_view: Option<(H::View, u64, bool)>,
    /// EXTERNAL-supersession fingerprint of the snapshot the current
    /// attempt was taken under. Gates BOTH the snapshot-coherence retry
    /// and the stable-promotion decision on the COMPLETE external-token
    /// dimensions (epoch / project-generation / env / identity /
    /// overlay), NOT `store_view_epoch` alone — an env-hash shift
    /// mid-compute moves no epoch but MUST block promotion of the now
    /// stale result.
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
    /// warm preflight hit: the driver's preflight `try_get_cached` would
    /// otherwise validate a cache entry against a known-stale view and
    /// false-positive a superseded result. Gates
    /// [`Self::snapshot_view_is_current`].
    snapshot_view_current: bool,
    captured_inputs: Option<H::CapturedInputs>,
    last_completeness: crate::semantic_query::ResultCompleteness,
    last_cache_refusal: Option<crate::resolver_core::fact_read_set::NonCacheablePropagation>,
    max_attempts: usize,
}

impl<'a, H: ComponentMetaRequestHost> ComponentMetaRequestExecutor<'a, H> {
    fn new(host: &'a H, canonical: String, mode: H::Mode, max_attempts: usize) -> Self {
        Self {
            host,
            canonical,
            mode,
            fixed_store_view: None,
            last_snapshot_supersession_fp: None,
            fallback_snapshot_incoherent: false,
            snapshot_view_current: true,
            captured_inputs: None,
            last_completeness: crate::semantic_query::ResultCompleteness::Complete,
            last_cache_refusal: None,
            max_attempts,
        }
    }

    fn with_fixed_view(mut self, store_view: Option<(&H::View, u64, bool)>) -> Self {
        self.fixed_store_view = store_view
            .map(|(view, captured_fp, is_current)| (view.clone(), captured_fp, is_current));
        self
    }
}

impl<'a, H> StableRequestExecutor<ResolutionNodeKey, Option<H::Resolution>>
    for ComponentMetaRequestExecutor<'a, H>
where
    H: ComponentMetaRequestHost,
{
    type View = H::View;
    type Error = ();

    fn cache_key(&self) -> ResolutionNodeKey {
        self.host.cache_key(&self.canonical, self.mode)
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
        // Default for the per-attempt path below; the fixed-view branch
        // overrides it with the capture's proven currentness.
        self.snapshot_view_current = true;

        if let Some((view, captured_fp, is_current)) = self.fixed_store_view.as_ref() {
            // Record the CAPTURED external-supersession fingerprint (taken
            // by the caller when it acquired the fixed view), NOT a fresh
            // live read here. The promotion fence then compares this
            // captured value against the live fingerprint: a mismatch
            // proves an external mutation landed since the caller captured
            // the view, so the result is return-only. Sampling the live
            // fingerprint here instead would defeat the fence — it would
            // always equal itself when re-read with no intervening
            // mutation, but a mutation that landed BEFORE this snapshot
            // (yet after the caller's capture) would go undetected.
            self.last_snapshot_supersession_fp = Some(*captured_fp);
            // The fixed view carries the capture's currentness. A
            // non-current capture (`ReturnOnly` under sustained churn) must
            // suppress the warm preflight peek and force the cold compute's
            // nested probes closed — exactly as a non-current per-attempt
            // snapshot does. It must NOT be laundered to `true` here.
            self.snapshot_view_current = *is_current;
            self.captured_inputs = self
                .host
                .capture_component_meta_inputs(&self.canonical, view);
            return view.clone();
        }

        for _ in 0..self.max_attempts {
            // Capture the live external-supersession fingerprint BEFORE
            // building the view, then re-read it AFTER capturing inputs:
            // an unchanged fingerprint proves no external mutation
            // (epoch / project / env / identity) straddled the build, so
            // the snapshot is coherent. Gating on the COMPLETE external
            // token (not `store_view_epoch` alone) rejects a mid-build
            // env-hash shift that moves no epoch. The manager's own
            // currentness proof (`is_current`) is the second gate: a
            // `ReturnOnly` snapshot under sustained churn is never accepted
            // as a coherent warm-peek snapshot even if the external
            // fingerprint happens to match.
            let snapshot_fp = self.host.current_view_supersession_fingerprint();
            let (view, is_current) = self.host.snapshot_store_view_read();
            let captured_inputs = self
                .host
                .capture_component_meta_inputs(&self.canonical, &view);
            if is_current && self.host.current_view_supersession_fingerprint() == snapshot_fp {
                self.last_snapshot_supersession_fp = Some(snapshot_fp);
                self.captured_inputs = captured_inputs;
                self.snapshot_view_current = true;
                return view;
            }
        }

        // Coherence retries exhausted — take the fallback snapshot under
        // the SAME pre/post fingerprint discipline as the loop above. The
        // fingerprint recorded for promotion is the PRE-snapshot capture
        // (coherent with the returned view iff it is unchanged after the
        // build), NEVER a live post-snapshot read that can describe a
        // different host state than the view. If an external mutation
        // straddles the fallback build — OR the manager could not prove
        // the snapshot current — the snapshot is not provably coherent, so
        // mark the attempt incoherent: `is_stable` then returns FALSE and
        // the result is returned-only, never promoted. The same condition
        // also forbids the warm preflight hit (`snapshot_view_current`).
        let snapshot_fp = self.host.current_view_supersession_fingerprint();
        let (view, is_current) = self.host.snapshot_store_view_read();
        self.captured_inputs = self
            .host
            .capture_component_meta_inputs(&self.canonical, &view);
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

    fn snapshot_is_immutable(&self) -> bool {
        // A caller-pinned fixed view is the only immutable snapshot: every
        // `snapshot_view` call re-presents the SAME captured view + fingerprint
        // + currentness (the early-return branch above), so an unstable result
        // can never converge on a retry. The per-attempt path (`None`) re-reads
        // live state each attempt and MUST keep retrying.
        self.fixed_store_view.is_some()
    }

    fn try_get_cached(&mut self, view: &Self::View) -> Option<Option<H::Resolution>> {
        self.host
            .try_get_cached_component_meta(&self.canonical, self.mode, view)
            .map(Some)
    }

    fn compute(&mut self, view: &Self::View) -> Result<Option<H::Resolution>, Self::Error> {
        let outcome = self.host.compute_component_meta(
            &self.canonical,
            self.mode,
            self.captured_inputs.as_ref(),
            Some(view),
            // Thread the snapshot's currentness so the cold compute's
            // request-bound context fails its nested warm-cache probes
            // closed on a non-current (`ReturnOnly`) seed.
            self.snapshot_view_current,
        );
        self.last_completeness = outcome
            .value
            .as_ref()
            .map(|value| self.host.resolution_completeness(value))
            .unwrap_or(crate::semantic_query::ResultCompleteness::Complete);
        self.last_cache_refusal = outcome.cache_refusal;
        Ok(outcome.value)
    }

    fn is_stable(&mut self, _view: &Self::View) -> bool {
        // A fallback snapshot whose coherence could not be proven (the
        // external fingerprint moved across the fallback build) is NEVER
        // stable: the returned view and the recorded fingerprint may
        // describe different host states, so promoting it would warm the
        // shared cache with a result computed under an unprovable snapshot.
        // (A fixed view returns early from `snapshot_view` before this flag
        // is set, and the flag is reset to `false` at the top of every
        // `snapshot_view`, so this never spuriously fires for a fixed view.)
        if self.fallback_snapshot_incoherent {
            return false;
        }
        // A non-current snapshot is NEVER promotable, mirroring the
        // cold-path publish fence (`validation_token_still_live`): the
        // manager could not prove the snapshot current, so its result is
        // return-only even if the external fingerprint happens to still
        // match. For the per-attempt path `snapshot_view_current` is set by
        // the coherence loop; for a FIXED view it carries the capture's
        // proven currentness. Without this gate a fixed view captured as
        // `ReturnOnly` under sustained churn could promote a result
        // computed against a stale seed whenever its captured fingerprint
        // coincidentally still matched the live one.
        if !self.snapshot_view_current {
            return false;
        }
        // Promotion is gated on the COMPLETE external-supersession token,
        // NOT `store_view_epoch` alone: an env-hash change mid-compute
        // (e.g. `set_default_resolve_extensions`) moves no epoch but
        // invalidates the result, so the leader must NOT mark a now-stale
        // result stable and promote it to the shared cache.
        //
        // For a FIXED view this is the SOUNDNESS fence, not an
        // unconditional pass: `last_snapshot_supersession_fp` holds the
        // fingerprint the CALLER captured when it acquired the view, so
        // this comparison promotes ONLY when the live token has not moved
        // since that capture. A fixed view computed against a now-stale
        // snapshot fails this gate and is returned-only — never promoted.
        self.last_snapshot_supersession_fp
            .is_some_and(|fp| self.host.current_view_supersession_fingerprint() == fp)
    }

    fn store_stable(
        &mut self,
        value: &Option<H::Resolution>,
        _admission: crate::resolver_core::StableAdmission,
    ) {
        if let Some(result) = value.as_ref() {
            // The driver can mint `StableAdmission` only after proving this
            // attempt current, structurally complete, and free of an
            // owner-scoped cache refusal. This publisher therefore has no
            // second, drift-prone boolean policy to re-derive.
            self.host
                .store_component_meta_result(&self.canonical, self.mode, result);
        }
    }

    fn max_attempts(&self) -> usize {
        self.max_attempts
    }

    fn capture_completeness(&self) -> crate::semantic_query::ResultCompleteness {
        self.last_completeness
    }

    fn capture_cache_refusal(
        &self,
    ) -> Option<crate::resolver_core::fact_read_set::NonCacheablePropagation> {
        self.last_cache_refusal
    }

    fn fold_follower_completeness(&self, joined: crate::semantic_query::ResultCompleteness) {
        crate::request_context::fold_result_completeness(joined);
    }
}

/// Run a component-meta request through the stable-request driver.
///
/// `fixed_store_view`, when `Some`, pins the compute to a caller-captured
/// snapshot: `(view, captured_external_supersession_fingerprint,
/// captured_is_current)`. The captured fingerprint + currentness are the
/// snapshot's external-token proof at capture time; the driver's
/// promotion fence promotes the result into the shared cache ONLY when the
/// capture was current AND the captured fingerprint still equals the live
/// host fingerprint (no external mutation landed since capture). A fixed
/// view is therefore a FENCED fixed snapshot, never an unconditional
/// stable bypass — a result computed against a now-stale (or non-current)
/// fixed view is returned to the caller but not promoted. `None` runs the
/// per-attempt snapshot-coherence path.
pub(crate) fn run_component_meta_request<H>(
    host: &H,
    singleflight: &SingleflightGroup<
        ResolutionNodeKey,
        StableExecutionValue<Option<H::Resolution>>,
        (),
    >,
    canonical: &str,
    mode: H::Mode,
    fixed_store_view: Option<(&H::View, u64, bool)>,
    max_attempts: usize,
) -> RequestRunResult<Option<H::Resolution>>
where
    H: ComponentMetaRequestHost,
{
    let mut executor =
        ComponentMetaRequestExecutor::new(host, canonical.to_string(), mode, max_attempts)
            .with_fixed_view(fixed_store_view);
    run_stable_request(singleflight, &mut executor)
        .expect("component-meta request execution is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver_core::{
        RequestSource, ResolutionNodeKind, StoreViewCompatToken, TraversalLens,
    };
    use std::cell::Cell;

    /// Validation-trivial view: the executor's stability gate now reads
    /// `current_view_supersession_fingerprint()` from the HOST, not the
    /// view, so the view only needs a stub `compat_token` for the lane
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

    /// Mock host whose live external-supersession fingerprint can be
    /// flipped to simulate an env-hash change (e.g.
    /// `set_default_resolve_extensions`) that moves NO epoch. The
    /// `compute` step optionally flips it mid-compute so `is_stable`
    /// observes the divergence — exactly the stale-promotion race.
    struct MockHost {
        /// Live external-supersession fingerprint. `snapshot_view`
        /// captures this; `is_stable` re-reads it.
        live_fp: Cell<u64>,
        /// If `Some`, the FIRST `compute` sets `live_fp` to this value (a
        /// one-shot mid-compute external mutation), then disarms. One-shot
        /// because a real env-hash shift lands once; a subsequent retry
        /// computes against the new (stable) env. `None` = stable compute.
        flip_to_on_compute: Option<u64>,
        /// Disarms `flip_to_on_compute` after the first compute.
        flip_armed: Cell<bool>,
        /// When `true`, EVERY `snapshot_store_view_read()` call advances
        /// `live_fp` (increments it) as a side effect — modelling a
        /// concurrent external mutation that lands DURING the snapshot. A
        /// per-attempt pre/post fingerprint comparison therefore always
        /// diverges, so the coherence retry loop never finds a coherent
        /// snapshot and the post-loop FALLBACK branch is taken. The
        /// fallback then suffers the same straddling mutation: the captured
        /// view is OLD relative to the post-snapshot fingerprint read.
        flip_on_every_snapshot: Cell<bool>,
        /// Churn-then-settle budget. When `> 0`, each `snapshot_store_view_read()`
        /// call advances `live_fp` AND decrements the budget; once it reaches
        /// `0`, snapshots stop straddling. Sized to cover exactly the FIRST
        /// outer attempt's snapshots (its inner coherence loop plus the
        /// fallback) so that attempt latches the fallback incoherent, then
        /// SETTLES so a LATER outer attempt's coherence loop obtains a clean
        /// snapshot — the churn-then-settle case the per-attempt reset must
        /// handle.
        snapshot_flip_budget: Cell<usize>,
        /// Records every `store_component_meta_result` call — i.e. every
        /// PROMOTION into the shared cache.
        promotions: std::cell::RefCell<Vec<String>>,
        /// Count of every `compute_component_meta` call — i.e. every cold
        /// compute the driver ran. A fixed (immutable) snapshot that cannot
        /// converge to stable must compute EXACTLY ONCE (return-only), not
        /// `max_attempts + 1` times.
        computes: Cell<usize>,
    }

    const PARTIAL_RESULT_FP: u64 = 0xD3AD_B0D6;
    const NON_CACHEABLE_RESULT_FP: u64 = 0xCACE_F00D;

    impl ComponentMetaRequestHost for MockHost {
        type View = StubView;
        type Mode = ();
        type Resolution = usize;
        type CapturedInputs = ();

        fn cache_key(&self, canonical: &str, _mode: Self::Mode) -> ResolutionNodeKey {
            ResolutionNodeKey {
                symbol_id: canonical.to_string(),
                node_kind: ResolutionNodeKind::Assemble,
                traversal_lens: TraversalLens::StructuralObject,
                member_path_hash: 0,
                type_args_hash: 0,
                behavior_flags: 0,
                view_fingerprint: 0,
            }
        }

        // Owns no `StoreViewManager`; after applying the synthetic churn
        // below, the returned stub view itself is current by construction.
        fn snapshot_store_view_read(&self) -> (Self::View, bool) {
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
            (StubView, true)
        }

        fn current_view_supersession_fingerprint(&self) -> u64 {
            self.live_fp.get()
        }

        fn capture_component_meta_inputs(
            &self,
            _canonical: &str,
            _store_view: &Self::View,
        ) -> Option<Self::CapturedInputs> {
            Some(())
        }

        fn try_get_cached_component_meta(
            &self,
            _canonical: &str,
            _mode: Self::Mode,
            _store_view: &Self::View,
        ) -> Option<Self::Resolution> {
            None
        }

        fn compute_component_meta(
            &self,
            _canonical: &str,
            _mode: Self::Mode,
            _captured: Option<&Self::CapturedInputs>,
            _store_view: Option<&Self::View>,
            _base_is_current: bool,
        ) -> ComponentMetaComputeOutcome<Self::Resolution> {
            self.computes.set(self.computes.get() + 1);
            // Simulate an external mutation (env-hash shift) landing
            // mid-compute: the live external fingerprint moves WITHOUT any
            // epoch change. One-shot — disarmed after the first compute.
            if self.flip_armed.get() {
                if let Some(target) = self.flip_to_on_compute {
                    self.live_fp.set(target);
                    self.flip_armed.set(false);
                }
            }
            ComponentMetaComputeOutcome::from_owner_scope(
                Some(42),
                self.live_fp.get() == NON_CACHEABLE_RESULT_FP,
            )
        }

        fn store_component_meta_result(
            &self,
            canonical: &str,
            _mode: Self::Mode,
            _result: &Self::Resolution,
        ) {
            self.promotions.borrow_mut().push(canonical.to_string());
        }

        // The sentinel fingerprint drives a stable-but-partial result for
        // the scalar no-poison regression without changing the value type.
        fn resolution_completeness(
            &self,
            _result: &Self::Resolution,
        ) -> crate::semantic_query::ResultCompleteness {
            if self.live_fp.get() == PARTIAL_RESULT_FP {
                crate::semantic_query::ResultCompleteness::partial(
                    crate::semantic_query::PartialReasonSet::PROPAGATED,
                )
            } else {
                crate::semantic_query::ResultCompleteness::Complete
            }
        }
    }

    #[test]
    fn env_hash_change_mid_compute_blocks_stable_promotion() {
        // With the token modeling non-epoch dimensions, an env-hash change
        // WITHOUT an epoch bump (e.g. `set_default_resolve_extensions`)
        // during a compute must prevent the leader from marking a now-stale
        // result stable and promoting it to the shared cache. An `is_stable`
        // gated only on `current_store_view_epoch() == snapshot_epoch`
        // would leave an env-hash-only shift undetected (epoch unchanged) →
        // `is_stable` returns true → the stale result is promoted. The gate
        // reads the COMPLETE external-supersession fingerprint, which the
        // env-hash shift moves → promotion is blocked.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            // The (one-shot) compute flips the external fingerprint
            // (env-hash shift) — NO epoch change is modeled at all.
            flip_to_on_compute: Some(0xBBBB),
            flip_armed: Cell::new(true),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // `max_attempts = 1`: exactly ONE stability check, taken on the
        // attempt where the fingerprint diverges mid-compute. If
        // `is_stable` ignored the fingerprint (epoch-only) it
        // would return true here and PROMOTE the stale result; the
        // fingerprint gate returns false and blocks it.
        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 1);

        // The result is still HANDED to the caller (return-only)…
        assert_eq!(
            result.value,
            Some(42),
            "the computed value is still returned to the caller"
        );
        // …but it MUST NOT have been promoted to the shared cache: the
        // external fingerprint diverged between snapshot and is_stable.
        assert!(
            host.promotions.borrow().is_empty(),
            "an env-hash change mid-compute (no epoch bump) MUST block stable \
             promotion — the leader must NOT warm the shared cache with a result \
             computed against a now-superseded view"
        );
    }

    #[test]
    fn fixed_view_promotes_when_captured_fingerprint_still_live() {
        // Positive fixed-view case: a caller (the batch coordinator)
        // captures ONE store view plus its external-supersession
        // fingerprint, then threads BOTH into the executor as a fixed
        // view. When NO external mutation lands between capture and the
        // promotion fence, the captured fingerprint still equals the live
        // one, so the fixed-view result IS promoted. Proves the fixed-view
        // gate is not blanket-suppressing promotion.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // The batch captures the view and its fingerprint together. With
        // no mid-compute mutation, `captured_fp == live_fp` at the fence.
        // `is_current = true` — the batch captured a proven-current view.
        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, true)),
            1,
        );

        assert_eq!(result.value, Some(42));
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/App.vue".to_string()],
            "a fixed-view compute whose captured external fingerprint still \
             matches the live host fingerprint at the promotion fence (and \
             whose capture was proven current) MUST promote the result \
             exactly once"
        );
    }

    #[test]
    fn fixed_view_blocks_promotion_when_capture_was_not_current() {
        // SOUNDNESS: a fixed view captured as NON-CURRENT (a `ReturnOnly`
        // read under sustained churn) must NEVER promote, EVEN when its
        // captured external fingerprint still matches the live one — the
        // manager could not prove the snapshot current, so its result is
        // return-only by the `StoreViewRead` contract (mirroring the
        // cold-path publish fence's seed-currentness gate). Without the
        // `snapshot_view_current` gate in `is_stable`, a non-current
        // capture whose fingerprint coincidentally matched would promote a
        // result computed against a stale seed.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // Fingerprint matches at the fence (no mutation), but the capture
        // is NOT current (`is_current = false`).
        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, false)),
            1,
        );

        assert_eq!(
            result.value,
            Some(42),
            "the non-current fixed-view value is still returned to the caller"
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "a fixed view captured as NON-CURRENT MUST NOT be promoted even \
             when its captured fingerprint still matches the live one — a \
             non-current snapshot is return-only by the StoreViewRead contract"
        );
    }

    #[test]
    fn fixed_view_blocks_promotion_when_live_token_moved_since_capture() {
        // SOUNDNESS-CRITICAL fixed-view fence. A naive fixed-view path
        // returns `is_stable == true` UNCONDITIONALLY whenever a fixed
        // view is present, so a result computed against a captured view
        // would be promoted to the shared cache EVEN AFTER the live token
        // moved since the batch captured that view — a stale-cache
        // promotion. The fix gates the fixed-view `is_stable` on
        // `captured_fingerprint == live_fingerprint`: promote ONLY if the
        // live token has NOT moved since capture.
        //
        // Drive: the batch captures the view + fingerprint, then an
        // external mutation lands mid-compute (the one-shot
        // `flip_to_on_compute`, modelling e.g. a `set_default_resolve_extensions`
        // env-hash shift that moves NO epoch). At the promotion fence the
        // live fingerprint no longer matches the captured one.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            // The (one-shot) compute flips the external fingerprint
            // (env-hash shift) AFTER the batch captured 0xAAAA.
            flip_to_on_compute: Some(0xBBBB),
            flip_armed: Cell::new(true),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // The batch captures the view, its fingerprint (0xAAAA), and its
        // proven-current bit BEFORE any compute runs.
        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, true)),
            1,
        );

        // The computed value is still HANDED to the caller (return-only)…
        assert_eq!(
            result.value,
            Some(42),
            "the fixed-view value is still returned to the caller"
        );
        // …but it MUST NOT be promoted: the live token moved since the
        // batch captured the fixed view. An unconditional-`true`
        // fixed-view `is_stable` would promote here (the bug this guards).
        assert!(
            host.promotions.borrow().is_empty(),
            "a fixed-view result MUST NOT be promoted when the live token \
             moved since the batch captured the fixed view — the fixed-view \
             `is_stable` must compare captured-vs-live, NOT return true \
             unconditionally"
        );
    }

    #[test]
    fn stable_compute_promotes_result() {
        // Positive counterpart: when NO external mutation lands
        // mid-compute, the external fingerprint is unchanged across
        // snapshot → is_stable, so the result IS promoted. Proves the gate
        // is not blanket-suppressing promotion.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 3);

        assert_eq!(result.value, Some(42));
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/App.vue".to_string()],
            "a stable compute (no mid-compute external mutation) MUST promote the \
             result to the shared cache exactly once"
        );
    }

    #[test]
    fn scalar_partial_result_is_returned_but_never_promoted() {
        let host = MockHost {
            live_fp: Cell::new(PARTIAL_RESULT_FP),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // `None` selects the ordinary scalar lane. Historically only the
        // fixed/batch lane consulted per-result completeness, so this stable
        // partial was mirrored into the shared cache and reported Complete.
        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 1);

        assert_eq!(result.value, Some(42), "the partial remains return-only");
        assert!(
            result.completeness.is_partial(),
            "the scalar result must carry its typed partiality to callers and followers"
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "a stable scalar partial must not reach any shared-cache or legacy-mirror publisher"
        );
    }

    #[test]
    fn scalar_cache_refused_result_is_complete_return_only_and_never_promoted() {
        let host = MockHost {
            live_fp: Cell::new(NON_CACHEABLE_RESULT_FP),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 1);

        assert_eq!(
            result.value,
            Some(42),
            "the complete value remains returnable"
        );
        assert_eq!(
            result.completeness,
            crate::semantic_query::ResultCompleteness::Complete,
            "cache refusal is orthogonal to structural completeness"
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "a stable complete result with an owner-scoped cache refusal must never reach a publisher"
        );
    }

    #[test]
    fn incoherent_fallback_snapshot_blocks_stable_promotion() {
        // Snapshot-coherence soundness: the stability gate captures the
        // external-supersession fingerprint with pre/post discipline AROUND
        // the build inside the coherence RETRY LOOP — correct there. The
        // FALLBACK snapshot taken AFTER the loop is exhausted must apply the
        // SAME discipline; a naive fallback that read the live fingerprint
        // AFTER taking the view is unsound. A host mutation between
        // the fallback `snapshot_store_view()` and that live read would make
        // the recorded fingerprint describe the NEW host state while the
        // returned view is the OLD snapshot — and `is_stable` (re-reading
        // the now-settled live fingerprint that matches the recorded one)
        // would then promote a result computed under that stale fallback
        // snapshot.
        //
        // `flip_on_every_snapshot` advances the live fingerprint DURING
        // each `snapshot_store_view()`, so the coherence retry loop never
        // proves a coherent snapshot (pre != post on every attempt) and the
        // FALLBACK branch is taken. The fallback build suffers the same
        // straddling mutation. The fallback path applies the SAME pre/post
        // discipline, detects the straddle, marks the attempt incoherent,
        // and `is_stable` returns FALSE → no promotion. The unsound shape
        // this guards against (a live read after the snapshot) lets the
        // fingerprint settle to a value `is_stable` then matches → the
        // stale fallback result gets promoted.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            // No mid-compute flip: the divergence here is purely the
            // snapshot/fingerprint-read straddle in the fallback path.
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(true),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // `max_attempts = 1`: the executor's internal coherence loop runs
        // once (diverges), then the post-loop FALLBACK branch is taken.
        // `run_stable_request`'s outer loop also runs once, so the single
        // stability check observes the incoherent fallback attempt.
        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 1);

        // The computed value is still HANDED to the caller (return-only)…
        assert_eq!(
            result.value,
            Some(42),
            "the fallback value is still returned to the caller"
        );
        // …but it MUST NOT have been promoted: the fallback snapshot's
        // coherence was never provably established.
        assert!(
            host.promotions.borrow().is_empty(),
            "an incoherent FALLBACK snapshot (external fingerprint moved \
             across the fallback build) MUST block stable promotion — the \
             leader must NOT warm the shared cache with a result computed \
             under an unprovable snapshot taken because the coherence loop \
             could not obtain a stable one"
        );
    }

    #[test]
    fn churn_then_settle_promotes_later_coherent_attempt() {
        // Per-attempt latch reset: `fallback_snapshot_incoherent` is latched
        // `true` by the post-loop fallback path, so it MUST be reset at the
        // top of each attempt. The driver's outer loop calls `snapshot_view()`
        // up to `max_attempts` times on the SAME executor instance; without
        // the reset, a stale latch from an EARLIER attempt would wrongly
        // suppress promotion of a genuinely coherent LATER attempt's result.
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
        // true` → `is_stable` returns false → the coherent result is NOT
        // promoted → `promotions` stays empty. With the reset at the top of
        // `snapshot_view()`, the latch is cleared for the second attempt →
        // `is_stable` returns true → the result is promoted.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            // First-attempt snapshots: 2 inner coherence-loop iterations
            // (`0..max_attempts`) + 1 fallback snapshot.
            snapshot_flip_budget: Cell::new(3),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        let result = run_component_meta_request(&host, &singleflight, "/proj/App.vue", (), None, 2);

        assert_eq!(
            result.value,
            Some(42),
            "the computed value is returned to the caller"
        );
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/App.vue".to_string()],
            "a LATER outer attempt that obtains a coherent snapshot MUST \
             promote its result even though an EARLIER attempt's fallback \
             latched incoherent — each attempt's stability must reflect ONLY \
             that attempt's snapshot (per-attempt reset of \
             `fallback_snapshot_incoherent`)"
        );
    }

    #[test]
    fn non_current_fixed_snapshot_computes_once_then_returns_only() {
        // A fixed snapshot is IMMUTABLE: `snapshot_view` returns the SAME
        // view + captured fingerprint + captured currentness on every
        // attempt (it never re-reads the host). A fixed view captured as
        // NON-CURRENT therefore can NEVER become stable on a later attempt —
        // `is_stable` returns false every time. Without a fixed-snapshot
        // short-circuit the driver runs the cold compute once per attempt
        // (`max_attempts` off-lane non-current computes) PLUS once more in the
        // post-loop fallback = `max_attempts + 1` redundant computes, all
        // returning the SAME return-only value while burning projection
        // budget. The driver must instead return the FIRST fenced result
        // immediately: exactly ONE compute, no promotion.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        // Production `max_attempts` (STORE_VIEW_STABILITY_MAX_ATTEMPTS == 3).
        // Fingerprint matches at the fence, but the capture is NON-CURRENT
        // (`is_current = false`) — return-only, and immutable so retries can
        // never help.
        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, false)),
            3,
        );

        // The value is still HANDED to the caller (return-only)…
        assert_eq!(
            result.value,
            Some(42),
            "the non-current fixed-view value is still returned to the caller"
        );
        // …it is NEVER promoted (non-current capture is return-only)…
        assert!(
            host.promotions.borrow().is_empty(),
            "a non-current fixed snapshot is return-only and MUST NOT promote"
        );
        // …and crucially the cold compute ran EXACTLY ONCE. Pre-fix this is
        // `max_attempts + 1` (== 4): the off-lane non-current branch computes
        // and `continue`s every attempt, then the fallback computes once more.
        assert_eq!(
            host.computes.get(),
            1,
            "an IMMUTABLE non-current fixed snapshot can never converge to \
             stable, so the driver MUST return the first fenced (return-only) \
             result after EXACTLY ONE cold compute — not retry \
             `max_attempts + 1` times (the bug this guards)"
        );
        // The single fenced return is reported as a return-only Fallback.
        assert_eq!(result.source, RequestSource::Fallback);
    }

    #[test]
    fn fp_mismatched_fixed_snapshot_computes_once_then_returns_only() {
        // The current-but-fingerprint-mismatched fixed-snapshot variant. The
        // capture was proven current, so the attempt goes ON-lane and the
        // warm peek runs; but an external mutation landed since capture (the
        // one-shot `flip_to_on_compute` modelling e.g. an env-hash shift), so
        // at the promotion fence the captured fingerprint no longer matches
        // the live one and `is_stable` is false. A fixed snapshot is
        // immutable, so the next attempt re-presents the SAME stale capture
        // and can never match — retrying is pure waste. The driver must return
        // the first fenced result after exactly ONE compute.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            // The one-shot compute flips the live fingerprint AFTER the caller
            // captured 0xAAAA, so captured_fp (0xAAAA) != live_fp at the fence.
            flip_to_on_compute: Some(0xBBBB),
            flip_armed: Cell::new(true),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, true)),
            3,
        );

        assert_eq!(
            result.value,
            Some(42),
            "the fp-mismatched fixed-view value is still returned to the caller"
        );
        assert!(
            host.promotions.borrow().is_empty(),
            "a fixed snapshot whose live token moved since capture is \
             return-only and MUST NOT promote"
        );
        assert_eq!(
            host.computes.get(),
            1,
            "an IMMUTABLE fp-mismatched fixed snapshot can never re-match the \
             captured fingerprint on a later attempt, so the driver MUST \
             return the first fenced result after EXACTLY ONE cold compute"
        );
    }

    #[test]
    fn current_matching_fixed_snapshot_still_promotes_on_first_attempt() {
        // Happy-path guard: the fixed-snapshot short-circuit must NOT regress
        // a CURRENT + fingerprint-MATCHING fixed view. That snapshot IS
        // stable on the first attempt, so it promotes exactly once after a
        // single compute — unchanged by the no-retry optimization.
        let host = MockHost {
            live_fp: Cell::new(0xAAAA),
            flip_to_on_compute: None,
            flip_armed: Cell::new(false),
            flip_on_every_snapshot: Cell::new(false),
            snapshot_flip_budget: Cell::new(0),
            promotions: std::cell::RefCell::new(Vec::new()),
            computes: Cell::new(0),
        };
        let singleflight = SingleflightGroup::<
            ResolutionNodeKey,
            StableExecutionValue<Option<usize>>,
            (),
        >::default();

        let captured_fp = host.current_view_supersession_fingerprint();
        let result = run_component_meta_request(
            &host,
            &singleflight,
            "/proj/App.vue",
            (),
            Some((&StubView, captured_fp, true)),
            3,
        );

        assert_eq!(result.value, Some(42));
        assert_eq!(
            host.computes.get(),
            1,
            "a current + matching fixed snapshot computes once on the first \
             attempt (it is immediately stable)"
        );
        assert_eq!(
            host.promotions.borrow().as_slice(),
            ["/proj/App.vue".to_string()],
            "a current + matching fixed snapshot MUST still promote exactly \
             once — the no-retry short-circuit only fires on UNSTABLE \
             immutable snapshots, never on the stable happy path"
        );
    }
}
