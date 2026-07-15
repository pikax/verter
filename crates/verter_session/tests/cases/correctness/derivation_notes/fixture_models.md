Verter rule `./.claude/skills/component-meta` (Verter macros §model) — `defineModel<T>()` typed model lowering

`./.claude/skills/component-meta` documents the Verter rule for the
`defineModel<T>()` macro:

> defineModel<T>() exposes a model entry per call, with name from the
> optional first string argument (or 'modelValue' default) and type
> from the type parameter T.

Vue 3's documented `defineModel<T>()` contract additionally:

- emits a corresponding `<model_name>` prop whose type is `T |
  undefined` when the model is optional (default — no `{ required:
  true }` option) and not defaulted; `required: false`,
  `has_default: false`.
- emits an `update:<model_name>` event whose payload tuple is
  `[value: T | undefined]` for the same reason.

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
defineModel<string>();
defineModel<number>('count');
</script>
<template><div /></template>
```

Resolution semantics:

1. First call `defineModel<string>()` — no string argument, so the
   model name defaults to `modelValue`. `T = string`. The model
   entry is `{ name: "modelValue", type_expr: string }`.
2. Second call `defineModel<number>('count')` — the first string
   argument `'count'` becomes the model name. `T = number`. The
   model entry is `{ name: "count", type_expr: number }`.
3. Each model emits a synthesised prop with the same name. Per
   Vue's optional-by-default contract, the prop's type is
   `T | undefined`, with `required: false` and `has_default: false`
   (no `{ default: ... }` option present).
4. Each model emits a synthesised `update:<name>` event whose
   payload tuple is `[value: T | undefined]`.
5. The `SnapshotView` projection sorts every collection
   alphabetically by name. The fixture's surfaces are therefore
   ordered:
   - props: `[count: "number | undefined", modelValue: "string |
     undefined"]`.
   - models: `[count: "number", modelValue: "string"]`.
   - events: `[update:count: "[value: number | undefined]",
     update:modelValue: "[value: string | undefined]"]`.

Component-meta surface: two props, two events, zero slots, two
models, zero exposed bindings, no fallthrough surface.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::ModelDropped` — surfacing only one of `count` /
  `modelValue` (or neither) would mean the macro extractor missed a
  call. Detected.
- Implicit prop / event mutations — flipping `required` to `true`
  or removing `update:<name>` events would mean the macro's
  synthesised prop / event surfaces drifted. Detected via the
  fixture's prop / event signatures.

Phase linkage:
- `phase-00b-tier1-mismatches.md` row 2 documented the deferred
  rule-correct expected (models with concrete types, props with
  `T | undefined` shape, matching update events). Re-homed from
  Phase 5k to Phase 5j per parent §5.13 r15 table because
  `defineModel` shares the same `ResolveMacroPayload` /
  field-projection codepath as `defineSlots`.
- Phase 5j §5.12 closes the gap via the
  `compute_evaluated_types*` `expand_field_expr` closure in
  `crates/verter_session/src/host_manage.rs` (adds a `DefineModel`
  branch that lower+raises `parsed_type_argument` directly,
  bypassing the path-projection arm — the macro's `T` IS the
  field's type, not a parent shell with member-named children).
  The fixture is authored as a regression guard for that branch.
