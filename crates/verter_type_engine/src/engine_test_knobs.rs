//! Engine-owned test forcing state.
//!
//! The fence / partial / non-cacheable-serve toggles, the semantic-operand
//! seams, the fact-tracer refusal injectors and the macro-hot-mirror
//! rendezvous that the semantic engine's own code reads. In-process cache-poison
//! and no-warm-admission tests arm them to reproduce a mid-flight-supersession
//! fenced serve, a budget-truncated partial or a non-cacheable read
//! deterministically, without a torn multi-file fixture.
//!
//! The record is engine-owned so the engine's injection sites read only engine
//! state: the host holds one record per host (inside its own host-specific
//! knob set) and hands it to every engine binding and macro-mirror selector.
//! Host-specific knobs stay with the host.
//!
//! Gated on `any(test, feature = "test-support")`: the engine's injection sites
//! carry the same gate, so a test build that reaches this crate as a
//! non-`cfg(test)` dependency with `test-support` enabled still exercises them.
//! A production build (feature off) carries none of it. Every knob defaults to
//! inert, so an unarmed knob leaves behaviour unchanged.

/// Per-host engine test forcing knobs. See the module docs.
#[derive(Debug, Default)]
pub struct TestKnobs {
    /// One-shot rendezvous inside the next semantic cold build. Operand
    /// concurrency tests use it to hold the winning build while an identical
    /// caller joins the graph's existing in-flight entry.
    pub semantic_operand_cold_build_seam: SeamHook,
    /// Repeating seam fired BETWEEN the two halves of the operand
    /// environment window read. Sealing tests install a real unrelated
    /// content upsert here so a genuine republication lands inside the
    /// window while every read value stays identical — the composite read
    /// the epoch bracket must reject as torn.
    pub semantic_operand_env_window_seam: SeamHook,
    /// One-shot seam fired inside the authored force AFTER its nested
    /// locator-lowering child has completed and published, and BEFORE the
    /// force finishes its own candidate. Admission tests install a
    /// cancellation or budget drain here to prove that a refusal
    /// discovered past a completed child still withholds the force's own
    /// result.
    pub semantic_operand_post_child_seam: SeamHook,
    /// Per-host test-injection knob for the carrier-subject normalization
    /// prelude. When `true`, the traced carrier-normalization prelude
    /// (`trace_carrier_subject_normalization_if_needed`) fans a synthetic
    /// FENCED (ReturnOnly) serve onto its active tracer, so the prelude
    /// finalises with `non_cacheable_read_observed = true` and forces
    /// `cache_suppress` — exercising the prelude's no-poison suppress wiring
    /// (a carrier rewrite computed from a served-without-publication artifact
    /// must refuse warm admission) without a superseded-artifact fixture. Set
    /// directly in the inline carrier-normalization test. Test-support-gated:
    /// the only reader is the gated fence injection in
    /// `trace_carrier_subject_normalization_if_needed`.
    pub carrier_normalization_force_fence_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the shared cold-build closure. When
    /// `true`, the `traced_build` closure notes a synthetic FENCED (ReturnOnly)
    /// serve onto its active tracer BEFORE the inner build runs, so the build
    /// finalises `cache_suppress = true` — the deterministic in-process
    /// equivalent of a mid-flight-supersession fenced serve. Exercises the
    /// `cache_suppress` OR-aggregation at a nested read whose subject is NOT a
    /// carrier (the ImportType qualified-path `ProjectPath`), which the
    /// prelude-scoped `carrier_normalization_force_fence_for_tests` cannot reach.
    /// Per-host (no process-global concurrency hazard). Test-support-gated: the
    /// only reader is the gated injection in `execute_via_cold_build_helper`.
    pub force_fenced_serve_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the inline flow-return executor.
    /// When `true`, every inline flow evaluation notes a synthetic FENCED
    /// (ReturnOnly) serve INSIDE its own recorded scope, so the evaluation
    /// completes with a typed, deterministic refusal of its persistent
    /// admission — the in-process equivalent of a member whose body read a
    /// served-without-publication artifact. Test-support-gated: the only
    /// reader is the gated injection in
    /// `execute_flow_return_inline`.
    pub force_flow_member_fenced_serve_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the shared cold-build closure. When
    /// `true`, the `traced_build` closure taints THIS build's frame
    /// `result_is_partial` BEFORE the inner build runs, so it finalises
    /// `result_is_partial = true` — the deterministic in-process equivalent of a
    /// budget-/recursion-truncated nested read (the carrier-preserving peel stops
    /// at an `InstantiationRef` without evaluating its args, so no authored type
    /// can naturally reach the peeled node with a `Partial` completeness).
    /// Per-host (no process-global concurrency hazard). Test-support-gated: the
    /// only reader is the gated injection in `execute_via_cold_build_helper`.
    pub force_result_partial_for_tests: std::sync::atomic::AtomicBool,
    /// Per-host test-injection knob for the shared cold-build closure: a
    /// non-empty set taints EVERY cold build's frame partial under exactly
    /// these classes before the inner build runs — the in-process
    /// equivalent of a build whose read observed them (a cancellation, a
    /// superseded generation, a torn view). Empty (the default) is inert.
    pub force_result_partial_reasons_for_tests:
        parking_lot::Mutex<crate::semantic_query::PartialReasonSet>,
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
    /// process-global concurrency hazard). Test-support-gated: the only
    /// reader is the gated injection at the direct-serve probe in
    /// `resolve_bare_ref_head`.
    pub force_carrier_direct_serve_fence_for_tests: std::sync::atomic::AtomicBool,
    /// Mark every freshly-installed tracer as having consumed one typed
    /// non-cacheable read, so EVERY traced admission boundary refuses
    /// without a pathological workspace fixture. Per-host (no process-global
    /// concurrency hazard). Test-support-gated: the only reader is the gated
    /// injection at the shared tracer installer.
    pub force_fact_tracer_non_cacheable_read: std::sync::atomic::AtomicBool,
    /// A rendezvous every `macro_type_arg_hot_ref` demand waits on AFTER its
    /// lock-free warm-miss check and BEFORE it takes the per-slot build lock.
    /// The concurrent-first-demand singleflight test installs an N-party barrier
    /// so all N threads are DETERMINISTICALLY past the warm miss (and therefore
    /// all committed to the cold path) before any of them can build — the exact
    /// interleaving that double-lowers without the per-slot build lock. Unarmed
    /// (`None`) in every other test, where the hook is a single relaxed load.
    pub macro_hot_post_warm_miss_barrier:
        parking_lot::Mutex<Option<std::sync::Arc<std::sync::Barrier>>>,
    /// Per-host count of macro-hot-mirror COLD builds (`build_macro_hot_ref`
    /// entries). The per-slot singleflight guarantee is that concurrent first
    /// demands of ONE macro collapse onto ONE cold build; a test asserts this
    /// counter is `1` after a barrier-synchronised concurrent demand burst.
    pub macro_hot_cold_builds: std::sync::atomic::AtomicUsize,
}

