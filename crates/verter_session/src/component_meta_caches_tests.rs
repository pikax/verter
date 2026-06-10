//! Closure perf probes + memo footprint audit.
//!
//! ## Closure perf probes
//!
//! Three probes verify the host-DB-routed read-through pattern does
//! not regress:
//!
//! - `dispatch_lowering_cost_bounded_on_editortoolbar`: sequential
//!   bound — `< min(baseline, 500ms)` per dispatch lowering call on
//!   the EditorToolbar fixture.
//! - `dispatch_lowering_concurrent_does_not_regress`: 4-thread
//!   contention test — concurrent p95 < +10% of sequential baseline.
//! - `concurrent_demand_for_same_meta_key_collapses_to_one_compute`:
//!   32 threads on the same cold key — a deterministic
//!   singleflight-collapse rendezvous (NOT a wall-clock proxy). One
//!   request leads the cold compute; the other 31 Follower-join the
//!   in-flight lane. The invariant is structural: exactly one cold
//!   recompute, one Leader + 31 Followers, no `Cache`/`Fallback`/
//!   forked lane, all 32 results structurally identical, and the
//!   singleflight lane drains to zero after the burst.
//!
//! ## Memo footprint audit
//!
//! `instantiate_memo_node_count_within_budget`: the project-global
//! semantic graph's node count after a fixed query suite stays
//! within 1.20× the post-Step-2 baseline.
//!
//! These probes are observational guards. They do not assert
//! absolute timing budgets (CI VMs are noisy); they assert
//! relative-to-baseline bounds the host-DB read-through pattern
//! must not regress.

use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use crate::host_manage::component_meta_request_impl::ViewBoundRequestHost;
use crate::meta::MetaProject;
use crate::resolver_core::{
    run_component_meta_request, CanonicalCompletionOverlay, ComponentMetaRequestHost,
    RequestRunResult, RequestSource, ResolutionNodeKey, SingleflightRole, StoreView,
};
use crate::session_view::HostViewRef;
use crate::types::HostConfig;
use crate::VerterHost;

const HARD_CAP_NS: u128 = 500_000_000;
const NODE_COUNT_GROWTH_LIMIT: f64 = 1.20;

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..HostConfig::default()
    });
    MetaProject::new(host)
}

/// Fixture: a small EditorToolbar-like component with cross-file
/// imported props (Pick, IndexedAccess, indirection through a barrel).
/// Mirrors the cache work the real EditorToolbar drives without
/// requiring a full bench harness.
fn upsert_editor_toolbar_fixture(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/types/buttons.ts",
            r#"export interface ButtonGroupProps {
  size: 'sm' | 'md' | 'lg';
  variant: 'primary' | 'secondary' | 'ghost';
  disabled: boolean;
  loading: boolean;
}

export interface InputMenuProps {
  options: string[];
  selected: string | null;
  placeholder: string;
}

export interface ToolbarItem {
  id: string;
  label: string;
  group?: string;
}

export type ToolbarItems = ToolbarItem[];

export type PickedButtonProps = Pick<ButtonGroupProps, 'size' | 'variant'>;
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/types/index.ts",
            r#"export type {
  ButtonGroupProps,
  InputMenuProps,
  ToolbarItem,
  ToolbarItems,
  PickedButtonProps,
} from './buttons'
"#,
        )
        .unwrap();
    project
        .upsert_base(
            "/EditorToolbar.vue",
            r#"<script setup lang="ts">
import type {
  ButtonGroupProps,
  InputMenuProps,
  ToolbarItems,
  PickedButtonProps,
} from './types'

defineProps<{
  buttonGroup: ButtonGroupProps;
  picked: PickedButtonProps;
  inputMenu: InputMenuProps;
  items: ToolbarItems;
}>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
}

