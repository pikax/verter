//! Batch / scalar component-meta payload paths capture ONE fixed store
//! view per batch and thread it through the FENCED fixed-view fast path.
//!
//! Invariants characterized here:
//!
//! 1. **`from_host` calls are O(1), not O(N).** A warm batch of N
//!    components performs a (near-)constant number of `HostStoreView::
//!    from_host` calls — the per-batch capture, not ≥2 per item. Without
//!    the fixed view every per-job closure reads the store view at least
//!    twice (warm probe + extraction cold-seed), so the count scales ~2N.
//! 2. **Full-workspace sweeps stay O(1).** The `StoreViewManager`
//!    collapses a warm batch onto ~O(1) actual `build_coherent` sweeps —
//!    one capture per batch.
//! 3. **Fence soundness.** A result computed against a fixed view whose
//!    live token MOVED since capture is NOT promoted to the resolved-meta
//!    cache NOR the payload cache. The captured-vs-live fence rejects it;
//!    the value is still returned to the caller.
//! 4. **Batch == scalar.** The shared per-item body makes batch-payload
//!    bytes byte-identical to scalar-payload bytes for the same component.

use super::*;
use crate::types::HostConfig;
use crate::VerterHost;
use std::sync::Arc;

fn test_scheduler_config() -> verter_scheduler::scheduler::SchedulerConfig {
    // Single CPU thread: the batch fan-out runs on the calling thread, so
    // the PER-THREAD coherent-sweep counter (`warm_batch_payload_sweeps_
    // stay_o1`) reflects only this batch's sweeps. The per-HOST counters
    // the other tests measure are thread-agnostic and do not need this.
    verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    }
}

fn make_project() -> Arc<MetaProject> {
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    MetaProject::new(host)
}

const TYPES_TS: &str = r#"export interface ButtonProps { label: string; size?: 'sm' | 'md' }
export interface ButtonEmits { (e: 'click', payload: number): void }
"#;

/// Cross-file owner: `defineProps`/`defineEmits` take imported type
/// arguments, so each component's cold compute walks the import graph (a
/// real resolver workload, not an inline-literal short-circuit).
fn owner_sfc(idx: usize) -> String {
    format!(
        r#"<script setup lang="ts">
import type {{ ButtonProps, ButtonEmits }} from './types'
defineProps<ButtonProps>()
defineEmits<ButtonEmits>()
const local_{idx} = {idx}
</script>
<template><button>{{{{ local_{idx} }}}}</button></template>"#
    )
}

/// Build a project with `count` distinct cross-file components + the shared
/// `types.ts`. Returns the canonical ids in input order.
fn build_components(project: &Arc<MetaProject>, count: usize) -> Vec<String> {
    project.upsert_base("/src/types.ts", TYPES_TS).unwrap();
    let mut ids = Vec::with_capacity(count);
    for i in 0..count {
        let canonical = format!("/src/Comp{i}.vue");
        project.upsert_base(&canonical, &owner_sfc(i)).unwrap();
        ids.push(canonical);
    }
    ids
}

/// Trivial discriminating encoder: per-kind member counts. A real
/// cross-file resolution yields `props=2 events=1`; an empty / failed
/// resolution would not.
fn encode_counts(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    _resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<u8> {
    format!(
        "props={} events={}",
        analysis.props.len(),
        analysis.events.len()
    )
    .into_bytes()
}

/// (1) The warm-batch read collapse: a WARM batch of N components performs
/// O(1) `from_host` calls, NOT O(N).
///
/// HERMETIC MEASUREMENT (per-host counter): the count is read from
/// `host.provenance().store_view_from_host_reads`, a PER-`VerterHost`
/// counter bumped in the `HostStoreView::from_host_read` chokepoint (NOT
/// the process-global per-call-site attribution table). `make_project`
/// builds a fresh host, so a CONCURRENT test reading store views on a
/// DIFFERENT host can never inflate this test's reset→measure window —
/// the prior process-global measurement was a shared-process-run flake
/// source (`cargo test -p verter_session --tests`).
///
/// Discrimination: WITHOUT the per-batch fixed view each per-job closure
/// calls `resolver_store_view_read()` (→ `from_host_read`, which bumps
/// this host's counter unconditionally) at least TWICE — the payload warm
/// probe and the extraction-context cold-seed — so a warm batch of N
/// performs ~2N `from_host` calls ON THIS HOST. This test asserts the
/// warm batch's per-host delta is STRICTLY LESS THAN N (in fact ~O(1)).
/// Against a per-job-read tree the delta is ≥ N, so the `< N` assertion
/// FAILS; with the per-batch capture the single
/// `capture_batch_fixed_view` read dominates and the delta is a small
/// constant. The companion `>= 1` assertion proves the counter is LIVE
/// (the warm capture's read was counted), so a dead/unwired counter
/// cannot trivially satisfy the `< N` bound with 0.
#[test]
fn warm_batch_payload_from_host_calls_are_o1_not_per_item() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    let host = project.host();
    const N: usize = 12;
    let ids = build_components(&project, N);
    let session = project.open_session_batch().expect("batch session");

    // Cold pass: populate the resolved-meta + payload caches.
    let cold = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("cold batch dispatch");
    assert_eq!(cold.len(), N, "one slot per input");
    assert!(
        cold.iter().all(|slot| slot.is_some()),
        "every cold slot resolves to a payload",
    );

    // WARM pass: measure this host's `from_host` reads in isolation.
    host.provenance()
        .store_view_from_host_reads
        .store(0, Relaxed);
    let warm = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("warm batch dispatch");
    let warm_from_host = host.provenance().store_view_from_host_reads.load(Relaxed);

    assert_eq!(warm.len(), N);
    assert!(
        warm.iter().all(|slot| slot.is_some()),
        "every warm slot still resolves to a payload",
    );
    assert!(
        warm_from_host >= 1,
        "the warm batch MUST perform at least one real `from_host` read on \
         this host (the per-batch fixed-view capture), so a dead counter \
         cannot trivially satisfy the O(1) bound below; observed \
         {warm_from_host}",
    );
    // The discriminating bound: O(1), well under N. A per-job-read path is
    // ~2N.
    assert!(
        warm_from_host < N as u64,
        "a warm batch of N={N} must perform O(1) `from_host` calls (the one \
         per-batch fixed-view capture), NOT ≥2 per item. Observed \
         {warm_from_host} `from_host` calls on this host — a per-job-read \
         path is ~2N={} (per-job warm-probe + extraction cold-seed reads).",
        2 * N,
    );
}

/// (2) Sweeps stay O(1): a WARM batch performs ~O(1) actual full-workspace
/// `build_coherent` SWEEPS (not O(N)).
///
/// Discrimination: the sweep counter bumps once per real sweep. With the
/// single per-batch capture a warm batch sweeps at most once (often zero —
/// the `StoreViewManager` serves the cached base view). A regression that
/// re-read+rebuilt the base view per item would drive this O(N).
#[test]
fn warm_batch_payload_sweeps_stay_o1() {
    let project = make_project();
    const N: usize = 12;
    let ids = build_components(&project, N);
    let session = project.open_session_batch().expect("batch session");

    // Cold pass warms the manager's base view + the result caches.
    let _ = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("cold batch dispatch");

    // Measure the PER-THREAD sweep count, not the process-global counter.
    // With `cpu_threads: 1` the batch's `capture_batch_fixed_view` (the only
    // sweep-triggering read of a warm batch) runs on THIS calling thread, so
    // the thread-local captures exactly this batch's sweeps. The process-wide
    // counter is inflated by unrelated parallel tests' builds, which would
    // false-positive this `<= 1` bound; the thread-local never is (the same
    // robustness pattern `store_view_manager_tests` uses for its sweep
    // assertions).
    crate::resolver_store::COHERENT_BUILD_SWEEPS_THIS_THREAD.with(|c| c.set(0));
    let _ = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("warm batch dispatch");
    let warm_sweeps =
        crate::resolver_store::COHERENT_BUILD_SWEEPS_THIS_THREAD.with(std::cell::Cell::get);

    assert!(
        warm_sweeps <= 1,
        "a warm batch of N={N} must collapse onto ~O(1) full-workspace \
         sweeps (the single per-batch capture); observed {warm_sweeps} on \
         this thread. An O(N) value means the per-item path rebuilt the base \
         view.",
    );
}

