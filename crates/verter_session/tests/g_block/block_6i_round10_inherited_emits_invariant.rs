//! Block 6.i Round 10 — inherited-emits invariant regression guard.
//!
//! Locks down the codex 5th-consult Q5 named tests + the round-9
//! branch-merge guard against any Round-10 commit (Chain V / X / Y / Z
//! closure) accidentally routing a Conditional macro payload through
//! the transit-shallow path that bypasses
//! `PayloadSurfaceScope::EmitClassMacroObject`.
//!
//! From codex Q5 (BINDING):
//!
//! > Risk exists mainly in V, X, and Y if Conditional macro payloads
//! > are accidentally routed through the shallow carrier path that
//! > bypasses `PayloadSurfaceScope::EmitClassMacroObject`.
//! >
//! > Tests that must stay green:
//! > - `block_6i_round9_inherited_emits_branch_merge_survives::round9_inherited_emits_branch_merge_survives_path_precise_transit_shallow`
//! > - `block_6i_round7_inherited_emits_branch_merge::round7_inherited_emits_branch_merged_surface_survives_transit_cutover`
//! > - `component_meta_audit::resolver_coverage_inherited_emits::resolver_coverage_inherited_emits_branch_merged_surface`
//!
//! This file is the Round-10 lockdown: it mirrors the round-9
//! Conditional-emits-merge fixture and asserts the inherited-emits
//! surface survives every Round-10 transit-shallow migration. The
//! corresponding existing tests in the named modules above continue
//! to provide their own per-round coverage; this guard exists to
//! make the regression visible in the round-10 test enumerate so
//! the orchestrator can spot a Conditional-bypass at the round
//! boundary without having to run the per-round modules separately.

#![allow(clippy::too_many_lines, dead_code, unused_imports)]

use crate::harness;

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
fn round10_inherited_emits_branch_merge_survives_all_chain_closures() {
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
            "Block 6.i Round 10 inherited-emits invariant — Round-10's \
             Chain V / X / Y / Z closures MUST NOT regress the \
             inherited-emits branch-merge surface. The parent's \
             `accepted_events` MUST include the child's `{required}` \
             from BOTH branches of the conditional \
             `Mode extends 'editor' ? EditorEmits : ViewerEmits`. A \
             round-10 regression here means a Conditional macro payload \
             was accidentally routed through one of the new transit-\
             shallow paths (Commit 2's per-prop field materialiser \
             carrier-stop, Commit 4's slot compound fallback, Commit 5's \
             route fast-path transit-shallow sibling) — the round-9 \
             path-precision predicate \
             `macro_payload_root_is_conditional_carrier` must keep \
             Conditional roots on the `Published(Expanded)` rail so \
             `resolve_payload_surface_with_scope(EmitClassMacroObject)` \
             can enumerate both branches' event rows. Got events: \
             {accepted_event_names:?}"
        );
    }

    assert!(
        !accepted_event_names.iter().any(|n| n == "phantomEventXyz"),
        "Block 6.i Round 10 — branch-merge MUST stay scoped to macro \
         object publication; no phantom event names may leak. Got: \
         {accepted_event_names:?}"
    );
}