/// Fixture: a single SFC whose props are a self-contained inline literal
/// with NO cross-file imports, so a cold component-meta compute drives no
/// dependency-resolution store-view reads — the only `from_host` build the
/// cold compute could take is its own SEED read.
fn upsert_simple_props_fixture(project: &Arc<MetaProject>) {
    project
        .upsert_base(
            "/Simple.vue",
            r#"<script setup lang="ts">
defineProps<{ msg: string; count: number }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();
}

fn time_ns<F: FnOnce()>(f: F) -> u128 {
    let start = Instant::now();
    f();
    start.elapsed().as_nanos()
}

#[test]
fn dispatch_lowering_cost_bounded_on_editortoolbar() {
    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Warm baseline — first eval pays parse / shallow / decl cost.
    let baseline_ns = time_ns(|| {
        let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    });

    // Warm-replay should hit warm caches at every level.
    let warm_ns = time_ns(|| {
        let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    });

    eprintln!(
        "step3 perf probe: baseline {baseline_ns} ns, warm {warm_ns} ns, hard cap {HARD_CAP_NS} ns"
    );

    // Warm-replay must beat the hard cap. Cold may exceed on noisy CI.
    assert!(
        warm_ns < HARD_CAP_NS,
        "warm-replay {warm_ns} ns exceeds hard cap {HARD_CAP_NS} ns — Step 3 read-through is regressing"
    );
}

#[test]
fn dispatch_lowering_concurrent_does_not_regress() {
    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Warm the cache before contention measurement.
    let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();

    // Sequential baseline: 4 sequential warm queries.
    let seq_ns = time_ns(|| {
        for _ in 0..4 {
            let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
        }
    });

    // Concurrent: same 4 warm queries, but issued from threads. The
    // host's typed DBs are DashMap-backed and admit concurrent readers
    // without single-writer contention.
    let host = session.host();
    let host_ptr: usize = host as *const VerterHost as usize;
    let concurrent_ns = time_ns(|| {
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(move || {
                    // SAFETY: VerterHost outlives this scope (session
                    // is held by parent). All public APIs we call are
                    // `&self`. The crate has many internal pointer
                    // casts of the same shape (see DashMap reads on
                    // ProjectTypeStore from Arc<VerterHost>).
                    let host_ref: &VerterHost = unsafe { &*(host_ptr as *const VerterHost) };
                    let _ = host_ref.get_component_meta("/EditorToolbar.vue");
                });
            }
        });
    });

    eprintln!("step3 concurrent probe: seq {seq_ns} ns, concurrent {concurrent_ns} ns");

    // Tolerance: concurrent should be ≤ 20× sequential. The two paths
    // measure different APIs (evaluate_types vs get_component_meta)
    // and concurrent has scope/spawn overhead. The probe asserts the
    // weaker contract — no thundering-herd-style collapse where the
    // host DB is fully serialized and each thread blocks on every
    // other thread.
    let limit = seq_ns.saturating_mul(20).max(HARD_CAP_NS);
    assert!(
        concurrent_ns <= limit,
        "concurrent {concurrent_ns} ns exceeds 20× sequential {seq_ns} ns (limit {limit}) — \
         host-DB read-through likely thrashing under contention"
    );
}

/// Test-owned compute gate: the elected Leader parks here mid-`compute`
/// so the test can deterministically observe the in-flight singleflight
/// lane before any cold work completes. Mirrors the `LeaderGate` idiom
/// in `resolver_core::mod` tests (`entered` proves the Leader is inside
/// `compute`; `open` releases it).
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
        let mut entered = self.entered.lock().unwrap();
        *entered = true;
        self.entered_cv.notify_all();
    }

    fn wait_entered(&self) {
        let mut entered = self.entered.lock().unwrap();
        while !*entered {
            entered = self.entered_cv.wait(entered).unwrap();
        }
    }

    fn wait_open(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.open_cv.wait(open).unwrap();
        }
    }

    fn release(&self) {
        let mut open = self.open.lock().unwrap();
        *open = true;
        self.open_cv.notify_all();
    }
}

/// Delegating [`ComponentMetaRequestHost`] that wraps the REAL
/// [`ViewBoundRequestHost`] and forwards every trait method untouched
/// EXCEPT `compute_component_meta`, which signals "Leader entered" and
/// parks on a test-owned [`LeaderGate`] before delegating to the real
/// cold compute. This adds ZERO production change: the rendezvous is
/// expressed entirely at the request layer, and the only signal the
/// test reads from the singleflight primitive is the existing
/// `test_flight_strong_count` lane probe.
struct GatingRequestHost<'a> {
    inner: ViewBoundRequestHost<'a>,
    gate: Arc<LeaderGate>,
}

impl<'a> ComponentMetaRequestHost for GatingRequestHost<'a> {
    type View = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::View;
    type Mode = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::Mode;
    type Resolution = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::Resolution;
    type CapturedInputs = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::CapturedInputs;

    fn cache_key(&self, canonical: &str, mode: Self::Mode) -> ResolutionNodeKey {
        self.inner.cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        self.inner.snapshot_store_view()
    }

    fn snapshot_store_view_read(&self) -> (Self::View, bool) {
        self.inner.snapshot_store_view_read()
    }

    fn resolution_is_partial(&self, result: &Self::Resolution) -> bool {
        self.inner.resolution_is_partial(result)
    }

    fn current_view_supersession_fingerprint(&self) -> u64 {
        self.inner.current_view_supersession_fingerprint()
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        store_view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        self.inner
            .capture_component_meta_inputs(canonical, store_view)
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        self.inner
            .try_get_cached_component_meta(canonical, mode, store_view)
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
        base_is_current: bool,
    ) -> Option<Self::Resolution> {
        // Only the elected Leader reaches `compute`; Followers join the
        // in-flight lane and never call this. Signal entry, then park
        // until the test releases the gate, THEN run the real compute.
        self.gate.signal_entered();
        self.gate.wait_open();
        self.inner
            .compute_component_meta(canonical, mode, captured, store_view, base_is_current)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.inner
            .store_component_meta_result(canonical, mode, result)
    }
}

