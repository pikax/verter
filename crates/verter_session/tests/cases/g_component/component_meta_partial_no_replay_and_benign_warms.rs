//! Final-cache no-replay (partial) + benign-complete warms (partial-metadata invariant §4).
//!
//! Two end-to-end discriminators on a single host, complementing the
//! lib-level producer / memo tests:
//!
//! 1. **No laundered warm replay.** A projection-budget-exhausted
//!    component-meta resolution is a GENUINE partial. Request 1 produces it;
//!    requests 2 and 3 (fresh same-host) must NOT warm-hit a published final
//!    result AND must NOT warm-replay a laundered semantic-memo entry — the
//!    `ComponentMetaResultDb` stays empty and each request re-runs the cold
//!    compute. This is the integration counterpart of the lib-level
//!    `partial_value_leaves_no_memo_entry_and_fresh_request_cold_rebuilds`
//!    memo-laundering proof.
//!
//! 2. **Benign-complete warms + replays.** A COMPLETE component-meta result
//!    publishes to `ComponentMetaResultDb` and a fresh request warm-hits it
//!    (`from_cache=true`), with `synthesis_should_suppress=false`. This is
//!    the public-API half of the §1 invariant (benign non-cacheability must
//!    NOT block the final warm). The REAL benign non-cacheable PRODUCER (a
//!    forced tracer-signature Overflow surfacing a COMPLETE Value with
//!    `result_is_partial=false`) is exercised + mutation-checked directly at
//!    the `pub(crate)` materialiser entry by the lib-level
//!    `overflow_returns_valid_outcome_and_refuses_cache_admission` test —
//!    the shallow-by-default projector does not route a plain
//!    `resolve_component_meta` through the materialiser, so the forced-overflow
//!    knob cannot fire on this public path.

#![cfg(test)]

use std::sync::Arc;

use verter_session::audited_request::AuditedRequest;
use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

const HELPER_DTS: &str = r#"
export interface Surface {
  a: { x: string; y: number; z: boolean }
  b: { x: string; y: number; z: boolean }
  c: { x: string; y: number; z: boolean }
  d: { x: string; y: number; z: boolean }
  e: { x: string; y: number; z: boolean }
  f: { x: string; y: number; z: boolean }
}
"#;

const COMPONENT_SFC: &str = r#"<script setup lang="ts">
import type { Surface } from './helper'
defineProps<{
  pa: Surface['a']['x']
  pb: Surface['b']['x']
  pc: Surface['c']['x']
  pd: Surface['d']['x']
  pe: Surface['e']['x']
  pf: Surface['f']['x']
  pg: Surface['a']['y']
  ph: Surface['b']['y']
  pi: Surface['c']['y']
  pj: Surface['d']['y']
  pk: Surface['e']['y']
  pl: Surface['f']['y']
}>();
</script>
<template><div /></template>
"#;

const SIMPLE_SFC: &str = r#"<script setup lang="ts">
import type { Surface } from './helper'
defineProps<{ icon: Surface['a']['x']; label: Surface['b']['y'] }>();
</script>
<template><div /></template>
"#;

fn build_host(projection_op_budget: usize, files: &[(&str, &str)]) -> Arc<VerterHost> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    for (path, src) in files {
        workspace.inject_file((*path).into(), Arc::from(*src));
    }
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: true,
            projection_op_budget,
            ..HostConfig::default()
        },
        ws_access,
        scheduler_config,
    ))
}

fn component_meta_cache_len(host: &Arc<VerterHost>) -> usize {
    host.project_type_store().component_meta_results().len()
}

fn materialize_structure_cache_len(host: &Arc<VerterHost>) -> usize {
    host.project_type_store()
        .materialize_structure_db()
        .live_count()
}

fn shape_cache_len(host: &Arc<VerterHost>) -> usize {
    host.project_type_store().shape_cache_db().live_count()
}