/// (3) SOUNDNESS: a payload computed against a fixed view whose LIVE token
/// moved since capture is NOT promoted to EITHER the resolved-meta cache or
/// the payload cache. The value is still returned to the caller.
///
/// Drive: capture ONE `BatchFixedView` (as the batch coordinator does),
/// then advance the live external-supersession token via
/// `bump_store_view_epoch` (modelling a mid-batch external mutation), then
/// run the shared per-item body. The captured-vs-live fence in the request
/// executor must decline the resolved-meta promotion, and the
/// payload-write fence (`payload_promotion_admissible`) must decline the
/// payload-cache write.
///
/// Discrimination: against a tree whose fixed-view `is_stable` returns
/// `true` unconditionally (an unfenced fixed-view fast path) the
/// resolved-meta cache WOULD be promoted; against a tree with no payload
/// fence the payload cache WOULD be written. Both assertions FAIL on such
/// a tree. The corroborating arm proves the value is still returned.
#[test]
fn fixed_view_fence_blocks_stale_promotion_under_midbatch_token_move() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let session = project.open_session_batch().expect("batch session");
    let host = project.host();

    // Capture the fixed view FIRST (the snapshot the "batch" will compute
    // against), exactly as `get_component_meta_batch_payloads` does after
    // its prewarm. A base (empty-overlay) view captures with no overlay COW
    // — identical to the un-overlaid capture this test models.
    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);

    // An external mutation lands AFTER capture: bump the store-view epoch
    // so the live external-supersession fingerprint moves. The captured
    // fingerprint on `fixed` now no longer matches the live one.
    host.bump_store_view_epoch();

    // The payload-write fence must now decline promotion outright.
    assert!(
        !fixed.payload_promotion_admissible(host),
        "after the live token moved since capture, the fixed view must NOT \
         be payload-promotable (captured-vs-live fence)",
    );

    // Run the shared per-item body against the now-stale fixed view.
    let view = crate::session_view::HostViewRef::new(host);
    let payload = session.resolve_one_payload_item(canonical, &view, &fixed, encode_counts);

    // The value is STILL returned to the caller (return-only)…
    let payload = payload
        .expect("resolve must not error")
        .expect("the value is still returned even though promotion is fenced");
    assert_eq!(
        payload, b"props=2 events=1",
        "the returned payload reflects the real cross-file surface",
    );

    // …but NEITHER cache was warmed with the stale-snapshot result.
    let payload_cache_entry = host
        .derived_raw_cache()
        .get(canonical)
        .and_then(|e| e.value().cached_meta_payload.clone());
    assert!(
        payload_cache_entry.is_none(),
        "the payload cache MUST NOT be written when the fixed view's live \
         token moved since capture — a stale payload must not be admitted \
         (the payload-write fence)",
    );

    let resolved_meta_promoted = host
        .derived_raw_cache()
        .get(canonical)
        .map(|e| !e.value().cached_resolved_meta.is_empty())
        .unwrap_or(false);
    assert!(
        !resolved_meta_promoted,
        "the resolved-meta cache MUST NOT be promoted when the fixed view's \
         captured fingerprint no longer matches the live one — the executor's \
         captured-vs-live `is_stable` fence must decline",
    );
}

/// (3b) Positive counterpart: with NO mid-capture token move, the fixed
/// view IS promotable and the shared body DOES warm both caches — proving
/// the fence in (3) is not blanket-suppressing promotion.
#[test]
fn fixed_view_promotes_both_caches_when_token_unchanged() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let session = project.open_session_batch().expect("batch session");
    let host = project.host();

    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);
    // No token move between capture and use.
    assert!(
        fixed.payload_promotion_admissible(host),
        "a freshly-captured current fixed view must be payload-promotable",
    );

    let view = crate::session_view::HostViewRef::new(host);
    let payload = session
        .resolve_one_payload_item(canonical, &view, &fixed, encode_counts)
        .expect("resolve must not error")
        .expect("owner resolves to a payload");
    assert_eq!(payload, b"props=2 events=1");

    let payload_cached = host
        .derived_raw_cache()
        .get(canonical)
        .and_then(|e| e.value().cached_meta_payload.clone());
    assert!(
        payload_cached.is_some(),
        "with no mid-capture token move the payload cache MUST be written",
    );
    let resolved_meta_promoted = host
        .derived_raw_cache()
        .get(canonical)
        .map(|e| !e.value().cached_resolved_meta.is_empty())
        .unwrap_or(false);
    assert!(
        resolved_meta_promoted,
        "with no mid-capture token move the resolved-meta cache MUST be promoted",
    );
}

/// (4) CORRECTNESS: batch-payload bytes == scalar-payload bytes for the
/// same components. The fixed view changes only HOW reads are served (one
/// capture vs per-item), never the OUTPUTS.
#[test]
fn batch_payload_equals_scalar_payload() {
    const N: usize = 5;

    // Scalar path on its own project (fresh caches) — one call per id.
    let scalar_project = make_project();
    let scalar_ids = build_components(&scalar_project, N);
    let scalar_session = scalar_project.open_session_batch().expect("batch session");
    let scalar: Vec<Vec<u8>> = scalar_ids
        .iter()
        .map(|id| {
            scalar_session
                .get_component_meta_payload(id, encode_counts)
                .expect("scalar payload call")
                .expect("scalar payload present")
        })
        .collect();

    // Batch path on a separate project (fresh caches) — one fan-out.
    let batch_project = make_project();
    let batch_ids = build_components(&batch_project, N);
    let batch_session = batch_project.open_session_batch().expect("batch session");
    let batch: Vec<Vec<u8>> = batch_session
        .get_component_meta_batch_payloads(&batch_ids, encode_counts)
        .expect("batch dispatch")
        .into_iter()
        .map(|slot| slot.expect("batch payload present"))
        .collect();

    assert_eq!(
        batch, scalar,
        "batch-payload bytes must equal scalar-payload bytes for the same \
         components — the fixed view affects only read-sharing, not outputs",
    );
    // Discriminating content: real cross-file resolution, not empties.
    assert!(
        scalar.iter().all(|p| p == b"props=2 events=1"),
        "each component's payload must reflect the resolved cross-file \
         surface (props=2 events=1), not an empty/failed resolution",
    );
}

/// (5) The struct-returning ANALYSIS batch (`get_component_meta_batch`) is
/// on the SAME fixed-view fast path as the payload batch (no dual path): a
/// WARM analysis batch of N performs O(1) `from_host` calls, NOT O(N).
///
/// HERMETIC MEASUREMENT (per-host counter): reads
/// `host.provenance().store_view_from_host_reads` — see test (1) for why
/// the per-host counter is immune to concurrent tests' store-view traffic
/// by construction (the process-global table race fired in the
/// shared-process `cargo test -p verter_session --tests` gate).
///
/// Discrimination: the analysis path's per-job `get_component_meta_via_view`
/// previously took its own store-view reads (the warm probe + the
/// cold-seed fence) AND ran `prewarm_view_overlays` per job — every one
/// of which enters `from_host_read` on THIS host and bumps the counter.
/// Lifting the pre-warm + capture once and threading one fixed view
/// collapses the warm reads to O(1); a per-job-read analysis path is ≥ N.
/// The `< N` bound FAILS against such a path. The companion `>= 1`
/// assertion proves the counter is LIVE, so a dead/unwired counter cannot
/// trivially satisfy the bound with 0.
#[test]
fn warm_analysis_batch_from_host_calls_are_o1_not_per_item() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    let host = project.host();
    const N: usize = 12;
    let ids = build_components(&project, N);
    let session = project.open_session_batch().expect("batch session");

    // Cold pass populates the result caches.
    let cold = session
        .get_component_meta_batch(&ids)
        .expect("cold analysis batch dispatch");
    assert_eq!(cold.len(), N);
    assert!(
        cold.iter()
            .all(|slot| slot.as_ref().ok().and_then(|o| o.as_ref()).is_some()),
        "every cold analysis slot resolves",
    );

    host.provenance()
        .store_view_from_host_reads
        .store(0, Relaxed);
    let warm = session
        .get_component_meta_batch(&ids)
        .expect("warm analysis batch dispatch");
    let warm_from_host = host.provenance().store_view_from_host_reads.load(Relaxed);

    assert_eq!(warm.len(), N);
    for slot in &warm {
        let analysis = slot
            .as_ref()
            .expect("warm analysis slot ok")
            .as_ref()
            .expect("warm analysis slot present");
        assert_eq!(
            analysis.props.len(),
            2,
            "warm analysis batch must resolve the cross-file props surface",
        );
    }
    assert!(
        warm_from_host >= 1,
        "the warm analysis batch MUST perform at least one real `from_host` \
         read on this host (the per-batch fixed-view capture), so a dead \
         counter cannot trivially satisfy the O(1) bound below; observed \
         {warm_from_host}",
    );
    assert!(
        warm_from_host < N as u64,
        "a warm ANALYSIS batch of N={N} must perform O(1) `from_host` calls \
         (the one per-batch fixed-view capture), NOT >=2 per item. Observed \
         {warm_from_host} on this host — a per-job-read analysis path is >= N.",
    );
}

