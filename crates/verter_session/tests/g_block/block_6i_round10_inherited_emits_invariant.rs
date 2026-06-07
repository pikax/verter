//! Inherited-emits invariant regression guard.
//!
//! Locks down the inherited-emits branch-merge guard against any
//! change to the Chain V / X / Y / Z closures accidentally routing a
//! Conditional macro payload through the transit-shallow path that
//! bypasses `PayloadSurfaceScope::EmitClassMacroObject`.
//!
//! Risk exists mainly in V, X, and Y if Conditional macro payloads
//! are accidentally routed through the shallow carrier path that
//! bypasses `PayloadSurfaceScope::EmitClassMacroObject`.
//!
//! Tests that must stay green:
//! - `block_6i_round9_inherited_emits_branch_merge_survives::round9_inherited_emits_branch_merge_survives_path_precise_transit_shallow`
//! - `block_6i_round7_inherited_emits_branch_merge::round7_inherited_emits_branch_merged_surface_survives_transit`
//! - `component_meta_audit::resolver_coverage_inherited_emits::resolver_coverage_inherited_emits_branch_merged_surface`
//!
//! It mirrors the Conditional-emits-merge fixture and asserts the
//! inherited-emits surface survives every transit-shallow migration of
//! the Chain V / X / Y / Z closures. The corresponding existing tests
//! in the named modules above provide their own coverage; this guard
//! makes the regression visible in this test enumerate so a
//! Conditional-bypass is spotted without running the per-module suites
//! separately.

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
            "inherited-emits invariant — the \
             Chain V / X / Y / Z closures MUST NOT regress the \
             inherited-emits branch-merge surface. The parent's \
             `accepted_events` MUST include the child's `{required}` \
             from BOTH branches of the conditional \
             `Mode extends 'editor' ? EditorEmits : ViewerEmits`. A \
             regression here means a Conditional macro payload \
             was accidentally routed through one of the transit-\
             shallow paths (the per-prop field materialiser \
             carrier-stop, the slot compound fallback, the \
             route fast-path transit-shallow sibling) — the \
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
        "branch-merge MUST stay scoped to macro \
         object publication; no phantom event names may leak. Got: \
         {accepted_event_names:?}"
    );
}
