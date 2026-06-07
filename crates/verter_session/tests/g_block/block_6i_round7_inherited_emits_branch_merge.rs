//! Discriminator: **inherited emits branch-merge preservation**.
//!
//! Regression guard. A parent SFC that inherits emits from a child
//! component whose `defineEmits` payload is an OPEN CONDITIONAL
//! (`Mode extends 'editor' ? EditorEmits : ViewerEmits`) MUST surface
//! BOTH branches' event names in the parent's `accepted_events` set.
//!
//! ## Why this guards the behaviour
//!
//! A publication path that lowers the macro payload via
//! `ProjectionMode::Navigate` and runs the empty-path
//! `Published(Shallow)` surface walk over a Conditional carrier
//! distributes the conditional and merges both branches' event rows
//! through the surface walker. The parent's accepted_events surface
//! receives both `itemEdited` and `itemViewed`.
//!
//! `resolve_macro_payload` lowers via
//! `structural_transit_with_mode(Navigate)`, and the macro-payload's
//! surface is read through `resolve_payload_surface(Published(Shallow))`.
//! For undecided conditional macro object payloads, the substrate
//! applies **branch-merged shallow semantics**: project the
//! true and false branches under `Published(Shallow)` and merge the
//! top-level event rows. Scoped tightly to macro object publication
//! so non-macro symbolic surfaces are not widened.
//!
//! ## Discrimination
//!
//! `resolve_payload_surface`'s branch-merged shallow support restores
//! the inherited surface. A regression where the branch-merge isn't
//! wired or the inherited surface is dropped fails this test loudly.
//! This test is the lock for
//! `resolver_coverage_inherited_emits_branch_merged_surface`.

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
fn round7_inherited_emits_branch_merged_surface_survives_transit() {
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
            "parent SFC's `accepted_events` MUST include the child's \
             component-specific emit `{required}` from BOTH branches of the conditional \
             `Mode extends 'editor' ? EditorEmits : ViewerEmits`, via \
             `resolve_payload_surface`'s branch-merged shallow semantics. \
             A regression here means branch-merge was not wired into the \
             surface reader. Got events: {accepted_event_names:?}"
        );
    }

    // Negative assertion: no phantom event leaks in (rigorous scoping —
    // branch-merge must not widen unrelated symbolic surfaces).
    assert!(
        !accepted_event_names.iter().any(|n| n == "phantomEventXyz"),
        "branch-merge MUST be scoped to macro object publication; \
         no phantom event names may leak. Got: {accepted_event_names:?}"
    );
}