/// (6) The analysis path's fixed-view fence is sound: an analysis result
/// computed against a fixed view whose live token MOVED since capture is
/// NOT promoted to the `ComponentMetaResultDb`. The value is still returned.
///
/// Discrimination: routes through
/// `get_component_meta_via_view_with_fixed_store_view` with a fixed view
/// captured BEFORE a `bump_store_view_epoch`. The cold-path publish fence
/// (built from the fixed view's captured token) must decline promotion;
/// against a tree with no analysis-path fence the result WOULD be promoted.
#[test]
fn analysis_fixed_view_fence_blocks_stale_promotion() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let host = project.host();

    let view = crate::session_view::HostViewRef::new(host);
    let fixed = host.capture_batch_fixed_view(&view);
    let results_before = host.project_type_store().component_meta_results().len();
    // External mutation lands AFTER capture.
    host.bump_store_view_epoch();

    let analysis = host.get_component_meta_via_view_with_fixed_store_view(canonical, &view, &fixed);

    // Value still returned to the caller…
    let analysis = analysis.expect("analysis value is still returned even when fenced");
    assert_eq!(
        analysis.props.len(),
        2,
        "the returned analysis reflects the real cross-file surface",
    );

    // …but the result cache admitted NO new candidate (no stale-snapshot
    // promotion). A fresh project never warm-published this owner, so the
    // live candidate count must be unchanged across the fenced cold call.
    let results_after = host.project_type_store().component_meta_results().len();
    assert_eq!(
        results_after, results_before,
        "the analysis result cache MUST NOT admit a candidate when the fixed \
         view's live token moved since capture — the cold-path publish fence \
         must decline (before={results_before} after={results_after})",
    );
    // The resolved-meta cache (the executor's promotion target) must also be
    // unpromoted.
    let resolved_meta_promoted = host
        .derived_raw_cache()
        .get(canonical)
        .map(|e| !e.value().cached_resolved_meta.is_empty())
        .unwrap_or(false);
    assert!(
        !resolved_meta_promoted,
        "the resolved-meta cache MUST NOT be promoted on the fenced analysis path",
    );
}

/// `types.ts` overlay that ADDS a third prop. Overlaying the DEPENDENCY (not
/// the owner SFC itself) changes the owner's resolved prop SURFACE while
/// leaving the owner's OWN whole-hash unchanged — so a cached BASE payload's
/// owner-file fact still matches the base content, and only the cross-file
/// dep fact distinguishes base from overlay.
const TYPES_TS_OVERLAY_THREE_PROPS: &str = r#"export interface ButtonProps { label: string; size?: 'sm' | 'md'; disabled?: boolean }
export interface ButtonEmits { (e: 'click', payload: number): void }
"#;

/// SOUNDNESS: the payload warm probe must validate the cached payload against
/// the OVERLAY-aware view, not the un-overlaid base fixed view.
///
/// Scenario: a BASE payload is already cached for an owner whose props come
/// from an imported `./types` interface. A session then overlays `types.ts`
/// to add a third prop — changing the owner's resolved surface via a
/// DEPENDENCY while the owner SFC's own content (whole-hash) is unchanged.
///
/// The owner-file fact in the cached base payload therefore still matches the
/// base content; only the cross-file `types.ts` dep fact differs under the
/// overlay. A warm probe that validates against the BASE fixed view
/// (`fixed.current_view()` un-overlaid) sees `old == old` for every fact it
/// checks and returns the STALE base payload (props=2). The fix applies the
/// session overlay to the captured current view before validation (mirroring
/// the analysis warm probe), so the overlaid `types.ts` dep fact MISSES, the
/// request falls to the overlay-aware cold resolve, and the OVERLAY surface
/// (props=3) is returned.
///
/// Discrimination: pre-fix this returns `props=2 events=1` (the stale base
/// payload); post-fix it returns `props=3 events=1` (the overlay-aware
/// surface). The assertion on `props=3` FAILS against the un-overlaid-probe
/// tree.
#[test]
fn overlay_session_payload_probe_validates_against_overlaid_view_not_base() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();

    // Warm the BASE payload cache (props=2) through a base session — this is
    // the entry the overlay session's warm probe could wrongly return.
    let base_session = project.open_session_batch().expect("base session");
    let base_payload = base_session
        .get_component_meta_payload(canonical, encode_counts)
        .expect("base payload call")
        .expect("base owner resolves");
    assert_eq!(
        base_payload, b"props=2 events=1",
        "the base payload reflects the 2-prop base ButtonProps",
    );
    // The base payload IS cached (a fresh current capture promotes it).
    assert!(
        project
            .host()
            .derived_raw_cache()
            .get(canonical)
            .and_then(|e| e.value().cached_meta_payload.clone())
            .is_some(),
        "the base payload must be cached so the overlay probe has a base \
         candidate to (wrongly) validate against",
    );

    // Open an OVERLAY session and overlay the DEPENDENCY `types.ts` to add a
    // third prop. The owner SFC file is NOT overlaid, so its own whole-hash
    // is unchanged.
    let overlay_session = project.open_session().expect("overlay session");
    overlay_session
        .upsert("/src/types.ts", TYPES_TS_OVERLAY_THREE_PROPS.to_string())
        .expect("overlay types.ts");

    let overlay_payload = overlay_session
        .get_component_meta_payload(canonical, encode_counts)
        .expect("overlay payload call")
        .expect("overlay owner resolves");

    // Post-fix: the overlay-aware surface (props=3). Pre-fix: the stale base
    // payload (props=2) — the warm probe validated against the un-overlaid
    // base fixed view.
    assert_eq!(
        overlay_payload, b"props=3 events=1",
        "the overlay session MUST return the OVERLAY-aware payload (props=3 \
         from the overlaid 3-prop ButtonProps), NOT the stale base payload \
         (props=2). A warm probe that validates the cached base payload \
         against the un-overlaid `fixed.current_view()` returns the stale \
         base surface — the probe must apply the session overlay before \
         validating (mirroring the analysis warm probe).",
    );
}

/// Negative/companion: a BASE session (empty overlay) returns the cached base
/// payload on a warm probe — the overlay threading must NOT regress the
/// base-session warm-hit path. An empty overlay yields a view that validates
/// identically to the base, so the warm probe still hits and returns the
/// cached payload.
#[test]
fn base_session_payload_probe_still_warm_hits_with_empty_overlay() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let session = project.open_session_batch().expect("base session");

    // Cold pass warms the payload cache.
    let cold = session
        .get_component_meta_payload(canonical, encode_counts)
        .expect("cold payload call")
        .expect("owner resolves");
    assert_eq!(cold, b"props=2 events=1");

    // Warm pass with an EMPTY overlay must still serve the cached payload
    // (no overlay invalidates any fact) — and from the warm cache, not a
    // re-encode. `payload_encodes` is a per-host counter (each `make_project`
    // is a fresh host), so this measurement is immune to cross-test
    // pollution — no global counter is touched here.
    let host = project.host();
    let encodes_before = host
        .provenance()
        .payload_encodes
        .load(std::sync::atomic::Ordering::Relaxed);
    let warm = session
        .get_component_meta_payload(canonical, encode_counts)
        .expect("warm payload call")
        .expect("owner resolves warm");
    let encodes_after = host
        .provenance()
        .payload_encodes
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_eq!(
        warm, b"props=2 events=1",
        "a base (empty-overlay) session must still warm-hit the cached base \
         payload — the overlay threading must not regress the base path",
    );
    assert_eq!(
        encodes_before, encodes_after,
        "the warm base-session probe must hit the payload cache (no re-encode) \
         — an empty overlay yields a view that validates identically to the \
         base, so the probe must NOT miss to a cold re-encode",
    );
}

