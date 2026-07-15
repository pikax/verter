//! Test-only force-injection knobs grouped off the root `VerterHost`.
//!
//! These are the `#[cfg(test)]` fence / partial / non-cacheable-serve toggles
//! the in-process cache-poison and no-warm-admission tests arm to reproduce a
//! mid-flight-supersession fenced serve, a budget-truncated partial, or a
//! non-cacheable read deterministically — without a torn multi-file fixture.
//! Grouping them into one sub-struct keeps the root `VerterHost` struct thin.
//!
//! Each knob is per-host (no process-global concurrency hazard) and defaults to
//! `false`; a production build carries none of them — both this struct and its
//! `VerterHost` field are `#[cfg(test)]`, so they compile to nothing in a
//! release build.

/// Per-host test-only force-injection knobs. See the module docs for the
/// no-poison / no-warm-admission rationale each toggle exercises.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestForceKnobs {
    /// Observations of the session-wrapper operations that runtime-render
    /// compilation must bypass. Host-backed compilation is the firing control.
    pub(crate) wrapper_source_clone_count: std::sync::atomic::AtomicUsize,
    pub(crate) wrapper_cache_mode_classification_count: std::sync::atomic::AtomicUsize,
    pub(crate) wrapper_sync_transitive_count: std::sync::atomic::AtomicUsize,
    pub(crate) wrapper_store_view_read_count: std::sync::atomic::AtomicUsize,
    pub(crate) wrapper_resolver_ctx_construction_count: std::sync::atomic::AtomicUsize,
    /// Per-host test-injection knob for the carrier-subject normalization
    /// prelude. When `true`, the traced carrier-normalization prelude
    /// (`trace_carrier_subject_normalization_if_needed`) fans a synthetic
    /// FENCED (ReturnOnly) serve onto its active tracer, so the prelude
    /// finalises with `non_cacheable_read_observed = true` and forces
    /// `cache_suppress` — exercising the prelude's no-poison suppress wiring
    /// (a carrier rewrite computed from a served-without-publication artifact
    /// must refuse warm admission) without a superseded-artifact fixture. Set
    /// directly in the inline carrier-normalization test. `#[cfg(test)]`-gated:
    /// the only reader is the `#[cfg(test)]` fence injection in
    /// `trace_carrier_subject_normalization_if_needed`.
    pub(crate) carrier_normalization_force_fence_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the shared cold-build closure. When
    /// `true`, the `traced_build` closure notes a synthetic FENCED (ReturnOnly)
    /// serve onto its active tracer BEFORE the inner build runs, so the build
    /// finalises `cache_suppress = true` — the deterministic in-process
    /// equivalent of a mid-flight-supersession fenced serve. Exercises the
    /// `cache_suppress` OR-aggregation at a nested read whose subject is NOT a
    /// carrier (the ImportType qualified-path `ProjectPath`), which the
    /// prelude-scoped `carrier_normalization_force_fence_for_tests` cannot reach.
    /// Per-host (no process-global concurrency hazard). `#[cfg(test)]`-gated: the
    /// only reader is the `#[cfg(test)]` injection in `execute_via_cold_build_helper`.
    pub(crate) force_fenced_serve_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the shared cold-build closure. When
    /// `true`, the `traced_build` closure taints THIS build's frame
    /// `result_is_partial` BEFORE the inner build runs, so it finalises
    /// `result_is_partial = true` — the deterministic in-process equivalent of a
    /// budget-/recursion-truncated nested read (the carrier-preserving peel stops
    /// at an `InstantiationRef` without evaluating its args, so no authored type
    /// can naturally reach the peeled node with a `Partial` completeness).
    /// Per-host (no process-global concurrency hazard). `#[cfg(test)]`-gated: the
    /// only reader is the `#[cfg(test)]` injection in `execute_via_cold_build_helper`.
    pub(crate) force_result_partial_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the carrier-subject DIRECT serve on
    /// the EVALUATOR carrier path. When `true`, the head resolver's
    /// `ensure_indexed_ready_serve` `resolves_to_file` probe
    /// (`resolve_bare_ref_head`, carrier.rs) treats a present serve as FENCED
    /// (`store_published = false`) and fans a NON-CACHEABLE read onto every
    /// active tracer — the deterministic in-process equivalent of a
    /// mid-flight-supersession fenced serve consumed by the DIRECT carrier
    /// serve that Navigate/Skeleton/Shallow interns-and-returns WITHOUT any
    /// nested `execute_read` (an EMPTY `build_local_taint` frame, so only the
    /// evaluator-scoped nested tracer can observe it). Presence still governs
    /// `resolves_to_file`, so the production resolution shape is preserved.
    /// Placing the injection AT the direct-serve probe proves the direct serve
    /// lies inside the evaluator's nested-tracer scope. Per-host (no
    /// process-global concurrency hazard). `#[cfg(test)]`-gated: the only
    /// reader is the `#[cfg(test)]` injection at the direct-serve probe in
    /// `resolve_bare_ref_head`.
    pub(crate) force_carrier_direct_serve_fence_for_tests: std::sync::atomic::AtomicBool,
    /// When armed, [`VerterHost::ensure_indexed_ready_serve`] treats a would-be
    /// PUBLISHED serve as FENCED (`store_published = false`) and fans a
    /// NON-CACHEABLE read onto every active tracer — the deterministic in-process
    /// equivalent of a same-generation singleflight-race fenced serve, WITHOUT a
    /// `project_generation` bump (so a `GenerationSuperseded` admission gate can
    /// NOT mask the fenced-serve refusal under test). Downstream route /
    /// prepared-decl / augmentation consumers that ride
    /// `ensure_indexed_ready_serve` (`resolve_imported_registry_symbol`, the
    /// module-augmentation stitch, the framework script-fact import resolution)
    /// therefore observe the fenced serve exactly as the production
    /// mid-flight-supersession path produces it, while the served `indexed` still
    /// resolves the value (ReturnOnly). Per-host (no process-global concurrency
    /// hazard). `#[cfg(test)]`-gated: the only reader is the `#[cfg(test)]`
    /// override at the top of `ensure_indexed_ready_serve`.
    pub(crate) force_indexed_ready_serve_fence_for_tests: std::sync::atomic::AtomicBool,
    /// Number of synthetic `FileWholeHash` observations every
    /// `fact_signature_helpers::install_fact_tracer` scope fans into its
    /// freshly-installed tracer. A value above `FACT_SIGNATURE_CAP` (1024)
    /// deterministically drives EVERY traced admission boundary's tracer to
    /// finalise `FactReadSetFinalise::Overflow` — an observation set no
    /// signature can root, so a warm read could never revalidate the entry.
    /// The in-process equivalent of a compute that genuinely observes thousands
    /// of facts, without a pathological workspace fixture. Per-host (no
    /// process-global concurrency hazard). `#[cfg(test)]`-gated: the only reader
    /// is the `#[cfg(test)]` injection at the shared tracer installer.
    pub(crate) force_fact_tracer_overflow_observations: std::sync::atomic::AtomicUsize,
    /// A rendezvous every `macro_type_arg_hot_ref` demand waits on AFTER its
    /// lock-free warm-miss check and BEFORE it takes the per-slot build lock.
    /// The concurrent-first-demand singleflight test installs an N-party barrier
    /// so all N threads are DETERMINISTICALLY past the warm miss (and therefore
    /// all committed to the cold path) before any of them can build — the exact
    /// interleaving that double-lowers without the per-slot build lock. Unarmed
    /// (`None`) in every other test, where the hook is a single relaxed load.
    pub(crate) macro_hot_post_warm_miss_barrier:
        parking_lot::Mutex<Option<std::sync::Arc<std::sync::Barrier>>>,
}

