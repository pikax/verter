//! Block 6.i Transit-Shallow Publication — discriminator #4.
//!
//! Regression guard for the macro-publication boundary: a
//! `defineSlots<LiteralKeyedSlots<{...}>>()` MUST keep publishing the
//! `badge.plan` binding regardless of which lowering mode the
//! publication helper uses (`Expanded` pre-cutover OR
//! `StructuralTransit(Navigate)` post-cutover).
//!
//! ## Why this guards the cutover
//!
//! Pre-Commit-3 the macro publication lowers the slot expression at
//! `Published(Expanded)`. The `Mapped<{ [K in 'badge'|'title']?: ... }>`
//! reduces eagerly: `build_mapped_type`'s key enumeration succeeds
//! (literal-union keyspace), per-key Conditional reduction produces
//! `Function { params: [{ planId, plan }] }`, and
//! `compute_bindings_via_graph` reads `Function.params[0].ty`'s
//! Shallow surface — yielding `badge.plan` + `title.plan` binding
//! rows in `expanded.slot_bindings`.
//!
//! Post-Commit-3 the publication switches to
//! `structural_transit_with_mode(Navigate)`. The Mapped carrier-stops
//! at the publication boundary; the slot value is no longer eagerly
//! a `Function`. The graph-native consumer (`compute_bindings_via_graph`)
//! must then realize the slot member through the shared callable-
//! realization primitive (Function → Alias → closed-Conditional →
//! InstantiationRef → DeclRef) AND the Shallow walker's
//! `synthesise_mapped_surface` must produce per-key substituted
//! values via the Commit-2 substrate + Commit-3 source-surface
//! enumeration.
//!
//! Without the substrate + realization + cutover-all-7-consumers
//! migration, the cutover regresses this test (the
//! `imported_mapped_slots_reach_resolved_evaluated_types`-equivalent
//! locked-down regression). The test holds the cutover honest.
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate, no cutover):** PASS — HEAD's Expanded
//!   publication produces the binding directly. Test passes as a
//!   regression guard.
//! - **Commit 2 (substrate added, no cutover):** PASS — publication
//!   unchanged.
//! - **Commit 3 (atomic cutover):** PASS — publication is now
//!   `StructuralTransit(Navigate)`; the substrate + realization
//!   primitive must keep this passing.
//!
//! At Commit 3 the locked-down regression `imported_mapped_slots_reach_resolved_evaluated_types`
//! (and this test) protect against an incomplete cutover landing.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use crate::harness;

use verter_session::audited_request::AuditedRequest;

const PRICING_SLOTS_TS: &str = r#"
export interface PricingPlan {
  id: string;
  tier: string;
}

export interface PricingPlanSlots {
  badge(props: { planId: string }): unknown;
  title(props: { planId: string }): unknown;
}

export type ExtendSlotWithPlan<TPlan, TKey extends keyof PricingPlanSlots> =
  PricingPlanSlots[TKey] extends (props: infer P) => unknown
    ? (props: P & { plan: TPlan }) => unknown
    : PricingPlanSlots[TKey];

export type LiteralKeyedSlots<TPlan extends PricingPlan = PricingPlan> = {
  [K in 'badge' | 'title']?: ExtendSlotWithPlan<TPlan, K>
};
"#;

const PRICING_PLANS_VUE: &str = r#"<script setup lang="ts">
import type { LiteralKeyedSlots, PricingPlan } from './pricing_slots';
defineSlots<LiteralKeyedSlots<PricingPlan>>();
</script>
<template><div></div></template>
"#;

fn resolve_pricing_plans() -> verter_semantic::analysis::component_meta::ComponentMetaAnalysis {
    let host = harness::build_hermetic_host(&[
        ("/pricing_slots.ts", PRICING_SLOTS_TS),
        ("/PricingPlans.vue", PRICING_PLANS_VUE),
    ]);
    let (analysis, _resolved, _audit) = AuditedRequest::builder()
        .attach_to(host)
        .resolve_component_meta("/PricingPlans.vue")
        .expect("hermetic resolve must succeed");
    analysis
}

fn slot_binding_names(
    analysis: &verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for slot in analysis.slots.iter() {
        for binding in slot.bindings.iter() {
            out.push((slot.name.clone(), binding.name.clone()));
        }
    }
    out
}

#[test]
fn macro_publication_realizes_callable_slot_with_plan_binding() {
    let analysis = resolve_pricing_plans();
    let pairs = slot_binding_names(&analysis);

    let badge_plan_present = pairs.iter().any(|(s, b)| s == "badge" && b == "plan");

    assert!(
        badge_plan_present,
        "Block 6.i Transit-Shallow Publication — the `badge.plan` slot \
         binding MUST appear on the analysis's slot-bindings surface under \
         BOTH pre-cutover (Expanded publication) AND post-cutover \
         (StructuralTransit(Navigate) publication + realization primitive). \
         A regression here means either (a) the per-key substrate \
         (`materialize_mapped_member_value_for_key`) did not produce a \
         callable Function for the substituted ExtendSlotWithPlan body, \
         OR (b) the graph-native consumer's `Function` match failed to \
         realize the value through the shared realization primitive. \
         Got pairs: {:?}",
        pairs,
    );
}

/// Symmetric assertion on the `title` slot — proves the realization
/// fires for every enumerated key, not just the first one.
#[test]
fn macro_publication_realizes_every_enumerated_slot() {
    let analysis = resolve_pricing_plans();
    let pairs = slot_binding_names(&analysis);

    let title_plan_present = pairs.iter().any(|(s, b)| s == "title" && b == "plan");

    assert!(
        title_plan_present,
        "Block 6.i Transit-Shallow Publication — the `title.plan` slot \
         binding MUST appear symmetrically with `badge.plan`. Per-key \
         substitution + callable realization must fire for EVERY \
         enumerated key, not just the first. Got pairs: {:?}",
        pairs,
    );
}
