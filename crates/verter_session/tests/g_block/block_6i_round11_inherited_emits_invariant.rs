//! Inherited-emits invariant regression guard.
//!
//! Locks down the top risk against the Chain V (StructuralTransit
//! carrier lowering) and Chain X (fact-backed DeclRef keyspace
//! admission) paths accidentally routing a Conditional macro payload
//! through a transit-shallow path that bypasses
//! `PayloadSurfaceScope::EmitClassMacroObject`'s branch-merge protocol.
//!
//! Top risk: a StructuralTransit expansion of V can regress
//! inherited-emits if Conditional macro payloads bypass
//! EmitClassMacroObject branch merge.
//!
//! The path-precision predicate `macro_payload_root_is_conditional_carrier`
//! keeps Conditional macro payload roots on
//! the `Published(Expanded)` rail so the inherited-emits branch-merge
//! protocol can enumerate both branches' event rows. The
//! materialiser changes live BELOW the macro publication entry —
//! `reduce_field_type_expr_with_mode` runs AFTER the macro publication
//! layer has decided whether the root is Conditional and routed
//! accordingly. The materialiser never sees a Conditional macro
//! payload root directly, so the invariant is preserved.
//!
//! It mirrors the Conditional-emits-merge fixture and asserts the
//! inherited-emits surface survives the Chain V + Chain X paths.

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
fn round11_inherited_emits_branch_merge_survives_v_x_chain_closures() {
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
            "inherited-emits invariant — \
             Chain V (StructuralTransit carrier lowering in \
             `materialize_component_meta_type_expr_until_stable_full`) \
             and Chain X (fact-backed `DeclRef` keyspace admission in \
             `keyspace_admits_literal_non_emitting`) MUST NOT regress \
             the inherited-emits branch-merge surface. The parent's \
             `accepted_events` MUST include the child's `{required}` \
             from BOTH branches of the conditional \
             `Mode extends 'editor' ? EditorEmits : ViewerEmits`. A \
             regression here means a Conditional macro payload \
             was routed through one of these code paths — the \
             path-precision predicate \
             `macro_payload_root_is_conditional_carrier` must keep \
             Conditional roots on the \
             `Published(Expanded)` rail so \
             `resolve_payload_surface_with_scope(EmitClassMacroObject)` \
             can enumerate both branches' event rows. The \
             materialiser changes run BELOW macro publication and \
             never see a Conditional root directly. Got events: \
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