/// The closed set of ADDRESSABLE tracer scopes — the scopes a test may name as
/// the target of the one-shot overflow knob below.
///
/// A tracer scope is addressable only when its production open-site passes a
/// variant of this enum through the `named_cacheability_scope!` /
/// `named_fact_tracer!` macros in
/// [`crate::fact_signature_helpers`]. Every other scope in the crate opens
/// UNNAMED and can therefore never claim a one-shot armed for someone else —
/// that is what makes the knob TARGETED rather than positional, so adding a
/// tracer scope anywhere upstream of a scope under test cannot silently
/// retarget the knob.
///
/// The enum is `#[cfg(test)]`, and so are the macro arms that mention it: a
/// production build expands the plain, unnamed scope openers and carries no
/// trace of this type.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TracerScope {
    /// The whole component-meta request cold compute. Targeting this scope
    /// proves the request-level admission rail independently of every nested
    /// cache producer: nested scopes stay cacheable while only the final
    /// resolved-meta publication is refused.
    ComponentMetaRequest,
    /// The framework script-fact entry-point's IMPORT-ROUTE resolution scope —
    /// the cacheability tracer that brackets `resolve_snapshot_imports`, whose
    /// verdict (fenced serve OR fact-signature overflow) is the ONLY thing that
    /// can refuse the resolved-fact publication built from the sibling scope
    /// below.
    ScriptFactsImportRoute,
    /// The framework script-fact entry-point's `provider.validate` scope — the
    /// signature-CONSUMING tracer whose finalised observation set becomes the
    /// published entry's `ReadSetSignature`.
    ScriptFactsProviderValidate,
}