/// REGRESSION GUARD: a component-meta batch over an OVERLAY session applies
/// the session overlay ONCE per batch (O(1)), not once per job (O(N)).
///
/// The session overlay is applied by
/// [`crate::resolver_store::HostStoreView::with_session_overlay`], which on a
/// non-empty overlay CLONES the whole `StoreViewSnapshot` via `Arc::make_mut`
/// (the snapshot `Arc` is shared across the batch's jobs, so refcount > 1 →
/// full clone) and re-roots every overlaid canonical's per-domain snapshots.
/// That work is O(overlay-size); applying it per job over an N-job batch is
/// O(N) full snapshot clones + O(N·overlay) re-rooting — the measured
/// `cold 67→99s / warm 36→45s` regression.
///
/// This test runs a batch of N components over a session that overlays the
/// shared `./types` dependency, and asserts the ACTUAL overlay-COW count is
/// O(1) for the WHOLE batch on BOTH the cold and the warm pass — the single
/// per-batch capture's overlay application, shared across all N jobs.
///
/// HERMETIC MEASUREMENT (per-host counter): the COW count is read from
/// `host.provenance().session_overlay_cows`, a PER-`VerterHost` counter, NOT
/// a process-global static. Each `make_project()` builds a fresh host, so a
/// CONCURRENT test calling `with_session_overlay` on a DIFFERENT host can
/// never inflate this test's count — the prior process-global counter was a
/// CI-flake source (any parallel overlay application landed in this test's
/// reset→assert window). The per-host counter is immune by construction —
/// every counter this module measures is per-host (or per-thread for the
/// sweep test), so no cross-test serialization is needed.
///
/// DISCRIMINATING ACROSS WORKER THREADS: the overlay COWs this test must
/// observe run on the `HostBatchCoordinator`'s rayon WORKER threads (the
/// per-job closures fan out via `pool.install(|| par_iter(...))`), NOT on
/// this calling thread. A naive thread-local counter on the calling thread
/// would read 0 pre-fix (worker-blind). The per-host `AtomicU64` is the
/// right granularity: every worker overlays through the SAME host, so its
/// COW is counted regardless of which worker performed it.
///
/// Discrimination: against a per-job tree the COW count is ≥ N on the cold
/// pass (each job's cold compute re-applies the overlay at
/// `component_meta_request_impl.rs` — `from_executor_snapshot(...).
/// with_session_overlay(...)`) and ≥ N on the warm pass (each job's warm
/// probe re-applies it at `meta.rs` —
/// `current_view.clone().with_session_overlay(...)`). The `< N` bound FAILS
/// on such a tree. With the per-batch hoist the count is a SMALL CONSTANT,
/// N-independent — the per-batch `prewarm_view_overlays` + the per-batch
/// `capture_batch_fixed_view`, each applying the overlay exactly once
/// BEFORE the fan-out (so ~2 per pass, never once per job) — so the bound
/// holds for any N. The companion `>= 1` assertion proves the overlay is
/// genuinely non-empty (the per-batch setup DID perform a COW) — so a `0`
/// count cannot trivially satisfy the `< N` bound.
#[test]
fn batch_over_overlay_session_applies_overlay_o1_not_per_job() {
    use std::sync::atomic::Ordering::Relaxed;
    // The per-host overlay-COW counter makes this test's measurement
    // hermetic (see the doc comment) — no cross-test serialization needed.
    let project = make_project();
    let host = project.host();
    const N: usize = 12;
    let ids = build_components(&project, N);

    // A session that overlays the shared `./types` DEPENDENCY (adds a third
    // prop). Every owner imports `./types`, so every job's resolve walks the
    // overlaid dep — the overlay is genuinely load-bearing for all N jobs,
    // and the overlay set is non-empty so the per-batch capture performs a
    // real COW.
    let session = project.open_session_batch().expect("overlay batch session");
    session
        .upsert("/src/types.ts", TYPES_TS_OVERLAY_THREE_PROPS.to_string())
        .expect("overlay types.ts");

    // COLD pass: caches are empty, so each job runs the overlay-aware cold
    // compute. Pre-fix, each job re-applies the overlay in the cold-compute
    // seed (≥ N COWs); post-fix only the per-batch prewarm + capture apply
    // it (a small N-independent constant, ~2).
    host.provenance().session_overlay_cows.store(0, Relaxed);
    let cold = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("cold overlay batch dispatch");
    let cold_overlay_cows = host.provenance().session_overlay_cows.load(Relaxed);
    assert_eq!(cold.len(), N, "one slot per input");
    assert!(
        cold.iter()
            .all(|slot| slot.as_deref() == Some(b"props=3 events=1".as_slice())),
        "every cold slot must resolve to the OVERLAY-aware surface (props=3) \
         — confirming the shared overlaid fixed view actually carries the \
         overlay into every job; observed {cold:?}",
    );
    assert!(
        cold_overlay_cows >= 1,
        "the per-batch capture MUST perform at least one real overlay COW \
         (the overlay set is non-empty), so a `0` count cannot trivially \
         satisfy the O(1) bound below; observed {cold_overlay_cows}",
    );
    assert!(
        cold_overlay_cows < N as u64,
        "a COLD batch of N={N} over an overlay session must apply the session \
         overlay O(1) times (the single per-batch capture), NOT once per job. \
         Observed {cold_overlay_cows} overlay COWs — a per-job path re-applies \
         the overlay in each job's cold-compute seed (≥ N={N}), each a full \
         `StoreViewSnapshot` clone. Hoist the overlay into the per-batch \
         `BatchFixedView`.",
    );

    // WARM pass: the payload cache is now populated, so each job runs the
    // warm probe. Pre-fix, each job re-applies the overlay in the warm probe
    // (≥ N COWs); post-fix only the per-batch prewarm + capture apply it (a
    // small N-independent constant, ~2).
    host.provenance().session_overlay_cows.store(0, Relaxed);
    let warm = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("warm overlay batch dispatch");
    let warm_overlay_cows = host.provenance().session_overlay_cows.load(Relaxed);
    assert!(
        warm.iter()
            .all(|slot| slot.as_deref() == Some(b"props=3 events=1".as_slice())),
        "every warm slot must still resolve to the OVERLAY-aware surface \
         (props=3); observed {warm:?}",
    );
    assert!(
        warm_overlay_cows >= 1,
        "the per-batch capture MUST perform at least one real overlay COW on \
         the warm pass too; observed {warm_overlay_cows}",
    );
    assert!(
        warm_overlay_cows < N as u64,
        "a WARM batch of N={N} over an overlay session must apply the session \
         overlay O(1) times (the single per-batch capture), NOT once per job. \
         Observed {warm_overlay_cows} overlay COWs — a per-job path re-applies \
         the overlay in each job's warm probe (≥ N={N}). Hoist the overlay \
         into the per-batch `BatchFixedView`.",
    );
}

// ── Per-result completeness through the fixed-view batch executor ──

/// Sorted distinct prop names of a resolved component-meta analysis. Used by
/// the unbounded-oracle arm to assert the COMPLETE distinct surface (96
/// non-overlapping members). Name discovery is shallow/Navigate-owned, so a
/// budget trip does NOT shrink this set — see the test's contract comment.
fn prop_names(
    meta: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> std::collections::BTreeSet<String> {
    meta.props.iter().map(|p| p.name.clone()).collect()
}

/// 32 non-overlapping interfaces `S01..S32`, each `{ aNN: string; bNN:
/// number; cNN: boolean }`. Every member name is distinct across all 32
/// interfaces, so the full intersected surface is exactly 96 distinct props.
fn partial_helper_ts() -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for n in 1..=32u32 {
        let _ = writeln!(
            s,
            "export interface S{n:02} {{ a{n:02}: string; b{n:02}: number; c{n:02}: boolean }}"
        );
    }
    s
}