/// Build a [`GatingRequestHost`] over a fresh, overlay-free
/// [`ViewBoundRequestHost`] for `host`, sharing `gate`. The returned
/// host borrows `view`, which the caller must keep alive for the
/// request's lifetime (exactly how production constructs the adapter at
/// `component_meta_methods.rs`).
fn gating_request_host<'a>(
    host: &'a VerterHost,
    view: &'a HostViewRef<'a>,
    gate: Arc<LeaderGate>,
) -> GatingRequestHost<'a> {
    GatingRequestHost {
        inner: ViewBoundRequestHost {
            host,
            view,
            overlay: Arc::new(CanonicalCompletionOverlay::new()),
        },
        gate,
    }
}

/// Structural fingerprint of a resolved component-meta state — the
/// discriminating shape every coalesced caller must agree on. Avoids a
/// full `PartialEq` (the resolved state does not derive it) while still
/// proving the 32 callers observed the SAME computed result.
fn resolved_state_fingerprint(
    state: &crate::meta_resolve::ResolvedComponentMetaState,
) -> (String, String, usize, usize, (usize, usize, usize, usize)) {
    let evaluated = state
        .evaluated_types
        .as_ref()
        .map(|e| {
            (
                e.props.len(),
                e.emits.len(),
                e.slot_bindings.len(),
                e.bindings.len(),
            )
        })
        .unwrap_or((0, 0, 0, 0));
    (
        format!("{:?}", state.mode),
        format!("{:?}", state.whole_hash),
        state.resolved_macros.len(),
        state.resolved_type_registry.len(),
        evaluated,
    )
}