#[cfg(test)]
thread_local! {
    /// THREAD-SCOPED one-shot sibling of
    /// [`TestForceKnobs::force_fact_tracer_overflow_observations`]: the NAMED
    /// tracer scope armed here — and ONLY that scope — consumes the count when it
    /// is next entered ON THIS THREAD, and fans that many synthetic observations
    /// into ITSELF alone; every other scope in the same flow sees zero.
    ///
    /// The always-on knob overflows EVERY tracer in a flow, which makes it
    /// non-discriminating wherever a flow installs two tracers and EITHER overflow
    /// would independently refuse the same publication — the framework script-fact
    /// entry-point is exactly that shape (an import-resolution cacheability tracer,
    /// then a sibling `provider.validate` tracer whose finalised set feeds
    /// `SignatureAdmission`). Arming the one-shot for the import scope overflows
    /// ONLY it, leaving the validation tracer cacheable, so the test proves THAT
    /// boundary's rail on its own.
    ///
    /// TARGETED, not positional. The count is claimed by scope IDENTITY
    /// ([`TracerScope`]), never by scope ORDER: an unrelated scope that happens to
    /// open first — including one newly added UPSTREAM by an unrelated change —
    /// carries no name, does not match the armed target, and leaves the one-shot
    /// armed for its intended claimant. An order-keyed one-shot would be silently
    /// retargeted by exactly that change while the test using it stayed green.
    ///
    /// THREAD-scoped, not per-host, because tracer scopes are per-thread (the
    /// tracer stack is TLS). A per-host cell would be swapped by whichever thread
    /// happened to enter the named scope first, so a concurrent test — or any test
    /// running on a shared host while another thread traces — could consume someone
    /// else's one-shot. Arming and claiming on the same thread makes the seam
    /// deterministic under concurrency. The production build compiles it out.
    static FACT_TRACER_OVERFLOW_ONCE: std::cell::Cell<Option<(TracerScope, usize)>> =
        const { std::cell::Cell::new(None) };

    /// The scope that actually CLAIMED the one-shot, recorded at the moment it
    /// fanned its synthetic observations.
    ///
    /// This is the attribution rail: a test asserts the overflow landed on the
    /// scope UNDER TEST, not merely that the one-shot was consumed *somewhere*.
    /// Cleared by [`arm_fact_tracer_overflow_once`], so a reading test always sees
    /// the claim made after its own arming.
    static FACT_TRACER_OVERFLOW_CLAIMED_BY: std::cell::Cell<Option<TracerScope>> =
        const { std::cell::Cell::new(None) };
}