/// SFC whose `defineProps` type argument is `Partial<S01> & ... &
/// Partial<S32>` — a 32-arm intersection over the non-overlapping helper
/// interfaces. Enumerating prop names walks all 32 arms; under a tight
/// projection-op budget the cold materialisation trips the fuse and flags
/// the WHOLE result `completeness = Partial`.
fn partial_sfc() -> String {
    use std::fmt::Write as _;
    let mut names = String::new();
    let mut arms = String::new();
    for n in 1..=32u32 {
        if n > 1 {
            names.push_str(", ");
            arms.push_str(" & ");
        }
        let _ = write!(names, "S{n:02}");
        let _ = write!(arms, "Partial<S{n:02}>");
    }
    format!(
        "<script setup lang=\"ts\">\n\
         import type {{ {names} }} from './helper'\n\
         defineProps<{arms}>();\n\
         </script>\n\
         <template><div /></template>\n"
    )
}

/// Per-slot typed-completeness facts recovered through the fixed-view batch
/// PAYLOAD path. `get_component_meta_batch` does not expose the resolved
/// sidecar, so the batch's typed completeness is observed via
/// `get_component_meta_batch_payloads` with the encoder below, which records
/// `resolved.completeness.is_partial()` and `resolved.synthesis_should_suppress`
/// for the ACTUAL fixed-view batch result (not a separate scalar read).
struct SlotCompleteness {
    is_partial: bool,
    synthesis_should_suppress: bool,
    prop_count: usize,
}

/// Encoder for `get_component_meta_batch_payloads`: serialises the typed
/// completeness flags + prop count of the batch result so the test can
/// recover them per slot.
fn encode_completeness(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<u8> {
    format!(
        "is_partial={} suppress={} props={}",
        resolved.completeness.is_partial(),
        resolved.synthesis_should_suppress,
        analysis.props.len(),
    )
    .into_bytes()
}

fn parse_completeness(slot: &Option<Vec<u8>>) -> SlotCompleteness {
    let bytes = slot
        .as_ref()
        .expect("slot returns a payload (not swallowed)");
    let text = std::str::from_utf8(bytes).expect("payload is utf8");
    let mut is_partial = None;
    let mut synthesis_should_suppress = None;
    let mut prop_count = None;
    for field in text.split_whitespace() {
        let (key, val) = field.split_once('=').expect("field is key=value");
        match key {
            "is_partial" => is_partial = Some(val == "true"),
            "suppress" => synthesis_should_suppress = Some(val == "true"),
            "props" => prop_count = Some(val.parse().expect("props is a number")),
            other => panic!("unexpected payload field {other}"),
        }
    }
    SlotCompleteness {
        is_partial: is_partial.expect("is_partial present"),
        synthesis_should_suppress: synthesis_should_suppress.expect("suppress present"),
        prop_count: prop_count.expect("props present"),
    }
}

/// A budget-exhausted, UNCERTIFIED PARTIAL driven through the FIXED-VIEW
/// BATCH executor is returned to the caller but NEVER admitted/promoted to
/// the shared resolved-meta cache, while a COMPLETE sibling in the SAME
/// batch is admitted and stays warm.
///
/// THE CONTRACT (the no-poison / completion fence): a budget trip during the
/// cold materialisation makes the result UNCERTIFIED — the fuse interrupted
/// materialisation before natural completion — so the WHOLE result is flagged
/// `completeness = Partial` (and its bool projection
/// `synthesis_should_suppress = true`). An uncertified `Partial` is REFUSED
/// warm admission, REGARDLESS of how large or small the published prop-NAME
/// surface is. The discriminator is TYPED COMPLETENESS, NOT a name/prop count:
/// the surface size is incidental and is NOT a certification of completeness.
///
/// The payload path shares ONE projection budget across the cold resolve AND
/// the extract (the same single-context shape as the analysis surface), so a
/// pathological owner whose resolve EXHAUSTS the budget enumerating its
/// defineProps surface leaves the extract starved — the constrained result
/// publishes a budget-TRUNCATED surface (here zero props cold), exactly as the
/// analysis surface does for the same owner under the same budget. The
/// no-poison invariant holds independent of that surface size: the uncertified
/// Partial is still returned and still refused admission.
///
/// The two promotion rails are conjunctive (a result must be BOTH complete
/// AND token-stable/current to promote) and the completeness rail is
/// PER-RESULT: one job's genuine partial must not poison its complete
/// sibling's admission, and the sibling's admitted entry must warm a
/// follow-up batch.
///
/// REPEAT-BATCH semantics (per-result, not "stays partial forever"): the
/// projection-op budget trip is NON-deterministic across warm-cache repeats
/// BY DESIGN — the first cold compute warms the shared resolver memos (the
/// content-addressed macro-hot mirror + per-arm Instantiate/structure
/// memos), so a repeat recompute charges far fewer projection ops and may
/// complete UNDER the same budget, yielding a GENUINE Complete result. That
/// is valid healing, not laundering. The repeat-batch invariant is therefore
/// per-result: if the repeat result is STILL observed Partial it must STILL
/// be refused admission; if it recomputes Complete, admitting it is correct.
///
/// Fixture: a 32-arm `Partial<S01> & ... & Partial<S32>` intersection over
/// non-overlapping interfaces (96 distinct props) under a tight
/// `projection_op_budget = 6` — the top-level defineProps type argument must
/// be expanded to enumerate the surface, each distinct arm is a distinct
/// projection-dispatch cold build, and the budget trips mid-materialisation.
/// The plain-literal sibling completes within the same budget. The unbounded
/// oracle (`projection_op_budget = 100_000`) resolves the same owner as a
/// genuine Complete result — proving the constrained Partial is a real budget
/// trip, not a structurally-incomplete source.
#[test]
fn batch_partial_returned_never_admitted_while_complete_sibling_warms() {
    use crate::types::AnalysisLevel;

    let helper_ts = partial_helper_ts();
    let partial_sfc = partial_sfc();
    const SIMPLE_SFC: &str = r#"<script setup lang="ts">
defineProps<{ icon: string; label: number }>();
</script>
<template><div /></template>
"#;

    // ── Unbounded oracle: the SAME owner resolves as a genuine COMPLETE
    // result (96 distinct props, not Partial). This proves the constrained
    // host's Partial below is a real budget trip — not a structurally
    // incomplete source. The scalar `get_component_meta_with_resolution` is
    // the correct probe HERE because the unbounded host never trips the
    // fuse, so it returns Ok (the constrained host would return Err for the
    // partial slot — hence the batch-payload encoder for that one).
    let oracle_host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            projection_op_budget: 100_000,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    let oracle_project = MetaProject::new(oracle_host);
    oracle_project
        .upsert_base("/src/helper.ts", &helper_ts)
        .unwrap();
    oracle_project
        .upsert_base("/src/Partial.vue", &partial_sfc)
        .unwrap();
    let oracle_session = oracle_project
        .open_session_batch()
        .expect("oracle batch session");
    let (complete_meta, complete_resolution) = oracle_session
        .get_component_meta_with_resolution("/src/Partial.vue")
        .expect("oracle query ok")
        .expect("oracle resolves");
    let complete_names = prop_names(&complete_meta);
    assert_eq!(
        complete_names.len(),
        96,
        "the non-overlapping 32-arm intersection has exactly 96 distinct props",
    );
    assert!(
        !complete_resolution.completeness.is_partial(),
        "unbounded oracle must be Complete",
    );
    assert!(
        !complete_resolution.synthesis_should_suppress,
        "bool projection must agree with the Complete oracle",
    );

    // ── Constrained host: a tight projection-op budget. ──
    let host = VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            projection_op_budget: 6,
            ..HostConfig::default()
        },
        test_scheduler_config(),
    );
    let project = MetaProject::new(host);
    project.upsert_base("/src/helper.ts", &helper_ts).unwrap();
    project
        .upsert_base("/src/Partial.vue", &partial_sfc)
        .unwrap();
    project.upsert_base("/src/Simple.vue", SIMPLE_SFC).unwrap();
    let ids = vec![
        "/src/Partial.vue".to_string(),
        "/src/Simple.vue".to_string(),
    ];

    let host = project.host();
    // The session/batch analysis path admits into the shared RESOLVED-meta
    // cache (the `cached_resolved_meta` slot on DerivedRawState plus the
    // resolver-runtime mirror) — `ComponentMetaResultDb` is the BASE
    // `get_component_meta` cache and is not written by this path. The
    // admission oracle therefore reads the resolved-meta slot.
    let result_admitted = |canonical: &str| {
        host.derived_raw_cache()
            .get(canonical)
            .map(|e| !e.value().cached_resolved_meta.is_empty())
            .unwrap_or(false)
    };

    let session = project.open_session_batch().expect("batch session");

    // ── Batch 1 (cold): observe typed completeness on the ACTUAL fixed-view
    // batch path via the payload encoder; only the complete one admits. ──
    let payloads1 = session
        .get_component_meta_batch_payloads(&ids, encode_completeness)
        .expect("batch 1 payload dispatch");
    let partial1 = parse_completeness(&payloads1[0]);
    let simple1 = parse_completeness(&payloads1[1]);

    assert_eq!(
        simple1.prop_count, 2,
        "the complete sibling resolves its full 2-prop surface",
    );
    assert!(
        partial1.prop_count < 96,
        "the constrained partial publishes a budget-TRUNCATED surface (observed \
         zero props cold), NOT the full 96 — the payload extract shares the \
         resolve's projection budget, which the 32-arm enumeration exhausts, so \
         the surface is truncated exactly as the analysis surface truncates the \
         same owner under the same budget; this is a typed-completeness fixture, \
         NOT a name-count one (got {})",
        partial1.prop_count,
    );
    assert!(
        partial1.is_partial,
        "the tight projection-op budget must trip the 32-arm intersection \
         mid-materialisation — an UNCERTIFIED, budget-tripped Partial result. \
         The published name surface size is incidental (the budget-bound extract \
         truncates it): the discriminator is typed completeness. If this is not \
         Partial the fuse no longer trips and the fixture stops exercising the \
         no-poison producer.",
    );
    assert_eq!(
        partial1.synthesis_should_suppress, partial1.is_partial,
        "synthesis_should_suppress must remain the bool projection of typed \
         completeness",
    );
    assert!(
        !result_admitted("/src/Partial.vue"),
        "an uncertified PARTIAL through the fixed-view batch executor must be \
         returned but NEVER admitted to the shared resolved-meta cache",
    );
    assert!(
        !simple1.is_partial,
        "the complete sibling must stay Complete",
    );
    assert!(
        result_admitted("/src/Simple.vue"),
        "the COMPLETE sibling in the same batch must be admitted — one job's \
         partial must not poison the sibling's per-result completeness",
    );

    // ── Batch 2 (repeat): the per-result no-poison invariant. The complete
    // sibling stays warm. The partial re-runs; because batch 1 warmed the
    // shared resolver memos the recompute MAY complete under the same budget
    // (valid healing). The invariant is per-result: a result STILL observed
    // Partial must STILL be refused admission; a genuine Complete recompute
    // may be admitted — that is NOT partial laundering. ──
    let payloads2 = session
        .get_component_meta_batch_payloads(&ids, encode_completeness)
        .expect("batch 2 payload dispatch");
    let partial2 = parse_completeness(&payloads2[0]);
    let simple2 = parse_completeness(&payloads2[1]);

    assert_eq!(
        partial2.synthesis_should_suppress, partial2.is_partial,
        "synthesis_should_suppress stays the bool projection of typed \
         completeness on the repeat batch",
    );
    if partial2.is_partial {
        assert!(
            partial2.prop_count < 96,
            "a repeat-batch result still Partial publishes a budget-truncated \
             surface — the budget-bound extract truncates it on the repeat too, \
             NOT the full 96 (got {})",
            partial2.prop_count,
        );
        assert!(
            !result_admitted("/src/Partial.vue"),
            "a repeat-batch result that is STILL Partial must STILL be refused \
             admission — the no-poison invariant",
        );
    } else {
        assert!(
            result_admitted("/src/Partial.vue"),
            "a repeat-batch GENUINE Complete recompute may be admitted; this is \
             valid healing through shared resolver memo warmth, NOT partial \
             laundering",
        );
    }
    assert!(!simple2.is_partial, "the warm sibling stays Complete");
    assert!(
        result_admitted("/src/Simple.vue"),
        "the complete sibling's admitted entry must survive the repeat batch \
         (it stays warm)",
    );
}