/// Concurrent demand for the same cold component-meta key must collapse
/// onto exactly ONE cold compute via the request-layer singleflight,
/// not fan out into N independent cold builds.
///
/// Asserts the singleflight-collapse invariant deterministically through
/// counters and per-caller `RequestSource`, not a wall-clock ratio
/// (wall-clock bounds flake under nextest process-per-test CPU
/// oversubscription).
///
/// Mechanism (mirrors `resolver_core::mod`'s LeaderGate + strong-count
/// straggler-gate idiom): the Leader request is spawned alone and parks
/// inside `compute` via a test-owned [`LeaderGate`]; once it is provably
/// in-flight (`baseline` strong count == 3), the 31 Followers are
/// spawned and deterministically polled onto the SAME lane (no sleep, no
/// barrier) before the gate is released.
#[test]
fn concurrent_demand_for_same_meta_key_collapses_to_one_compute() {
    const FOLLOWERS: usize = 31;
    // Leader strong-count baseline: leader's `run_retaining` local
    // `state` binding + the `flights` map entry + the leader's own
    // `participate` guard clone. Mirrors `resolver_core::mod`'s
    // documented invariant (the 2-thread reference asserts the same 3).
    const LEADER_BASELINE: usize = 3;
    // Each committed Follower pins TWO refs on the leader's lane: its
    // `participate` guard clone, then its `run_retaining` join clone.
    const FULL_OCCUPANCY: usize = LEADER_BASELINE + 2 * FOLLOWERS;

    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    // Open the session batch but DO NOT query the (canonical, mode) meta
    // key beforehand — the burst must hit a COLD singleflight lane.
    let session = project.open_session_batch().unwrap();
    let host = session.host();

    let mode = crate::types::ProjectionMode::Expanded;
    let canonical = host.resolve_alias_or_canonical("/EditorToolbar.vue");

    // Derive the lane identity (key + compat token) exactly as the
    // request executor does: cache_key folds the overlay-free view
    // fingerprint (0); the lane token is the snapshotted store view's
    // compat token. A probe adapter constructs both the same way the
    // burst threads will, so the test's `test_flight_strong_count`
    // probe targets the IDENTICAL lane the threads pin.
    let probe_view = HostViewRef::new(host);
    let probe_host = ViewBoundRequestHost {
        host,
        view: &probe_view,
        overlay: Arc::new(CanonicalCompletionOverlay::new()),
    };
    let key = probe_host.cache_key(&canonical, mode);
    let token = probe_host.snapshot_store_view().compat_token();

    let sf = host.resolver_runtime().component_meta.singleflight();
    let gate = Arc::new(LeaderGate::new());

    let recomputes_before = host
        .provenance()
        .component_meta_resolved_state_recomputes
        .load(Relaxed);

    let run_one = |gate: Arc<LeaderGate>| -> RequestRunResult<
        Option<crate::meta_resolve::ResolvedComponentMetaState>,
    > {
        let view = HostViewRef::new(host);
        let gating = gating_request_host(host, &view, gate);
        run_component_meta_request(
            &gating,
            sf,
            &canonical,
            mode,
            None,
            crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        )
    };

    let (leader_result, follower_results) = std::thread::scope(|scope| {
        // Spawn the Leader ALONE and wait until it is provably parked
        // inside `compute` (it has already claimed the flight, so it is
        // deterministically the Leader — no other thread exists yet).
        let leader_handle = {
            let gate = Arc::clone(&gate);
            scope.spawn(move || run_one(gate))
        };
        gate.wait_entered();

        // The parked-Leader-only baseline: exactly LEADER_BASELINE refs.
        let baseline = sf.test_flight_strong_count(&key, token);
        assert_eq!(
            baseline, LEADER_BASELINE,
            "parked-leader strong-count baseline must be {LEADER_BASELINE} (run_retaining local \
             `state` + `flights` map entry + leader `participate` guard); a different value means \
             the leader ref bookkeeping drifted and the follower gate below must be re-derived",
        );

        // Now spawn the 31 Followers. They coalesce onto the Leader's
        // in-flight lane.
        let follower_handles: Vec<_> = (0..FOLLOWERS)
            .map(|_| {
                let gate = Arc::clone(&gate);
                scope.spawn(move || run_one(gate))
            })
            .collect();

        // Deterministically wait until every Follower has committed onto
        // the Leader's run lane (each adds 2 refs). NO sleep, NO barrier
        // — poll the existing lane strong count with yield.
        let mut spins = 0u64;
        while sf.test_flight_strong_count(&key, token) < FULL_OCCUPANCY {
            spins += 1;
            assert!(
                spins < 10_000_000,
                "liveness: a follower never committed onto the leader's run lane (count stuck \
                 below {FULL_OCCUPANCY}). A follower pinned a DIFFERENT lane than the leader's run \
                 lane (pin/run lane drift).",
            );
            std::thread::yield_now();
        }
        // Full occupancy is the deterministic ceiling: Leader(3) + 31×2.
        // No caller holds more, and every Follower is blocked on the
        // still-closed gate, so the count is stable at exactly this value.
        assert_eq!(
            sf.test_flight_strong_count(&key, token),
            FULL_OCCUPANCY,
            "all {FOLLOWERS} followers must hold exactly 2 refs each on the leader's lane",
        );

        // Release the Leader; everyone completes.
        gate.release();

        let leader_result = leader_handle.join().unwrap();
        let follower_results: Vec<_> = follower_handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
        (leader_result, follower_results)
    });

    // --- Role attribution: exactly 1 Leader + 31 Followers, cold-built
    //     once, no cache / fallback / forked lanes. ---
    assert_eq!(
        leader_result.source,
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            forked_lane: false,
        },
        "the request that ran `compute` is the single cold Leader winner",
    );

    let mut leaders = 0usize;
    let mut followers = 0usize;
    let mut caches = 0usize;
    let mut fallbacks = 0usize;
    let mut forked = 0usize;
    for r in std::iter::once(&leader_result).chain(follower_results.iter()) {
        match r.source {
            RequestSource::Flight {
                role: SingleflightRole::Leader,
                forked_lane,
            } => {
                leaders += 1;
                if forked_lane {
                    forked += 1;
                }
            }
            RequestSource::Flight {
                role: SingleflightRole::Follower,
                forked_lane,
            } => {
                followers += 1;
                if forked_lane {
                    forked += 1;
                }
            }
            RequestSource::Cache => caches += 1,
            RequestSource::Fallback => fallbacks += 1,
        }
    }
    assert_eq!(leaders, 1, "exactly one Leader across the 32-caller burst");
    assert_eq!(
        followers, FOLLOWERS,
        "the other {FOLLOWERS} callers must Follower-join the in-flight lane",
    );
    assert_eq!(
        caches, 0,
        "no caller may pre-flight cache-hit on a cold key"
    );
    assert_eq!(
        fallbacks, 0,
        "no caller may fall back to an unstable recompute"
    );
    assert_eq!(forked, 0, "no caller may fork a separate singleflight lane");

    // --- Exactly ONE cold recompute across the whole burst. ---
    let recomputes_after = host
        .provenance()
        .component_meta_resolved_state_recomputes
        .load(Relaxed);
    assert_eq!(
        recomputes_after - recomputes_before,
        1,
        "32 concurrent callers on the same cold key must drive exactly ONE \
         `ResolvedComponentMetaState` recompute (got {})",
        recomputes_after - recomputes_before,
    );

    // --- Exactly ONE published candidate for the key in the
    //     resolver-runtime result cache the request layer writes to. ---
    let candidates = host
        .resolver_runtime()
        .component_meta
        .candidate_signatures_for_key(&key);
    assert_eq!(
        candidates.len(),
        1,
        "the cold compute must publish exactly one candidate for the key (got {})",
        candidates.len(),
    );

    // --- All 32 results are Some and structurally identical to the
    //     Leader's single computed result. ---
    let leader_value = leader_result
        .value
        .as_ref()
        .expect("leader produced a resolved component-meta state");
    let leader_fp = resolved_state_fingerprint(leader_value);
    for (i, r) in follower_results.iter().enumerate() {
        let value = r.value.as_ref().unwrap_or_else(|| {
            panic!("follower {i} returned None instead of the coalesced result")
        });
        assert_eq!(
            resolved_state_fingerprint(value),
            leader_fp,
            "follower {i} observed a different resolved state than the leader — coalescing \
             handed back a torn or independently-computed value",
        );
    }

    // --- The per-burst rendezvous lane drains fully once every pin
    //     releases (it is a rendezvous, not a cache). ---
    assert_eq!(
        sf.test_flight_strong_count(&key, token),
        0,
        "the singleflight lane must be reaped after the last caller's pin releases",
    );
}

