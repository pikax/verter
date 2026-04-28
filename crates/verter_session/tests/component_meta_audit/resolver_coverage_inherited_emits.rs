//! Phase 5b §5.A — TDD seed for resolver coverage gap: inherited
//! emits surface from a child component, projected through a
//! branch-merged surface, do not currently resolve.
//!
//! **Root cause (per sub-plan §5 commit 7):** engine's
//! `evaluate_value_expression_via_env_or_dispatch` returned `None`
//! for emit types in branch-merged surfaces because
//! `project_expr_surface_expr` rejected non-`Object` roots. The
//! `ProjectPath{[],Expanded}` dispatch path resolves union branches
//! correctly.
//!
//! **Pre-Phase-5b behaviour:** the parent SFC inherits its child
//! component's emits, but the surface that would carry them resolves
//! to a non-`Object` shape (the dispatch failure swallows the emit
//! payload), so the inherited event names never reach the parent's
//! `analysis.events`.
//!
//! **Post-Phase-5b expected:** once `ProjectPath` becomes the dispatch
//! surface for inherited-emits (5f §7), the parent emits include
//! every event name from the child's `defineEmits<...>` macro.
//!
//! This seed remains RED through the end of Phase 5b. It closes in
//! 5f §7 via callsite migration in `fallthrough_resolver.rs`.

use crate::harness::{build_hermetic_host_with_lib, resolve_under_audit, STUB_LIB_ES5};

/// Parent uses a child component as its single root and forwards the
/// child's emits. The child's `defineEmits` type comes from a
/// CONDITIONAL union (`Mode extends 'editor' ? EditorEmits :
/// ViewerEmits`) that the resolver must distribute through to surface
/// the inherited surface. The conditional is intentionally OPEN at
/// the macro site — the type parameter `Mode` is unbound, so Verter
/// must merge BOTH branches into the inherited emit surface.
///
/// Pre-fix, `evaluate_value_expression_via_env_or_dispatch` in the
/// engine returns `None` for the conditional shape because the
/// underlying `project_expr_surface_expr` rejects non-`Object` roots.
/// `ProjectPath{[],Expanded}` correctly distributes a conditional
/// over the union and hands back both branches' events.
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
fn resolver_coverage_inherited_emits_branch_merged_surface() {
    let host = build_hermetic_host_with_lib(
        &[("/parent.vue", PARENT_VUE), ("/child.vue", CHILD_VUE)],
        &[("lib.es5.d.ts", STUB_LIB_ES5)],
    );
    let (analysis, _resolution, _record) = resolve_under_audit(host, "/parent.vue");

    // Discriminating assertion: the parent's accepted-events set MUST
    // include the child's component-specific emits (NOT generic DOM
    // events that would leak in regardless). If dispatch-side
    // branch-merge drops the inherited surface, these custom names
    // are absent. Pre-fix, this fails.
    let accepted_event_names: Vec<String> = analysis
        .accepted_events
        .iter()
        .map(|e| e.name.clone())
        .collect();
    for required in ["itemEdited", "itemViewed"] {
        assert!(
            accepted_event_names.iter().any(|n| n == required),
            "inherited emit `{required}` (component-specific, not a native DOM event) must surface in parent.accepted_events; got {accepted_event_names:?}"
        );
    }

    // Negative: a phantom name the child does NOT declare must NOT
    // leak into the inherited set. `phantomEventXyz` is not a native
    // DOM event nor declared by the child.
    assert!(
        !accepted_event_names.iter().any(|n| n == "phantomEventXyz"),
        "inherited emits must not contain phantom events; got {accepted_event_names:?}"
    );
}
