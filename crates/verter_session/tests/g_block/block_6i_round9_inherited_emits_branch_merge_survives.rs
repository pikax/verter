//! Inherited-emits branch-merge regression guard.
//!
//! A naive non-slot Class A transit-shallow migration regresses two
//! inherited-emits locked-down tests when admitting root `Conditional`
//! carriers: `resolver_coverage_inherited_emits_branch_merged_surface`
//! and `round7_inherited_emits_branch_merged_surface_survives_transit`.
//! The *path-precise* migration branches on the lowered root's
//! semantic shape: an Object/Intersection/Mapped/Ref/InstantiationRef
//! root dispatches via the transit-shallow helper; a Conditional root
//! retains `Published(Expanded)` so the inherited-emits branch-merge
//! protocol (`resolve_payload_surface_with_scope(EmitClassMacroObject)`)
//! continues to enumerate both branches' event rows at the macro
//! publication surface.
//!
//! ## Discrimination
//!
//! Parent SFC inherits emits from a child whose `defineEmits` payload
//! is an OPEN CONDITIONAL `Mode extends 'editor' ? EditorEmits :
//! ViewerEmits`. Parent's `accepted_events` MUST include events from
//! BOTH branches. A regression where the path-precision
//! predicate mis-classifies the Conditional root and dispatches it
//! through transit-shallow would drop one branch's events from the
//! merged surface — this test fires loudly.

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
fn round9_inherited_emits_branch_merge_survives_path_precise_transit_shallow() {
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
            "path-precise non-slot transit-shallow MUST NOT \
             regress the inherited-emits branch-merge surface. The parent's \
             `accepted_events` MUST include the child's `{required}` from BOTH \
             branches of the conditional `Mode extends 'editor' ? EditorEmits : \
             ViewerEmits`. A regression here means the path-precision \
             predicate mis-classified the Conditional macro payload root and \
             routed it through the transit-shallow helper; Conditional \
             roots must retain `Published(Expanded)` so `resolve_payload_surface_with_scope` \
             can enumerate both branches' event rows. Got events: {accepted_event_names:?}"
        );
    }

    assert!(
        !accepted_event_names.iter().any(|n| n == "phantomEventXyz"),
        "branch-merge MUST stay scoped to macro object \
         publication; no phantom event names may leak. Got: {accepted_event_names:?}"
    );
}