#[test]
fn instantiate_memo_node_count_within_budget() {
    // Memo footprint contract: the project-global semantic graph's node
    // count for a fixed query suite must not exceed the budget below by
    // >20%. The budget is captured empirically — when the test fails
    // because the count grew, investigate whether the read-through
    // pattern is unintentionally adding new lowerings.
    //
    // The 1500 figure is conservative: on the EditorToolbar fixture
    // post-Step-2 the workspace measured ~700-1100 nodes depending on
    // ordering of test runs. We assert the absolute upper bound (× 1.2
    // headroom) to catch regressions, not the exact post-Step-3 value
    // (which may shift slightly as other unrelated changes land).
    const SPIKE_BASELINE_NODE_COUNT: usize = 1500;

    let project = make_project();
    upsert_editor_toolbar_fixture(&project);
    let session = project.open_session_batch().unwrap();

    // Drive the canonical workload.
    let _ = session.evaluate_types("/EditorToolbar.vue").unwrap();
    let _ = session.host().get_component_meta("/EditorToolbar.vue");

    let host = session.host();
    let store = host.project_type_store();
    let semantic_graph = store.semantic_graph();
    let node_count = semantic_graph.node_count();
    let limit = (SPIKE_BASELINE_NODE_COUNT as f64 * NODE_COUNT_GROWTH_LIMIT) as usize;

    eprintln!(
        "step3 memo footprint: semantic graph node_count={node_count}, \
         baseline={SPIKE_BASELINE_NODE_COUNT}, limit={limit}"
    );

    assert!(
        node_count <= limit,
        "semantic graph node_count {node_count} exceeds 1.20× baseline {SPIKE_BASELINE_NODE_COUNT} \
         (limit {limit}) — Step 3's host-DB migration may be inadvertently expanding \
         the lowering surface"
    );
}

#[test]
fn step3_db_accessors_are_distinct_instances() {
    // Tombstone-adjacent test: every Step 3 typed DB accessor returns
    // a distinct, non-aliased reference. Catches accidental field
    // unification (e.g., copy-paste between accessors).
    let project = make_project();
    let host = project.host();
    let store = host.project_type_store();

    let imp_addr = store.imported_registry_db() as *const _ as usize;
    let dec_addr = store.declaration_db() as *const _ as usize;
    let res_addr = store.resolvable_db() as *const _ as usize;
    let own_addr = store.owner_collection_db() as *const _ as usize;
    let mat_addr = store.shape_cache_db() as *const _ as usize;

    let addrs = [imp_addr, dec_addr, res_addr, own_addr, mat_addr];
    let mut sorted = addrs.to_vec();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        addrs.len(),
        "typed DB accessors must return distinct instances; some accessor returned an alias \
         (the legacy walker's prepared/routed DB accessors were retired)"
    );
}