impl TestKnobs {
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
    pub fn wait_macro_hot_post_warm_miss_barrier(&self) {
        let barrier = self.macro_hot_post_warm_miss_barrier.lock().clone();
        if let Some(barrier) = barrier {
            barrier.wait();
        }
    }
}

/// The closed set of ADDRESSABLE tracer scopes — the scopes a test may name as
/// the target of the one-shot refusal knob below.
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
/// The enum is test-support gated, and so are the macro arms that mention it: a
/// production build expands the plain, unnamed scope openers and carries no
/// trace of this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TracerScope {
    /// The whole component-meta request cold compute. Targeting this scope
    /// proves the request-level admission rail independently of every nested
    /// cache producer: nested scopes stay cacheable while only the final
    /// resolved-meta publication is refused.
    ComponentMetaRequest,
    /// The separately-finalized output-materialization read set used by a
    /// reusable component public-contract projection witness.
    ComponentMetaOutput,
    /// The framework script-fact entry-point's IMPORT-ROUTE resolution scope —
    /// the cacheability tracer that brackets `resolve_snapshot_imports`, whose
    /// verdict (a non-cacheable read or mutation instability) is the ONLY thing
    /// that can refuse the resolved-fact publication built from the sibling
    /// scope below.
    ScriptFactsImportRoute,
    /// The framework script-fact entry-point's `provider.validate` scope — the
    /// signature-CONSUMING tracer whose finalised observation set becomes the
    /// published entry's `ReadSetSignature`.
    ScriptFactsProviderValidate,
}