/// Host-global count of semantic-memo COLD builds (one increment per
/// `execute_cooperative` cold-build closure that installed a fact tracer).
/// A laundered warm replay would short-circuit before the cold-build
/// closure fires, so this counter's per-request DELTA is the direct signal
/// of "did the semantic memo cold-rebuild or warm-hit".
fn semantic_memo_cold_builds(host: &Arc<VerterHost>) -> u64 {
    host.provenance()
        .memo_entry_fact_tracer_installs
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// Partial: NO laundered warm replay across THREE requests (partial-metadata invariant §4).
///
/// Request 1 produces a budget-exhausted partial; the final cache stays
/// empty (synthesis-suppress refused admission). Requests 2 and 3 must each
/// observe a final-cache MISS (no warm hit, `from_cache=false`) — proving
/// neither a laundered `ComponentMetaResultDb` entry NOR a laundered
/// semantic-memo entry replayed the partial as complete.
#[test]
fn partial_component_meta_does_not_warm_replay_across_three_requests() {
    // Tight cap so the cold compute trips the projection-op budget
    // mid-materialisation (matching the sibling no-poison discriminator's
    // budget=4 contract on the same fixture through the direct
    // `get_component_meta` path; the audited harness path resolves the
    // indexed-access props structurally and does not trip, so this e2e
    // drives `get_component_meta` directly).
    let host = build_host(
        4,
        &[
            ("/workspace/helper.ts", HELPER_DTS),
            ("/workspace/Comp.vue", COMPONENT_SFC),
        ],
    );
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "fixture invariant: final cache empty before first resolve",
    );

    let mut prop_counts = Vec::new();
    let mut cold_build_deltas = Vec::new();
    for request_index in 0..3u32 {
        let cold_before = semantic_memo_cold_builds(&host);
        let meta = host
            .get_component_meta("/workspace/Comp.vue")
            .unwrap_or_else(|| panic!("request {request_index}: component-meta returns Some"));
        let cold_after = semantic_memo_cold_builds(&host);
        prop_counts.push(meta.props.len());
        cold_build_deltas.push(cold_after - cold_before);

        // The budget-exhausted partial must NEVER be admitted to the final
        // cache, so NO request can warm-hit it: the ComponentMetaResultDb
        // stays empty after EVERY request. A laundered semantic-memo entry
        // would let request 2/3 reconstruct a COMPLETE result and admit it
        // here — the no-launder memo invariant (proven at the lib level)
        // keeps every request cold + refused.
        assert_eq!(
            component_meta_cache_len(&host),
            0,
            "request {request_index}: ComponentMetaResultDb must stay EMPTY after a \
             budget-exhausted partial — a non-empty cache means a laundered partial (final or \
             semantic-memo) replayed as complete and admitted (got {} entries)",
            component_meta_cache_len(&host),
        );
    }

    // No laundered warm replay: every request produced the SAME partial
    // shape. A laundered complete replay on request 2/3 would yield a
    // DIFFERENT (larger) prop count than the cold request-1 partial.
    assert!(
        prop_counts.windows(2).all(|w| w[0] == w[1]),
        "requests 2/3 must reproduce request 1's partial shape (no laundered complete replay); \
         got prop counts {prop_counts:?}",
    );

    // SEMANTIC COLD/WARM COUNTER DELTAS. Request 1 is genuinely cold: the
    // initial compute MUST drive a strictly positive count of semantic cold
    // builds — a zero request-1 delta would mean the partial was served from
    // somewhere without ever computing, which is its own launder signal.
    //
    // Requests 2/3 carry NO positive-delta requirement: benign COMPLETE
    // sub-results legitimately warm across requests (the §1-correct
    // behaviour), and the projection-op budget trips inside the PROJECTOR
    // loop — not inside a refused semantic subquery — so a follow-up request
    // can read every issued subquery warm (delta 0) and still reproduce the
    // SAME refused partial. The no-launder discriminators for requests 2/3
    // are the per-request final-cache-empty assertion in the loop above plus
    // the stable-partial-shape assertions below: a laundered warm-COMPLETE
    // replay yields a LARGER prop count and a non-empty final cache.
    assert!(
        cold_build_deltas[0] > 0,
        "request 1 MUST drive semantic cold builds (it is the genuinely cold compute); \
         semantic cold-build delta was {} (deltas {cold_build_deltas:?})",
        cold_build_deltas[0],
    );

    // REQUEST 3 discriminator. Request 3's cold delta MAY legitimately reach 0
    // (every benign-COMPLETE cacheable prefix is warm by now — the §1-correct
    // behaviour), so a `deltas[2] > 0` assertion would be a false invariant.
    // The discriminating request-3 probe is instead the NEGATIVE no-launder
    // signal: had the refused partial been laundered into a warm-complete
    // semantic-memo or final-cache entry, request 3 would warm-serve it as a
    // COMPLETE result — yielding a LARGER prop count than the cold request-1
    // partial AND a non-empty final cache. We assert request 3 reproduced the
    // EXACT request-1 partial shape (caught here directly, independent of the
    // windows() chain above) and that the final cache is STILL empty after
    // request 3 — the joint condition that a laundered complete replay would
    // violate.
    assert_eq!(
        prop_counts[2], prop_counts[0],
        "request 3 MUST reproduce request 1's EXACT partial shape — a larger prop count would mean \
         request 3 warm-served a laundered complete entry (got {prop_counts:?})",
    );
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "request 3 MUST leave the ComponentMetaResultDb empty — a non-empty cache after the third \
         request means a laundered partial replayed as complete and was admitted",
    );
}

