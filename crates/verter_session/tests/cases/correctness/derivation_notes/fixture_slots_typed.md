Verter rule `./.claude/skills/component-meta` (Verter macros §slots) — `defineSlots<T>` typed-binding lowering

`./.claude/skills/component-meta` documents the Verter rule for the
`defineSlots<T>()` macro:

> defineSlots<T> must surface every key of T as a slot, with bindings
> extracted from each slot function's first parameter.

A slot's "binding" is a per-name + per-type entry whose type is the
leaf type of the named property in the slot function's first
parameter Object literal. For the SFC fixture below, the rule
mechanically determines the resolved slot binding shapes.

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
</script>
<template><div /></template>
```

Resolution semantics:

1. The macro's type argument T is the inline Object literal
   `{ default(...): any; named(...): any }`. Each member is a method
   shape whose value lowers to a `Function` carrier with one
   parameter and a return type.
2. For each slot key K in T:
   - Take the slot member's value (the `Function` carrier).
   - Read `params[0].ty` — the slot function's first-parameter type.
     This is always an Object literal `{ name: T_name; ... }` per
     Vue's slot scope binding contract.
   - Each Object property `{ name: ty }` becomes a slot binding
     with `name = name` and `type_expr = ty`.
3. For the fixture above, this yields:
   - Slot `default` has one binding `item: string`.
   - Slot `named` has one binding `row: number`.
4. The `SnapshotView` projection sorts slots alphabetically by name
   and renders each slot's `payload_signature` as `{ <bindings sorted
   alphabetically>; }`. The fixture's slot list is therefore
   `[default, named]` and each `payload_signature` is exactly
   `{ <name>: <type> }`.

Component-meta surface: zero props/events/models/exposed bindings,
no fallthrough surface (the SFC declares no other macros, no
`defineOptions`, and the template's `<div />` is intrinsic). Two
slots — `default` with `payload_signature = "{ item: string }"`, and
`named` with `payload_signature = "{ row: number }"`.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::SlotDropped` — surfacing only `default` (or only
  `named`, or neither) would mean the slot extractor did not iterate
  every key of T. Detected.
- `MutationKind::SlotPayloadChanged` — surfacing the slot but with
  a binding name or type that does not match the parameter Object
  literal would mean the binding extractor walked a different path.
  Detected. (Pre-Phase-5j Verter produced
  `{ item: /*unknown*/ semanticMiss }` because the slot value's
  `Function` was not descended into; the post-Phase-5j helper
  `ProjectSemanticDispatch::project_slot_binding_member` closes that
  gap.)

Phase linkage:
- `phase-00b-tier1-mismatches.md` row 1 documented the deferred
  rule-correct expected (slots = `[default: { item: string }, named:
  { row: number }]`).
- Phase 5j §5.12 closes the gap via
  `crates/verter_session/src/project_semantic_dispatch/mod.rs`
  (adds `project_slot_binding_member` non-variant dispatch helper)
  and the `expand_field_expr` closure in
  `crates/verter_session/src/host_manage.rs::compute_evaluated_types*`
  (routes `FieldKind::SlotBinding` through the helper instead of the
  generic 2-segment `ProjectPath`). The fixture is authored as a
  regression guard for both pieces.
