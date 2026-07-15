# CLAUDE.md §Fallthrough — single component root propagates child surface

Source: CLAUDE.md "Fallthrough / Root Inheritance (CRITICAL)" plus
`./.claude/skills/component-meta` "Fallthrough / Root Inheritance
(CRITICAL)" Semantic rules — third-from-last bullet:

> Single component root -- recursive propagation through the child's
> full public surface

For the fixture, the wrapper SFC is

```vue
<script setup lang="ts">
import Inner from './inner.vue';
</script>
<template><Inner /></template>
```

and the imported child is

```vue
<script setup lang="ts">
defineProps<{ label: string }>();
</script>
<template><div /></template>
```

The wrapper has a single component root `<Inner />`, no declared
props, no `inheritAttrs: false`. Per the rule above, the inherited
fallthrough surface IS the child's accepted surface — one prop
`label: string` whose `InheritedSource` is
`Component { canonical_id: "/inner.vue" }`.

The snapshot view's `build_fallthrough_view` emits the projection
because at least one branch entry has a component source (the (b)
arm of the projection rule documented in `snapshot_view.rs`). The
`format_inherited_sources` helper appends ` /* from
component:/inner.vue */` to each entry. With `inherit_attrs = true`
(the wrapper did not opt out) the projection is

```
Some(FallthroughView {
  inherit_attrs: true,
  surface_signature: "{ label: string /* from component:/inner.vue */ }",
})
```

Discriminating-test linkage (§0p.A.5):
- `MutationKind::FallthroughSurfaceChanged` — corrupting
  `fallthrough.surface_signature` (e.g., to `__mutated__{ label:
  string ... }`) must fail the gate. The fixture has
  `Some(FallthroughView)` with a non-empty surface so the mutation
  has a live target.

Negative assertion: the wrapper's `props` is empty (it declares
none); `events`, `slots`, `models`, and `exposed` are also empty.
The fallthrough surface does NOT include `class` or `style` (Vue's
fallthrough always merges those silently and `format_inherited_sources`
should not surface them).