/// Benign-complete result warms the final cache + request 2 hits it
/// (partial-metadata invariant §1) — the PUBLIC-API warm-not-blocked proof.
///
/// A COMPLETE component-meta synthesis must publish to `ComponentMetaResultDb`
/// and a fresh request MUST warm-hit it. This is the public-API counterpart
/// of the no-launder partial test above: where a partial leaves the final
/// cache empty (and every request cold-rebuilds), a COMPLETE result fills it
/// once and serves it warm thereafter. `synthesis_should_suppress` MUST stay
/// false for a complete result.
///
/// The REAL benign non-cacheable PRODUCER (a forced tracer-signature
/// Overflow that surfaces a COMPLETE `MaterializeOutcome::Value` with
/// `result_is_partial = false`) is exercised + mutation-checked directly at
/// the materialiser entry by the lib-level
/// `overflow_returns_valid_outcome_and_refuses_cache_admission` test
/// (Discrimination #1–#4): the materialiser is `pub(crate)`, and the
/// shallow-by-default projector deliberately does NOT route a plain
/// component-meta synthesis through `materialize_component_meta_structure`,
/// so the forced-overflow knob cannot fire on this public `resolve_component_meta`
/// path (it stayed at 0 — the prior round's test was vacuous for exactly
/// this reason). This integration test therefore owns the orthogonal
/// public-API half: a complete result warms + replays warm.
///
/// DISCRIMINATION: were a complete result wrongly tagged
/// `result_is_partial=true` at any synthesis sub-read, the final admission
/// gate would refuse it and request 2 would MISS (from_cache=false) — this
/// test fails.
#[test]
fn benign_complete_result_warms_and_replays_from_final_cache() {
    // HIGH budget so the synthesis completes (no partial). The plain
    // indexed-access props resolve structurally to a COMPLETE surface.
    let host = build_host(
        2000,
        &[
            ("/workspace/helper.ts", HELPER_DTS),
            ("/workspace/Simple.vue", SIMPLE_SFC),
        ],
    );

    // Request 1 — cold. A COMPLETE component-meta result is admitted to the
    // final cache.
    let (_a1, resolution1, record1) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/workspace/Simple.vue")
        .expect("request 1 must succeed");
    assert!(
        !resolution1.synthesis_should_suppress,
        "a COMPLETE component-meta result MUST NOT set synthesis_should_suppress",
    );
    assert!(
        !record1.from_cache,
        "request 1 must be cold (from_cache=false)",
    );
    assert_eq!(
        component_meta_cache_len(&host),
        1,
        "the COMPLETE result MUST be admitted to ComponentMetaResultDb — got {} entries",
        component_meta_cache_len(&host),
    );

    // Request 2 — must HIT the final cache.
    let (_a2, _resolution2, record2) = AuditedRequest::builder()
        .attach_to(Arc::clone(&host))
        .resolve_component_meta("/workspace/Simple.vue")
        .expect("request 2 must succeed");
    assert!(
        record2.from_cache,
        "request 2 MUST warm-hit the final cache (from_cache=true) — a complete result must warm; \
         a result wrongly tagged result_is_partial would have been refused",
    );
}