/// Full-surface correctness encoder: serialises every resolved
/// prop/event/slot/exposed name AND its resolved type shape. Unlike
/// `encode_counts` (which only counts members) this exercises the data
/// carried through the shared `ScriptAnalysisSnapshot` — a corruption that
/// dropped or aliased fields when the snapshot moved behind an `Arc` would
/// change these bytes.
fn encode_full_surface(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    _resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut out = String::new();
    for p in &analysis.props {
        let _ = writeln!(
            out,
            "prop {} req={} default={} type={:?}",
            p.name, p.required, p.has_default, p.type_expr
        );
    }
    for e in &analysis.events {
        let _ = writeln!(out, "event {} payload={:?}", e.name, e.payload);
    }
    for s in &analysis.slots {
        let _ = writeln!(out, "slot {} scoped={}", s.name, s.is_scoped);
    }
    for x in &analysis.exposed {
        let _ = writeln!(out, "exposed {}", x.name);
    }
    out.into_bytes()
}

/// THE WIN: the dependency-resolution read path SHARES the one
/// `ScriptAnalysisSnapshot` allocation by `Arc` instead of deep-copying it.
///
/// `effective_file_state()` is read on every dependency-resolution hop
/// (`analysis_source_exists`, `current_eval_state_uncached`,
/// `ensure_indexed_ready`, the fallthrough/css-flow probes, …) —
/// per-dependency × per-component during a resolution pass. When
/// `ScriptAnalysisSnapshot` was stored by value, each of those reads
/// deep-copied ~18 owned vectors only to read one or two scalar fields and
/// drop the rest; a `sample(1)` profile attributed ~65 % of the batch's CPU to
/// this clone+drop. Sharing the snapshot by `Arc` turns the read into a
/// refcount bump: every `effective_file_state` read for the same parse
/// generation hands back the SAME underlying allocation.
///
/// Mechanism: call `effective_file_state(canonical, None)` TWICE for the same
/// canonical at the same parse generation and assert the two returned
/// `script_analysis` handles are `Arc::ptr_eq` — i.e. they are clones of ONE
/// allocation, the source `Arc<ScriptAnalysisSnapshot>` held in the
/// scheduler's `HostSourceData`, not two independent deep copies. The owner
/// SFC carries real imports + macros + bindings, so its snapshot owns
/// non-empty vectors; a by-value read would have to materialise a fresh deep
/// copy each call, and two such copies can never be `ptr_eq`.
///
/// Discrimination (by construction, hermetic, deterministic): pre-fix the
/// field was `script_analysis: ScriptAnalysisSnapshot` BY VALUE on both
/// `ParseSnapshot` and `EffectiveFileState`, so `effective_file_state` built a
/// FRESH owned snapshot on every call (`hd.parse.script_analysis.clone()` =
/// field-by-field deep copy). Two reads therefore returned two distinct
/// allocations at two distinct addresses — `Arc::ptr_eq` is impossible (the
/// field was not even an `Arc`, so the post-fix assertion cannot hold against
/// the pre-fix shape). Post-fix the field is `Arc<ScriptAnalysisSnapshot>`
/// sourced once at parse; both reads `Arc::clone` the SAME source handle, so
/// the two pointers are equal and the assertion passes. This needs no clone
/// counter, no thread coordination, and no process-global state, so it cannot
/// under-measure work that ran on a coordinator-pool worker thread.
///
/// A corroborating resolution arm runs a real cross-file component-meta batch
/// (which reads `effective_file_state` on every hop) BETWEEN the two reads, to
/// prove the shared snapshot is exactly the one the hot resolution path
/// consumes — and that resolution does not swap the source out from under the
/// shared `Arc`.
#[test]
fn effective_file_state_shares_script_analysis_arc_across_reads() {
    let project = make_project();
    // A single cross-file owner is enough: the invariant under test is per
    // file (one canonical, one parse generation). The owner imports `./types`
    // and runs `defineProps`/`defineEmits`, so its snapshot owns real
    // imports/macros/bindings vectors — a by-value read would deep-copy them.
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let host = project.host();

    // FIRST read of the shared snapshot for this parse generation.
    let first = host
        .effective_file_state(canonical, None)
        .expect("owner is in the scheduler after upsert");
    // The owner's snapshot is genuinely non-trivial — so "share, don't copy"
    // is load-bearing here, and a deep copy would be expensive.
    assert!(
        !first.script_analysis.imports.is_empty() && !first.script_analysis.macros.is_empty(),
        "the owner SFC must carry real imports + macros so the shared snapshot \
         owns non-empty vectors (a by-value read would deep-copy them); \
         imports={} macros={}",
        first.script_analysis.imports.len(),
        first.script_analysis.macros.len(),
    );

    // A REAL cross-file resolution runs between the two reads. It reads
    // `effective_file_state` on every dependency hop — the exact hot path this
    // fix turns into a refcount bump. It must not re-root the source snapshot.
    let session = project.open_session_batch().expect("batch session");
    let resolved = session
        .get_component_meta_payload(canonical, encode_counts)
        .expect("resolution must not error")
        .expect("owner resolves its cross-file surface");
    assert_eq!(
        resolved, b"props=2 events=1",
        "the owner must resolve its imported `ButtonProps` (2) + `ButtonEmits` \
         (1) cross-file surface — proving the resolution actually walked the \
         import graph that reads `effective_file_state`",
    );

    // SECOND read, same canonical, same parse generation (no upsert/edit since
    // the first read, so the scheduler still holds the same source `Arc`).
    let second = host
        .effective_file_state(canonical, None)
        .expect("owner is still in the scheduler");

    // THE INVARIANT: both reads handed back clones of ONE allocation — the
    // source `Arc<ScriptAnalysisSnapshot>`. Pre-fix (by-value field) each read
    // deep-copied a fresh snapshot, so this could never hold; post-fix the
    // shared `Arc` makes the two pointers equal.
    assert!(
        Arc::ptr_eq(&first.script_analysis, &second.script_analysis),
        "two `effective_file_state` reads of the same canonical at the same \
         parse generation MUST return `Arc::ptr_eq` `script_analysis` handles \
         — i.e. clones of the ONE source `Arc<ScriptAnalysisSnapshot>`, not two \
         independent ~18-vector deep copies. They are NOT pointer-equal: the \
         read path materialises a fresh owned snapshot per call. Store \
         `script_analysis` as `Arc<ScriptAnalysisSnapshot>` on `ParseSnapshot` \
         + `EffectiveFileState` and `Arc::clone` it on the read path so a \
         caller that reads one scalar bumps a refcount instead of deep-copying \
         every vector.",
    );
}