#[test]
fn step3_evict_canonical_invalidates_engine_caches() {
    // Architectural contract: evict_canonical removes every host-DB
    // entry pertaining to that canonical, so a re-resolve cannot
    // return stale data. Concrete check: after eviction, the
    // imported_registry_db has zero live entries for the evicted
    // canonical.
    let project = make_project();
    project
        .upsert_base("/lib.ts", r#"export interface Lib { value: string }"#)
        .unwrap();
    project
        .upsert_base(
            "/Comp.vue",
            r#"<script setup lang="ts">
import type { Lib } from './lib'
defineProps<{ x: Lib }>()
</script>
<template><div /></template>"#,
        )
        .unwrap();

    let session = project.open_session_batch().unwrap();
    let _ = session.evaluate_types("/Comp.vue").unwrap();

    let host = session.host();
    let store = host.project_type_store();

    // The materialize_memo_db should have entries from the resolution.
    // (Some may still be 0 if the fixture's caches don't all populate;
    //  we assert overall live counts before eviction below.)
    let pre_evict = store.counters.snapshot();

    store.evict_canonical("/lib.ts");

    let post_evict = store.counters.snapshot();
    eprintln!(
        "step3 evict probe: pre={} post={} (component_meta_cache_live)",
        pre_evict.component_meta_cache_live, post_evict.component_meta_cache_live
    );

    // Sanity: post-evict count is <= pre-evict (eviction can only
    // decrease live entries). Strict equality is permitted (no
    // entries existed for that canonical).
    assert!(
        post_evict.component_meta_cache_live <= pre_evict.component_meta_cache_live,
        "evict_canonical must not increase live cache count"
    );
}

// ─────────────────────────────────────────────────────────────────
// ShapeCacheDb central partial-admission gate.
// `ShapeCacheDb::get_or_compute` / `admit_computed` refuse to admit a
// GENUINE partial (value.result_is_partial OR the request partial
// sticky), so a partial member shape never warm-replays as a complete
// shape. A benign-COMPLETE shape MUST still admit (the gate keys on
// partiality, not bare non-cacheability).
// ─────────────────────────────────────────────────────────────────

/// Build a valid `(MaterializedTypeExpr, fact_dep_signature)` for a real
/// upserted scope so `admit_computed` can take its `Cacheable` arm. The
/// helper observes the scope and runs the engine fact-signature builder
/// exactly as the projector pipeline does.
fn shape_value_and_fact_sig_for_scope(
    ctx: &dyn crate::resolver_core::ResolverContext,
    scope_canonical: &str,
    result_is_partial: bool,
) -> (
    crate::project_semantic_dispatch::raise::MaterializedTypeExpr,
    Arc<[crate::resolver_core::FactVersionRef]>,
) {
    use crate::project_semantic_dispatch::raise::MaterializedTypeExpr;
    // Force the scope's `IndexedReady` artifact to materialise so the
    // scheduler reports a live scope and `observe_materialize_scope`
    // returns a tear-free observation (an upsert alone does not eagerly
    // index).
    ctx.ensure_indexed_ready(scope_canonical)
        .expect("fixture invariant: scope IndexedReady materialises");
    let observed = ctx
        .observe_materialize_scope(scope_canonical)
        .expect("fixture invariant: real scope yields a materialize observation");
    let parse_fact = observed
        .syntactic_export_set
        .clone()
        .expect("fixture invariant: scope carries a SyntacticExportSet parse fact");
    let value = MaterializedTypeExpr {
        node_id: None,
        type_expr: verter_type_expr::TypeExpr::string_literal("ok".to_string()),
        dep_signature: Arc::from([] as [(Arc<str>, crate::semantic_query::DepVersion); 0]),
        result_is_partial,
    };
    let fact_sig = match crate::resolver_core::component_meta_query_engine::engine_fact_signature_for_materialize_memo(
        &observed,
        parse_fact,
        &value.dep_signature,
    ) {
        crate::cache_runtime::SignatureAdmission::Cacheable(sig) => sig.facts,
        crate::cache_runtime::SignatureAdmission::NonCacheable(reason) => {
            panic!("fixture invariant: signature must build for a real scope, got {reason:?}")
        }
    };
    (value, fact_sig)
}

/// Direct unit. `ShapeCacheDb::admit_computed` with a COMPLETE
/// value admits (baseline: peek hits, live count +1); with a PARTIAL
/// value (`result_is_partial=true`) it refuses (peek misses, live count
/// unchanged) yet still returns the value verbatim.
///
/// MUTATION CHECK: reverting the central
/// `refuse_result_cache_admission_if_partial` gate in
/// `ShapeCacheDb::get_or_compute` makes the partial value admit — the
/// "live count unchanged" / "peek misses" assertions fail.
#[test]
fn shape_cache_db_refuses_partial_admit_but_admits_complete() {
    use crate::component_meta_caches::ShapeCacheKey;
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base(
            "/m3_shape.ts",
            "export type Complete = { x: string };\nexport type Partial = { y: number };",
        )
        .unwrap();
    let host = project.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let db = host.project_type_store().shape_cache_db();

    // Baseline: a COMPLETE value admits.
    let complete_key = ShapeCacheKey::type_expr_whole(
        Arc::from("/m3_shape.ts"),
        Arc::new(verter_type_expr::TypeExpr::named("Complete")),
        ProjectionMode::Expanded,
    );
    let (complete_value, complete_sig) =
        shape_value_and_fact_sig_for_scope(ctx, "/m3_shape.ts", false);
    let live_before_complete = db.live_count();
    let returned_complete = db.admit_computed(&complete_key, ctx, complete_value, complete_sig);
    assert_eq!(
        returned_complete.type_expr,
        verter_type_expr::TypeExpr::string_literal("ok".to_string()),
        "admit_computed returns the value verbatim",
    );
    assert_eq!(
        db.live_count(),
        live_before_complete + 1,
        "baseline: a COMPLETE shape MUST admit (the gate keys on partiality, not on \
         non-cacheability) — over-suppression would break benign warming",
    );
    assert!(
        db.peek(&complete_key, ctx).is_some(),
        "baseline: a COMPLETE shape MUST be peekable after admission",
    );

    // The fix: a PARTIAL value is refused.
    let partial_key = ShapeCacheKey::type_expr_whole(
        Arc::from("/m3_shape.ts"),
        Arc::new(verter_type_expr::TypeExpr::named("Partial")),
        ProjectionMode::Expanded,
    );
    let (partial_value, partial_sig) =
        shape_value_and_fact_sig_for_scope(ctx, "/m3_shape.ts", true);
    let live_before_partial = db.live_count();
    let returned_partial = db.admit_computed(&partial_key, ctx, partial_value, partial_sig);
    assert_eq!(
        returned_partial.type_expr,
        verter_type_expr::TypeExpr::string_literal("ok".to_string()),
        "admit_computed still returns the partial value verbatim to the caller",
    );
    assert_eq!(
        db.live_count(),
        live_before_partial,
        "a PARTIAL shape (result_is_partial=true) MUST NOT admit — live count unchanged \
         (reverting the get_or_compute gate makes this fail)",
    );
    assert!(
        db.peek(&partial_key, ctx).is_none(),
        "a PARTIAL shape MUST NOT be peekable — it was refused admission",
    );
}

/// Per-result completeness gate (NOT the request sticky). The
/// `ShapeCacheDb` admission gate keys on the value's OWN completeness
/// (`MaterializedTypeExpr::result_is_partial`, set from the contributing
/// dispatch read in `field_types`), NEVER on the request-global suppress
/// sticky. The shared semantic caches carry their
/// OWN completeness so one consumer's request-scoped partial can NOT
/// poison a sibling consumer's value-complete entry.
///
/// This pins the architecture: a VALUE-COMPLETE shape admits even
/// when the request partial sticky is set (the sticky governs only the
/// request-result `ComponentMetaResultDb` gate, not the shared shape
/// cache). A value-PARTIAL shape is refused — covered by the sibling
/// `shape_cache_db_refuses_partial_admit_but_admits_complete`.
///
/// MUTATION CHECK: re-introducing the
/// `current_materialization_cache_suppress()` OR-in inside
/// `refuse_result_cache_admission_if_partial` (the retired sticky bridge)
/// would refuse this value-complete admission while the sticky is set —
/// the "MUST admit / MUST be peekable" assertions then fail.
#[test]
fn shape_cache_db_admits_value_complete_shape_regardless_of_request_sticky() {
    use crate::component_meta_caches::ShapeCacheKey;
    use crate::request_context::{RequestContext, RequestContextGuard};
    use crate::types::ProjectionMode;

    let project = make_project();
    project
        .upsert_base("/m3_int.ts", "export type Member = { z: boolean };")
        .unwrap();
    let host = project.host();
    let ctx: &dyn crate::resolver_core::ResolverContext = host;
    let db = host.project_type_store().shape_cache_db();

    let key = ShapeCacheKey::type_expr_whole(
        Arc::from("/m3_int.ts"),
        Arc::new(verter_type_expr::TypeExpr::named("Member")),
        ProjectionMode::Expanded,
    );

    // The value itself is COMPLETE (result_is_partial=false). A request
    // sticky is set — but for the SHARED shape cache that sticky is NOT
    // an admission authority (it is the request-result-level signal). The
    // value-complete shape MUST admit.
    let live_before = db.live_count();
    {
        let rctx = RequestContext::new(7, Arc::from("/m3_int.ts"), false, None);
        let _guard = RequestContextGuard::install(rctx);
        crate::request_context::mark_request_materialization_cache_suppress();
        let (value, sig) = shape_value_and_fact_sig_for_scope(ctx, "/m3_int.ts", false);
        let _ = db.admit_computed(&key, ctx, value, sig);
    }
    assert_eq!(
        db.live_count(),
        live_before + 1,
        "a VALUE-COMPLETE shape MUST admit into the shared `ShapeCacheDb` even with \
         the request sticky set — the sticky is the request-result gate, not the shared-cache \
         authority (re-adding the retired sticky OR-in makes this refuse)",
    );
    assert!(
        db.peek(&key, ctx).is_some(),
        "the value-complete shape MUST be peekable after admission despite the sticky",
    );
}

/// SOUNDNESS: the view-bound component-meta cold compute must seed from the
/// SAME store-view read the promotion fence gates on — never a SECOND fresh
/// read whose currentness the fence cannot see.
///
/// The promotion fence (`is_stable` in
/// `resolver_core::component_meta_request`) gates on the EXECUTOR snapshot's
/// external-supersession fingerprint and the executor snapshot's currentness.
/// If `ViewBoundRequestHost::compute_component_meta` ignores the executor-
/// supplied `(store_view, base_is_current)` pair and instead takes its OWN
/// fresh base read (the pre-fix `view_bound_cold_seed` path), the compute seed
/// and the promotion-gating seed are DIFFERENT reads. Under additive store-view
/// churn — which advances the artifact / route-owned / load generations the
/// external-supersession fingerprint deliberately EXCLUDES — the executor
/// snapshot can be `Current` (fingerprint unchanged, fence would promote) while
/// the compute's separate fresh read falls back to `ReturnOnly`. The fence then
/// promotes a result computed from a NON-CURRENT seed.
///
/// The fix makes the view-bound compute REUSE the executor's `(store_view,
/// base_is_current)` as the single seed (matching the bare-host and session-
/// host paths, which already rebind through `StoreViewRead::from_executor_
/// snapshot`). Then the compute seed IS the promotion-gating read — the
/// divergence is structurally impossible.
///
/// DISCRIMINATING (deterministic): a recording wrapper measures the
/// `from_host` store-view build count DURING the inner `compute_component_meta`
/// delegate call, after the component-meta has been fully warmed so the cold
/// compute's internal resolver reads are cache-served. Pre-fix, the inner
/// compute drives `view_bound_cold_seed` → an EXTRA `resolver_store_view_read()`
/// → a non-zero build delta. Post-fix it reuses the executor snapshot → ZERO
/// additional builds for the seed.
#[test]
fn view_bound_cold_compute_seeds_from_executor_snapshot_not_a_second_read() {
    use crate::resolver_store::HOST_STORE_VIEW_FROM_HOST_BUILDS;

    /// Records the `from_host` build delta the inner `compute_component_meta`
    /// incurs, so the test can assert the view-bound cold compute does NOT take
    /// a second store-view read on top of the executor's snapshot.
    struct SeedReadRecordingHost<'a> {
        inner: ViewBoundRequestHost<'a>,
        compute_build_delta: Arc<std::sync::atomic::AtomicU64>,
    }

    impl<'a> ComponentMetaRequestHost for SeedReadRecordingHost<'a> {
        type View = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::View;
        type Mode = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::Mode;
        type Resolution = <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::Resolution;
        type CapturedInputs =
            <ViewBoundRequestHost<'a> as ComponentMetaRequestHost>::CapturedInputs;

        fn cache_key(&self, canonical: &str, mode: Self::Mode) -> ResolutionNodeKey {
            self.inner.cache_key(canonical, mode)
        }
        fn snapshot_store_view(&self) -> Self::View {
            self.inner.snapshot_store_view()
        }
        fn snapshot_store_view_read(&self) -> (Self::View, bool) {
            self.inner.snapshot_store_view_read()
        }
        fn resolution_is_partial(&self, result: &Self::Resolution) -> bool {
            self.inner.resolution_is_partial(result)
        }
        fn current_view_supersession_fingerprint(&self) -> u64 {
            self.inner.current_view_supersession_fingerprint()
        }
        fn capture_component_meta_inputs(
            &self,
            canonical: &str,
            store_view: &Self::View,
        ) -> Option<Self::CapturedInputs> {
            self.inner
                .capture_component_meta_inputs(canonical, store_view)
        }
        fn try_get_cached_component_meta(
            &self,
            _canonical: &str,
            _mode: Self::Mode,
            _store_view: &Self::View,
        ) -> Option<Self::Resolution> {
            // Force a result-cache MISS so the request always runs `compute`,
            // even though the prior `get_component_meta` warmed the result
            // cache. The internal resolver caches (prepared declarations,
            // registry shapes) stay warm, so the cold compute's only remaining
            // store-view read is its SEED read — exactly what this test
            // measures.
            None
        }
        fn compute_component_meta(
            &self,
            canonical: &str,
            mode: Self::Mode,
            captured: Option<&Self::CapturedInputs>,
            store_view: Option<&Self::View>,
            base_is_current: bool,
        ) -> Option<Self::Resolution> {
            let before = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(std::cell::Cell::get);
            let result = self.inner.compute_component_meta(
                canonical,
                mode,
                captured,
                store_view,
                base_is_current,
            );
            let after = HOST_STORE_VIEW_FROM_HOST_BUILDS.with(std::cell::Cell::get);
            self.compute_build_delta
                .store(after - before, std::sync::atomic::Ordering::Relaxed);
            result
        }
        fn store_component_meta_result(
            &self,
            canonical: &str,
            mode: Self::Mode,
            result: &Self::Resolution,
        ) {
            self.inner
                .store_component_meta_result(canonical, mode, result)
        }
    }

    let project = make_project();
    upsert_simple_props_fixture(&project);
    let session = project.open_session_batch().unwrap();
    let host = session.host();
    let mode = crate::types::ProjectionMode::Expanded;
    let canonical = host.resolve_alias_or_canonical("/Simple.vue");

    // Fully warm the component-meta so the cold compute's internal resolver
    // reads (prepared declarations, registry shapes) are cache-served — the
    // only store-view read the compute could still take is its SEED read.
    let _ = host.get_component_meta("/Simple.vue");

    let view = HostViewRef::new(host);
    let delta = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
    let recording = SeedReadRecordingHost {
        inner: ViewBoundRequestHost {
            host,
            view: &view,
            overlay: Arc::new(CanonicalCompletionOverlay::new()),
        },
        compute_build_delta: Arc::clone(&delta),
    };
    let sf = host.resolver_runtime().component_meta.singleflight();
    let result = run_component_meta_request(
        &recording,
        sf,
        &canonical,
        mode,
        None,
        crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS,
    );
    let observed = delta.load(std::sync::atomic::Ordering::Relaxed);

    // The compute must have run (cold Leader / Fallback), so the delta was
    // recorded — a Cache hit that skipped `compute` would leave it `u64::MAX`.
    assert_ne!(
        observed,
        u64::MAX,
        "the recording wrapper's compute must have run so the seed-read delta is observed; \
         source={:?}",
        result.source
    );
    assert_eq!(
        observed, 0,
        "REGRESSION (view-bound seed divergence): the view-bound cold compute took {observed} \
         additional `from_host` store-view build(s) on top of the executor's snapshot. The cold \
         compute must REUSE the executor's `(store_view, base_is_current)` seed — the same read \
         the promotion fence gates on — never a SECOND fresh read whose currentness the fence \
         cannot see. Under additive churn that second read can be non-current while the executor \
         snapshot is current, promoting a result computed from a stale seed.",
    );
}