/// E2e replay across the result-cache set. After a
/// budget-exhausted request-1 partial:
/// - `ComponentMetaResultDb` stays EMPTY (the final partial never warms);
/// - `MaterializeStructureDb` stays EMPTY (no partial structural
///   entry is admitted);
/// - `ShapeCacheDb` holds ONLY benign-COMPLETE prefixes (the members that
///   resolved completely BEFORE the budget tripped legitimately warm —
///   the "benign-complete still warms" behaviour) and does NOT
///   GROW on request 2 (no laundered complete replay);
/// - request 2 reproduces request 1's EXACT partial shape (not served as
///   a laundered complete result).
///
/// MUTATION CHECK: reverting the `finish_materialize_admission` gate
/// lets the budget-tripped partial admit a `MaterializeStructureDb` entry
/// — the `materialize_structure_cache_len == 0` assertion fails. Reverting
/// the `ShapeCacheDb` gate lets the budget-tripped PARTIAL member
/// shapes admit too — the shape cache grows past its benign-complete
/// prefix count AND request 2 can launder a larger result, failing the
/// stable-shape / no-growth assertions.
#[test]
fn partial_leaves_result_caches_uncorrupted_and_request2_not_complete() {
    let host = build_host(
        4,
        &[
            ("/workspace/helper.ts", HELPER_DTS),
            ("/workspace/Comp.vue", COMPONENT_SFC),
        ],
    );
    assert_eq!(component_meta_cache_len(&host), 0);
    assert_eq!(materialize_structure_cache_len(&host), 0);
    assert_eq!(shape_cache_len(&host), 0);

    // Request 1 — budget-exhausted partial.
    let meta1 = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("request 1 returns Some (partial)");
    let request1_props = meta1.props.len();
    let shape_after1 = shape_cache_len(&host);

    // The final partial never warms; no partial structural entry is admitted.
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "ComponentMetaResultDb MUST be empty after a partial",
    );
    assert_eq!(
        materialize_structure_cache_len(&host),
        0,
        "MaterializeStructureDb MUST be empty after a budget-exhausted partial — a \
         non-zero count means the partial admitted a structural entry (revert the \
         finish_materialize_admission gate to see this fail)",
    );

    // Request 2 — MUST NOT be served as a laundered complete result.
    let meta2 = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("request 2 returns Some");
    assert_eq!(
        meta2.props.len(),
        request1_props,
        "request 2 MUST reproduce request 1's EXACT partial shape — a larger prop count means \
         a laundered complete result replayed",
    );
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "ComponentMetaResultDb MUST still be empty after request 2",
    );
    assert_eq!(
        materialize_structure_cache_len(&host),
        0,
        "MaterializeStructureDb MUST still be empty after request 2",
    );
    // ShapeCacheDb may hold benign-COMPLETE prefixes (the members that
    // completed before the budget tripped). The discriminating signal is
    // that it does NOT GROW on request 2 — a laundered partial replay
    // would add the budget-tripped partial shapes as admitted entries.
    assert_eq!(
        shape_cache_len(&host),
        shape_after1,
        "ShapeCacheDb MUST NOT grow on request 2 — its entries are benign-complete \
         prefixes only; growth means a budget-tripped PARTIAL member shape was admitted \
         (revert the ShapeCacheDb gate to see this fail)",
    );
}
