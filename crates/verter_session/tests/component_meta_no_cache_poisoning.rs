//! Final-cache no-poison discriminator for projection-budget partials.
//!
//! P0 #2 — characterise the propagation of `cache_suppress` from
//! the reducer/materializer pipeline through
//! `ResolvedComponentMetaState.synthesis_should_suppress` into the
//! final `ComponentMetaResultDb` admission gate.
//!
//! Pre-fix shape:
//!   - Dispatch returns `cache_suppress=true` on budget-exhausted
//!     projection reads. The `reduce_published_field_types` second
//!     pass (and the per-field materializer) reads through the
//!     dispatch but never surfaces the bit to
//!     `compute_component_meta_state_inner`. The resulting partial
//!     ComponentMeta is admitted to `ComponentMetaResultDb`. A
//!     subsequent identical request warm-hits the poisoned entry
//!     and replays the partial instead of re-running the cold
//!     compute against fresh budget.
//!
//! Post-fix shape:
//!   - `ReduceState.cache_suppress` (per-walk OR-fold of every
//!     `read.cache_suppress`) propagates into
//!     `MaterializedTypeExpr.cache_suppress`, AND the reducer raises
//!     a request-scoped sticky `materialization_cache_suppress`
//!     `AtomicBool` on `RequestContext`. The
//!     `compute_component_meta_state_inner` block OR-folds
//!     `current_materialization_cache_suppress()` into
//!     `synthesis_should_suppress` immediately before constructing
//!     the `ResolvedComponentMetaState`. The
//!     `ComponentMetaResultDb` cache admission gate at
//!     `host_manage::component_meta_entry::write_published_component_meta`
//!     refuses any partial whose synthesis-suppress is set.
//!
//! Discriminator contract:
//!   1. Hermetic project with a TIGHT `projection_op_budget` so the
//!      cold compute trips the cap mid-projection.
//!   2. First `get_component_meta` call returns Some (partial).
//!   3. `ComponentMetaResultDb` MUST have zero entries — the
//!      synthesis-suppress flag refused admission.
//!   4. Second identical call must re-trigger cold compute (warm
//!      hit on a poisoned partial would be the bug we are
//!      characterising).
//!   5. Counterfixture: a SECOND project with the default HIGH
//!      budget on the SAME fixture admits exactly one cache entry
//!      and yields a complete (non-partial) result.

#![cfg(test)]

use std::sync::Arc;

use verter_session::{AnalysisLevel, HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// SFC + helper interface designed so that `defineProps<...>` cold
/// resolution dispatches many `ProjectMember` / `KeyOf` /
/// `IndexedAccess` operations (each counts toward
/// `projection_op_budget`). The component publishes 12 props each
/// derived from a different intersection / indexed-access shape so
/// even a small per-prop hop count multiplies into a budget
/// exhaustion under a tight cap.
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

fn build_project(projection_op_budget: usize) -> Arc<VerterHost> {
    let scheduler_config = verter_scheduler::scheduler::SchedulerConfig {
        cpu_threads: 1,
        ..verter_scheduler::scheduler::SchedulerConfig::default()
    };
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file("/workspace/helper.ts".into(), Arc::from(HELPER_DTS));
    workspace.inject_file("/workspace/Comp.vue".into(), Arc::from(COMPONENT_SFC));
    let ws_access: Arc<dyn WorkspaceAccess> = workspace;
    Arc::new(VerterHost::new_with_scheduler_config(
        HostConfig {
            analysis_level: AnalysisLevel::Full,
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

/// **Primary discriminator** — projection-budget-exhausted
/// component-meta resolution MUST NOT admit a partial into the
/// final-result `ComponentMetaResultDb` cache.
///
/// Failure mode this test characterises: pre-fix, a partial
/// component-meta produced under budget exhaustion was admitted
/// to the cache because `reduce_published_field_types`'s internal
/// dispatch consultation observed `cache_suppress=true` but the
/// signal was dropped at the reducer/materializer/projector
/// pipeline boundaries (it only propagated when the slot-binding
/// synthesis itself bailed). A subsequent identical request
/// warm-hit the poisoned entry.
///
/// Post-fix: `current_materialization_cache_suppress()` is
/// OR-folded into `synthesis_should_suppress` so the admission
/// gate refuses the partial. The cache stays empty; the second
/// request re-runs the cold compute.
#[test]
fn component_meta_final_cache_does_not_admit_budget_exceeded_partial() {
    // Tight cap. The fixture's 12 props each need at least 2
    // projection hops (`Surface['x']['y']`), so 24+ projection ops
    // are required for full materialisation — a cap of 4 exhausts
    // partway through the cold compute.
    let host = build_project(4);

    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "fixture invariant: cache is empty before the first resolve",
    );

    // First cold resolution. Returns Some(partial) — `defineProps`
    // produces some prop entries but several remain opaque /
    // unresolved.
    let first = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("component-meta query returns Some even under partial");

    assert!(
        !first.props.is_empty(),
        "even a partial result should publish at least one prop (defineProps macro succeeded)",
    );

    // Primary assertion: the final-result cache MUST stay empty.
    // synthesis_should_suppress observed the budget-exhausted
    // partial through `current_materialization_cache_suppress()`
    // and `cache_component_meta` refused admission.
    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "BUG (P0 #1): projection-budget-exhausted partial was admitted to ComponentMetaResultDb. \
         The cache_suppress signal from the reducer/materializer pipeline failed to reach the \
         final admission gate. Expected the cache to stay EMPTY after a budget-exhausted cold \
         resolution; got {} entries.",
        component_meta_cache_len(&host),
    );

    // Secondary assertion: a subsequent identical request must
    // re-run the cold compute (no warm hit on a poisoned partial).
    // We re-query and verify the cache is STILL empty — a warm
    // hit would have left the entry alone (no admission) but
    // because the entry never existed, both attempts cold-fire
    // and both refuse to admit.
    let second = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("second component-meta query also returns Some(partial)");

    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "second budget-exhausted query must also refuse cache admission (got {} entries)",
        component_meta_cache_len(&host),
    );

    // Sanity: both calls returned structurally-identical partials
    // (same prop count). This is not the load-bearing assertion;
    // the primary check is the cache-emptiness above.
    assert_eq!(
        first.props.len(),
        second.props.len(),
        "both partial resolutions should produce identical published-prop counts; \
         got first={} second={}",
        first.props.len(),
        second.props.len(),
    );
}

/// **Counterfixture** — the SAME fixture with a HIGH budget admits
/// exactly one cache entry. Ensures the primary discriminator is
/// not a vacuous "cache stays empty under all conditions" pass.
#[test]
fn component_meta_final_cache_admits_complete_result_under_default_budget() {
    let host = build_project(2000);

    assert_eq!(
        component_meta_cache_len(&host),
        0,
        "fixture invariant: cache is empty before the first resolve",
    );

    let _ = host
        .get_component_meta("/workspace/Comp.vue")
        .expect("component-meta query returns Some under default budget");

    assert_eq!(
        component_meta_cache_len(&host),
        1,
        "under the default (HIGH) budget the cold compute completes within cap and the \
         result is admitted to ComponentMetaResultDb (got {} entries)",
        component_meta_cache_len(&host),
    );
}
