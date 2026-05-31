//! Block 6.i Round 7 — discriminator: **inherited emits branch-merge preservation**.
//!
//! Regression guard locked at the round-7 boundary. A parent SFC that
//! inherits emits from a child component whose `defineEmits` payload
//! is an OPEN CONDITIONAL (`Mode extends 'editor' ? EditorEmits :
//! ViewerEmits`) MUST surface BOTH branches' event names in the
//! parent's `accepted_events` set, AT ALL commits in the round-7
//! sequence.
//!
//! ## Why this guards the cutover
//!
//! Pre-Commit-2 the publication path lowers the macro payload via
//! `ProjectionMode::Navigate` and the empty-path `Published(Shallow)`
//! surface walk runs `ProjectPath` over a Conditional carrier — the
//! existing dispatch path distributes the conditional and merges both
//! branches' event rows through the surface walker. The parent's
//! accepted_events surface receives both `itemEdited` and
//! `itemViewed`.
//!
//! Post-Commit-2 substrate addition: `resolve_macro_payload` migrates
//! to `structural_transit_with_mode(Navigate)` lowering, and the
//! macro-payload's surface is read through
//! `resolve_payload_surface(Published(Shallow))`. For undecided
//! conditional macro object payloads, the substrate adds
//! **branch-merged shallow semantics** (codex Q3 / Q4): project the
//! true and false branches under `Published(Shallow)` and merge the
//! top-level event rows. Scoped tightly to macro object publication
//! (codex Q6 risk) so non-macro symbolic surfaces are not widened.
//!
//! ## Discrimination progression
//!
//! - **Commit 1 (no substrate extensions):** PASS — Expanded
//!   publication distributes the conditional and merges branches via
//!   the existing path. Regression guard.
//! - **Commit 2 (substrate extensions added):** PASS —
//!   `resolve_payload_surface` gains branch-merged shallow support;
//!   the existing test surface stays covered.
//! - **Commit 3 (atomic cutover):** PASS — consumer migrations land;
//!   branch-merge in `resolve_payload_surface` restores the inherited
//!   surface that round-6 Commit-3 regressed.
//!
//! A regression where the branch-merge isn't wired or the cutover
//! drops the inherited surface fails this test loudly. This test is
//! the round-7 boundary lock for `resolver_coverage_inherited_emits_branch_merged_surface`
//! — the new regression flagged by round-6's STOP.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

#[path = "../component_meta_audit/harness.rs"]
mod harness;

const PARENT_VUE: &str = r#"<script setup lang="ts">
import Child from './child.vue';
</script>
<template><Child /></template>
"#;

const CHILD_VUE: &str = r#"<script setup lang="ts" generic="Mode extends 'editor' | 'viewer'">
type EditorEmits = { itemEdited: [id: number] };
type ViewerEmits = { itemViewed: [id: number] };
type ConditionalEmits = Mode extends 'editor' ? EditorEmits : ViewerEmits;
defineEmits<ConditionalEmits>();
</script>
<template><div /></template>
"#;

#[test]
fn round7_inherited_emits_branch_merged_surface_survives_transit_cutover() {
    let host = harness::build_hermetic_host_with_lib(
        &[("/parent.vue", PARENT_VUE), ("/child.vue", CHILD_VUE)],
        &[("lib.es5.d.ts", harness::STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = harness::resolve_under_audit(host, "/parent.vue");

    let accepted_event_names: Vec<String> = analysis
        .accepted_events
        .iter()
        .map(|e| e.name.clone())
        .collect();

    for required in ["itemEdited", "itemViewed"] {
        assert!(
            accepted_event_names.iter().any(|n| n == required),
            "Block 6.i Round 7 — parent SFC's `accepted_events` MUST include the child's \
             component-specific emit `{required}` from BOTH branches of the conditional \
             `Mode extends 'editor' ? EditorEmits : ViewerEmits`. Under EVERY round-7 \
             commit: pre-cutover via the existing Expanded publication's distribution, \
             post-cutover via `resolve_payload_surface`'s branch-merged shallow semantics \
             (codex Q3). A regression here means branch-merge was not wired into the \
             cutover-path's surface reader. Got events: {accepted_event_names:?}"
        );
    }

    // Negative assertion: no phantom event leaks in (rigorous scoping
    // per codex Q6 — branch-merge must not widen unrelated symbolic
    // surfaces).
    assert!(
        !accepted_event_names.iter().any(|n| n == "phantomEventXyz"),
        "Block 6.i Round 7 — branch-merge MUST be scoped to macro object publication; \
         no phantom event names may leak. Got: {accepted_event_names:?}"
    );
}