/// Arm the thread-scoped one-shot overflow count FOR A NAMED SCOPE.
///
/// The count is claimed by the next entry of `scope` on this thread — not by the
/// next tracer scope to open, whatever it happens to be. Arming also clears the
/// claim record, so [`fact_tracer_overflow_claimed_by`] reports the claim made
/// after this call.
#[cfg(test)]
pub(crate) fn arm_fact_tracer_overflow_once(scope: TracerScope, count: usize) {
    FACT_TRACER_OVERFLOW_ONCE.with(|cell| cell.set(Some((scope, count))));
    FACT_TRACER_OVERFLOW_CLAIMED_BY.with(|cell| cell.set(None));
}

/// Claim the thread-scoped one-shot count on behalf of `scope`, returning the
/// count only when `scope` IS the armed target (and disarming it).
///
/// An UNNAMED scope passes `None` and can never claim. A NAMED scope that is not
/// the armed target leaves the one-shot armed for its intended claimant. The
/// claiming scope is recorded for [`fact_tracer_overflow_claimed_by`].
#[cfg(test)]
pub(crate) fn claim_fact_tracer_overflow_once(scope: Option<TracerScope>) -> usize {
    let Some(scope) = scope else {
        return 0;
    };
    FACT_TRACER_OVERFLOW_ONCE.with(|cell| match cell.get() {
        Some((armed, count)) if armed == scope => {
            cell.set(None);
            FACT_TRACER_OVERFLOW_CLAIMED_BY.with(|claimed| claimed.set(Some(scope)));
            count
        }
        _ => 0,
    })
}

/// Read the still-armed one-shot target WITHOUT claiming it — the anti-vacuity
/// check a test uses to prove the target scope actually ran (a still-armed
/// target means nothing overflowed), and to prove an unrelated upstream scope
/// did NOT steal it.
#[cfg(test)]
pub(crate) fn peek_fact_tracer_overflow_once() -> Option<(TracerScope, usize)> {
    FACT_TRACER_OVERFLOW_ONCE.with(|cell| cell.get())
}

/// The scope that claimed the one-shot since the last [`arm_fact_tracer_overflow_once`],
/// or `None` when no scope claimed it. The ATTRIBUTION oracle: a test asserts the
/// forced overflow landed on the scope under test, never merely that it landed.
#[cfg(test)]
pub(crate) fn fact_tracer_overflow_claimed_by() -> Option<TracerScope> {
    FACT_TRACER_OVERFLOW_CLAIMED_BY.with(|cell| cell.get())
}

#[cfg(test)]
impl TestForceKnobs {
    /// Block on [`Self::macro_hot_post_warm_miss_barrier`] when it is armed; a no-op
    /// single relaxed read otherwise. Called from the macro hot mirror's cold path,
    /// between its lock-free committed read and the per-slot build lock.
    ///
    /// # Non-re-entrancy invariant
    ///
    /// The mirror's cold path must NOT re-enter itself while the barrier is armed.
    /// The barrier is sized to the number of racing FIRST demands; a nested demand
    /// arriving from inside a builder would be an extra party the count does not
    /// include, and its `wait()` would block forever with no one left to release it.
    /// This holds today by construction: the cold builder produces INERT carrier
    /// nodes and resolves nothing, so it reaches no second macro-slot demand. A
    /// future builder that needs another macro's payload must take it from an
    /// already-committed slot, never by re-entering this cold path.
    pub(crate) fn wait_macro_hot_post_warm_miss_barrier(&self) {
        let barrier = self.macro_hot_post_warm_miss_barrier.lock().clone();
        if let Some(barrier) = barrier {
            barrier.wait();
        }
    }
}