/// Correctness is unchanged by the `Arc` share: the FULL resolved surface
/// (every prop/event/slot/exposed name + resolved type) is byte-identical
/// between a cold compute and a warm read, AND matches the expected
/// cross-file resolution. Sharing the snapshot must change only the copy
/// behaviour, never the resolved data.
#[test]
fn arc_shared_snapshot_preserves_full_meta_surface_bytes() {
    let project = make_project();
    const N: usize = 4;
    let ids = build_components(&project, N);
    let session = project.open_session_batch().expect("batch session");

    let cold = session
        .get_component_meta_batch_payloads(&ids, encode_full_surface)
        .expect("cold batch dispatch");
    let warm = session
        .get_component_meta_batch_payloads(&ids, encode_full_surface)
        .expect("warm batch dispatch");

    assert_eq!(cold.len(), N);
    assert_eq!(warm.len(), N);
    for (i, (c, w)) in cold.iter().zip(warm.iter()).enumerate() {
        let c = c.as_deref().expect("cold payload present");
        let w = w.as_deref().expect("warm payload present");
        assert_eq!(
            c,
            w,
            "component {i}: warm full-surface bytes diverged from cold — sharing \
             the snapshot by `Arc` must not alter resolved meta.\ncold={}\nwarm={}",
            String::from_utf8_lossy(c),
            String::from_utf8_lossy(w),
        );
    }

    // The cross-file resolution must actually have happened: each component
    // imports `ButtonProps` (2 props) + `ButtonEmits` (1 event) from
    // `./types`. A blank surface would byte-match cold==warm yet prove nothing,
    // so pin the expected content too.
    let first = cold[0].as_deref().expect("first payload present");
    let text = String::from_utf8_lossy(first);
    assert!(
        text.contains("prop label ") && text.contains("prop size "),
        "expected the imported `ButtonProps` members (label, size) in the \
         resolved surface; got:\n{text}",
    );
    assert!(
        text.contains("event click "),
        "expected the imported `ButtonEmits` event (click) in the resolved \
         surface; got:\n{text}",
    );
}

/// The overlay-set fingerprint is computed ONCE per batch, not once per
/// component / per `cache_key` / per warm-probe / per cache store.
///
/// `SessionView::fingerprint` folds into the resolved-meta singleflight
/// key (`cache_key`), the view-aware warm-cache probe, and the cache
/// store, so a per-component recompute calls it 3+ times per component.
/// Each full computation collects every overlay entry into a `Vec`, sorts
/// it by canonical string, and FxHashes it — O(overlay·log overlay) — yet
/// for an overlay set that is IMMUTABLE for the view's lifetime it always
/// returns the IDENTICAL `u64`. An overlay-bearing view is constructed
/// ONCE per batch (one `with_overlay_view` over the whole batch), so the
/// full computation must run a SMALL N-INDEPENDENT CONSTANT number of
/// times for a batch of N, not O(N).
///
/// HERMETIC MEASUREMENT (per-host counter): the count is read from
/// `host.provenance().overlay_set_fingerprint_full_computations`, a
/// PER-`VerterHost` counter (NOT a process-global static). `make_project`
/// builds a fresh host, so a CONCURRENT test fingerprinting a DIFFERENT
/// host can never inflate this test's count — every counter this module
/// measures is per-host (or per-thread for the sweep test), so no
/// cross-test serialization is needed.
///
/// DISCRIMINATING ACROSS WORKER THREADS: the `fingerprint()` reads run on
/// the `HostBatchCoordinator`'s rayon WORKER threads (`cache_key` /
/// warm-probe / store all execute inside the per-job closures), NOT on
/// this calling thread. A naive thread-local counter on the calling
/// thread would read 0 pre-fix (worker-blind). The per-host `AtomicU64`
/// is the right granularity: every worker reads the SAME shared view's
/// fingerprint through the SAME host.
///
/// Discrimination: against a per-call (un-memoized) tree the count is
/// ≥ N (each job's `cache_key` recomputes the full sort+hash once, plus
/// the warm-probe and store paths), so the `< N` bound FAILS. With the
/// per-view memo the count is a SMALL CONSTANT, N-independent — the view
/// is constructed once per `with_overlay_view`, the analysis-path
/// `get_analysis_via_view` prewarm aside, so the bound holds for any N.
/// The companion `>= 1` assertion proves the overlay is genuinely
/// non-empty (a full computation DID run) — so a `0` count cannot
/// trivially satisfy the `< N` bound.
#[test]
fn batch_over_overlay_session_computes_fingerprint_o1_not_per_job() {
    use std::sync::atomic::Ordering::Relaxed;
    // The per-host fingerprint-computation counter makes this test's
    // measurement hermetic (see the doc comment) — no cross-test
    // serialization needed.
    let project = make_project();
    let host = project.host();
    const N: usize = 12;
    let ids = build_components(&project, N);

    // A session that overlays the shared `./types` DEPENDENCY (adds a
    // third prop). The overlay set is non-empty, so a full overlay-set
    // fingerprint computation actually walks the sort+hash body (the
    // empty short-circuit returns 0 without counting).
    let session = project.open_session_batch().expect("overlay batch session");
    session
        .upsert("/src/types.ts", TYPES_TS_OVERLAY_THREE_PROPS.to_string())
        .expect("overlay types.ts");

    // COLD pass: caches are empty, so each job runs the full request path
    // (cache_key + cold compute + store) — every one of which reads the
    // shared view's fingerprint. Pre-fix each read recomputes the full
    // sort+hash (≥ N computations); post-fix the view memoizes it at
    // construction so the count is a small N-independent constant.
    host.provenance()
        .overlay_set_fingerprint_full_computations
        .store(0, Relaxed);
    let cold = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("cold overlay batch dispatch");
    let cold_fp_computations = host
        .provenance()
        .overlay_set_fingerprint_full_computations
        .load(Relaxed);
    assert_eq!(cold.len(), N, "one slot per input");
    assert!(
        cold.iter()
            .all(|slot| slot.as_deref() == Some(b"props=3 events=1".as_slice())),
        "every cold slot must resolve to the OVERLAY-aware surface (props=3) \
         — confirming the shared overlaid view actually carries the overlay \
         into every job; observed {cold:?}",
    );
    assert!(
        cold_fp_computations >= 1,
        "the per-batch view construction MUST perform at least one real \
         overlay-set fingerprint computation (the overlay set is non-empty), \
         so a `0` count cannot trivially satisfy the O(1) bound below; \
         observed {cold_fp_computations}",
    );
    assert!(
        cold_fp_computations < N as u64,
        "a COLD batch of N={N} over an overlay session must compute the \
         overlay-set fingerprint O(1) times (memoized once at view \
         construction), NOT once per component / per `cache_key`. Observed \
         {cold_fp_computations} full computations — an un-memoized \
         `fingerprint()` recomputes the sort+hash on every `cache_key` / \
         warm-probe / store (≥ N={N}). Memoize the fingerprint on the view.",
    );

    // WARM pass: the payload cache is now populated, so each job runs the
    // warm probe (which also reads the fingerprint via the view-aware
    // cache key). Pre-fix each probe recomputes (≥ N); post-fix the memo
    // makes it a small N-independent constant.
    host.provenance()
        .overlay_set_fingerprint_full_computations
        .store(0, Relaxed);
    let warm = session
        .get_component_meta_batch_payloads(&ids, encode_counts)
        .expect("warm overlay batch dispatch");
    let warm_fp_computations = host
        .provenance()
        .overlay_set_fingerprint_full_computations
        .load(Relaxed);
    assert!(
        warm.iter()
            .all(|slot| slot.as_deref() == Some(b"props=3 events=1".as_slice())),
        "every warm slot must still resolve to the OVERLAY-aware surface \
         (props=3); observed {warm:?}",
    );
    assert!(
        warm_fp_computations >= 1,
        "the per-batch view construction MUST perform at least one real \
         overlay-set fingerprint computation on the warm pass too; observed \
         {warm_fp_computations}",
    );
    assert!(
        warm_fp_computations < N as u64,
        "a WARM batch of N={N} over an overlay session must compute the \
         overlay-set fingerprint O(1) times (memoized once at view \
         construction), NOT once per component / per warm probe. Observed \
         {warm_fp_computations} full computations — an un-memoized \
         `fingerprint()` recomputes the sort+hash on every warm probe \
         (≥ N={N}). Memoize the fingerprint on the view.",
    );
}