thread_local! {
    /// THREAD-SCOPED one-shot sibling of
    /// [`TestKnobs::force_fact_tracer_non_cacheable_read`]: the NAMED tracer
    /// scope armed here — and ONLY that scope — claims it when it is next
    /// entered ON THIS THREAD and notes one non-cacheable read from itself;
    /// every sibling scope in the same flow claims nothing.
    ///
    /// The always-on knob refuses EVERY tracer in a flow, which makes it
    /// non-discriminating wherever a flow installs two sibling tracers and
    /// EITHER refusal would independently decline the same publication — the
    /// framework script-fact entry-point is exactly that shape (an
    /// import-resolution cacheability tracer, then a sibling
    /// `provider.validate` tracer whose finalised set feeds
    /// `SignatureAdmission`). Arming the one-shot for the import scope refuses
    /// from ONLY it, leaving the validation tracer cacheable, so the test
    /// proves THAT boundary's rail on its own.
    ///
    /// TARGETED, not positional. The one-shot is claimed by scope IDENTITY
    /// ([`TracerScope`]), never by scope ORDER: an unrelated scope that happens
    /// to open first — including one newly added UPSTREAM by an unrelated
    /// change — carries no name, does not match the armed target, and leaves
    /// the one-shot armed for its intended claimant.
    ///
    /// THREAD-scoped, not per-host, because tracer scopes are per-thread (the
    /// tracer stack is TLS). Arming and claiming on the same thread makes the
    /// seam deterministic under concurrency. The production build compiles it
    /// out.
    static FACT_TRACER_REFUSAL_ONCE: std::cell::Cell<Option<TracerScope>> =
        const { std::cell::Cell::new(None) };

    /// The scope that actually CLAIMED the one-shot, recorded at the moment it
    /// noted its refusal.
    ///
    /// This is the attribution rail: a test asserts the refusal landed on the
    /// scope UNDER TEST, not merely that the one-shot was consumed *somewhere*.
    /// Cleared by [`arm_fact_tracer_refusal_once`], so a reading test always
    /// sees the claim made after its own arming.
    static FACT_TRACER_REFUSAL_CLAIMED_BY: std::cell::Cell<Option<TracerScope>> =
        const { std::cell::Cell::new(None) };
}

/// Arm the thread-scoped one-shot refusal FOR A NAMED SCOPE.
///
/// The one-shot is claimed by the next entry of `scope` on this thread — not by
/// the next tracer scope to open, whatever it happens to be. Arming also clears
/// the claim record, so [`fact_tracer_refusal_claimed_by`] reports the claim
/// made after this call.
pub fn arm_fact_tracer_refusal_once(scope: TracerScope) {
    FACT_TRACER_REFUSAL_ONCE.with(|cell| cell.set(Some(scope)));
    FACT_TRACER_REFUSAL_CLAIMED_BY.with(|cell| cell.set(None));
}

/// Claim the thread-scoped one-shot on behalf of `scope`: `true` only when
/// `scope` IS the armed target (and disarming it).
///
/// An UNNAMED scope passes `None` and can never claim. A NAMED scope that is not
/// the armed target leaves the one-shot armed for its intended claimant. The
/// claiming scope is recorded for [`fact_tracer_refusal_claimed_by`].
pub(crate) fn claim_fact_tracer_refusal_once(scope: Option<TracerScope>) -> bool {
    let Some(scope) = scope else {
        return false;
    };
    FACT_TRACER_REFUSAL_ONCE.with(|cell| match cell.get() {
        Some(armed) if armed == scope => {
            cell.set(None);
            FACT_TRACER_REFUSAL_CLAIMED_BY.with(|claimed| claimed.set(Some(scope)));
            true
        }
        _ => false,
    })
}

/// Read the still-armed one-shot target WITHOUT claiming it — the anti-vacuity
/// check a test uses to prove the target scope actually ran (a still-armed
/// target means nothing claimed it), and to prove an unrelated upstream scope
/// did NOT steal it.
pub fn peek_fact_tracer_refusal_once() -> Option<TracerScope> {
    FACT_TRACER_REFUSAL_ONCE.with(|cell| cell.get())
}

/// The scope that claimed the one-shot since the last
/// [`arm_fact_tracer_refusal_once`], or `None` when no scope claimed it. The
/// ATTRIBUTION oracle: a test asserts the forced refusal landed on the scope
/// under test, never merely that it landed.
pub fn fact_tracer_refusal_claimed_by() -> Option<TracerScope> {
    FACT_TRACER_REFUSAL_CLAIMED_BY.with(|cell| cell.get())
}

/// Test-only callback slot; manual `Debug` because `dyn Fn` has none.
#[derive(Default)]
pub struct SeamHook(pub parking_lot::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>);

impl SeamHook {
    /// Fire an installed one-shot hook, removing it first so a hook that
    /// re-enters the same seam cannot recurse.
    pub fn fire_once(&self) {
        let hook = self.0.lock().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// Fire an installed hook WITHOUT removing it, so a bounded retry loop
    /// observes it on every attempt. The lock is released before the call:
    /// a hook that touches the host must not run under this mutex.
    pub fn fire_repeating(&self) {
        let hook = self.0.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }
}

impl std::fmt::Debug for SeamHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeamHook")
            .field("installed", &self.0.lock().is_some())
            .finish()
    }
}