/// An analysis-only read over an overlay session computes the overlay-set
/// fingerprint ZERO times.
///
/// `MetaSession::get_analysis` → `with_overlay_view` →
/// `get_analysis_via_view` reads only tombstone / `source` /
/// `overlay_content_hash_for` — it NEVER calls `SessionView::fingerprint`.
/// So an analysis-only request has no need for the overlay-set fingerprint
/// at all. The fingerprint feeds cache-KEY derivation (the component-meta /
/// payload paths); the analysis path does not touch it.
///
/// DISCRIMINATING (eager vs lazy memo): when the memo is computed EAGERLY
/// in `OverlaidViewRef::new`, every `with_overlay_view` constructs a view
/// and immediately pays the collect + sort + hash over the whole overlay
/// set — so K analysis calls register ≥ K full computations on the per-host
/// counter, EVEN THOUGH no caller ever reads the fingerprint. With a LAZY
/// memo (`OnceLock` initialised empty in the constructor; computed on first
/// `fingerprint()` read) an analysis-only run that never reads the
/// fingerprint computes it ZERO times. Pre-fix this asserts `0` against an
/// observed `≥ K` (FAILS); post-fix the observed count is exactly `0`
/// (PASSES). This is THE regression guard for making the memo lazy.
///
/// HERMETIC: the counter is per-`VerterHost`
/// (`overlay_set_fingerprint_full_computations`), and `make_project` builds
/// a fresh host, so a concurrent test fingerprinting a DIFFERENT host can
/// never inflate this measurement. The overlay set is genuinely non-empty
/// (it overlays `/src/types.ts` with a third prop), so an eager view WOULD
/// run the full sort+hash body (the empty short-circuit returns 0 without
/// counting) — the `0` assertion cannot be trivially satisfied by an empty
/// overlay set.
#[test]
fn analysis_only_overlay_read_never_computes_fingerprint() {
    use std::sync::atomic::Ordering::Relaxed;
    let project = make_project();
    let host = project.host();
    const N: usize = 4;
    let ids = build_components(&project, N);

    // A session that overlays the shared `./types` dependency (adds a third
    // prop). The overlay set is NON-EMPTY, so an eager view construction
    // would walk the full sort+hash body — not the empty short-circuit.
    let session = project.open_session().expect("overlay session");
    session
        .upsert("/src/types.ts", TYPES_TS_OVERLAY_THREE_PROPS.to_string())
        .expect("overlay types.ts");

    // Measure ONLY the analysis-only reads. Each `get_analysis` constructs
    // exactly one fresh `OverlaidViewRef` (one `with_overlay_view`), and the
    // analysis path never reads `fingerprint()`. K reads over an EAGER memo
    // pay K full computations; over a LAZY memo they pay ZERO.
    host.provenance()
        .overlay_set_fingerprint_full_computations
        .store(0, Relaxed);
    for canonical in &ids {
        let analysis = session
            .get_analysis(canonical)
            .expect("analysis-only read succeeds");
        // Confirm the analysis path actually ran over the overlay session
        // (a real snapshot, not a tombstoned / empty miss) so the `0`
        // fingerprint count reflects a genuine analysis workload.
        assert!(
            analysis.is_some(),
            "analysis-only read must return a snapshot for {canonical}",
        );
    }
    // Also exercise the overlaid dependency itself through the analysis
    // path: still no `fingerprint()` read.
    let dep_analysis = session
        .get_analysis("/src/types.ts")
        .expect("analysis-only read of the overlaid dependency succeeds");
    assert!(
        dep_analysis.is_some(),
        "analysis-only read of the overlaid /src/types.ts must return a snapshot",
    );

    let analysis_fp_computations = host
        .provenance()
        .overlay_set_fingerprint_full_computations
        .load(Relaxed);
    assert_eq!(
        analysis_fp_computations, 0,
        "an analysis-only run over an overlay session must compute the \
         overlay-set fingerprint ZERO times — `get_analysis_via_view` never \
         reads `SessionView::fingerprint`, so a view constructed for an \
         analysis request must NOT eagerly compute the fingerprint. Observed \
         {analysis_fp_computations} full computations: the memo is being \
         computed EAGERLY at view construction. Make it lazy (compute on \
         first `fingerprint()` read).",
    );
}

/// S8 stamp-source pin: `store_meta_payload` stamps
/// `validated_at_generation` from the CALLER-CAPTURED (flight) project
/// generation, never a live re-read. A project bump landing in the
/// admission-fence→store window must leave the payload stamped under
/// the graph it was computed from, so the warm read's generation
/// backstop (`validated_at_generation == live`) rejects it — pre-fix
/// the live-read stamp gave the stale payload the post-bump generation
/// and the backstop was permanently defeated for the under-recorded
/// (empty-signature) case it exists for.
#[test]
fn store_meta_payload_stamps_flight_captured_generation_not_live() {
    let project = make_project();
    let ids = build_components(&project, 1);
    let canonical = ids[0].as_str();
    let host = project.host();

    // The producing flight captured THIS generation…
    let captured = host.project_type_store.current_project_generation();
    // …and a project mutation lands in the fence→store window.
    host.project_type_store.bump_project_generation();
    assert_ne!(
        captured,
        host.project_type_store.current_project_generation(),
        "anti-vacuity: the window mutation moved the live generation",
    );

    // The store stamps the CAPTURED generation (empty signature — the
    // exact under-recorded case the backstop exists for).
    host.store_meta_payload(canonical, &[], vec![0x58, 0x38], captured);
    let cached = host
        .derived_raw_cache()
        .get(canonical)
        .and_then(|e| e.value().cached_meta_payload.clone())
        .expect("payload stored");
    assert_eq!(
        cached.validated_at_generation, captured,
        "the stamp must be the flight-captured generation, not the live \
         counter (a live re-read here is exactly the window race)",
    );

    // And the generation backstop holds: the warm read rejects the
    // payload computed under the superseded graph.
    assert_eq!(
        host.try_get_cached_meta_payload(canonical),
        None,
        "a payload stamped under the captured (pre-bump) generation must \
         MISS the warm read after the project shape moved",
    );
}
