TS spec §4.5 — Template literal types in mapped key positions

TS spec §4.5 defines template literal types and their interaction
with mapped types. A mapped type's `as <expr>` clause re-maps the
iterated key:

```ts
type R = { [K in Source as <expr>]: Value };
```

When `<expr>` is a template literal type `\`<text>${K}<text>\``,
the iterated `K` substitutes into the template; the resulting
template-literal type folds to a string literal when every
substituted expression resolves to a string literal.

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
type R = { [K in 'A' | 'B' as `prefix${K}`]: number };
defineProps<R>();
</script>
<template><div /></template>
```

Resolution semantics:

1. `Source = 'A' | 'B'` enumerates the iterated key set.
2. For each `K` in the source set:
   - K = `'A'` substitutes into `\`prefix${K}\`` ->
     `\`prefix${'A'}\`` -> folded literal `"prefixA"`.
   - K = `'B'` substitutes into `\`prefix${K}\`` ->
     `\`prefix${'B'}\`` -> folded literal `"prefixB"`.
3. The mapped value is the constant `number` for each key.
4. The produced object surface is
   `{ prefixA: number; prefixB: number }`. Both members are
   non-optional (no `?` modifier on the mapped type).
5. The snapshot projection (`SnapshotView::from_analysis`) sorts
   props alphabetically by name, so the surface order is
   `[prefixA, prefixB]`.

Component-meta surface: two required props — `prefixA: number` and
`prefixB: number`. No events, slots, models, exposed bindings, or
fallthrough surface (the SFC uses no defineEmits / Slots / Model /
Expose; no defineOptions; no template content beyond `<div />`).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropMissingKey` — surfacing only `prefixA`
  (or only `prefixB`, or neither) would mean the iteration over
  the source union dropped a member. Detected.
- `MutationKind::PropExtraKey` — surfacing keys named `'A'` /
  `'B'` (the pre-Phase-5i behaviour where `name_remap` was
  ignored) would mean the resolver did not apply the `as` clause.
  Detected.
- `MutationKind::PropTypeShape` — surfacing keys but with type
  `/*unknown*/ semanticMiss` would mean the value evaluator
  failed to produce the constant `number`. Detected.

Phase linkage:
- `phase-00-tier1-mismatches.md` row 3 documented the deferred
  rule-correct expected (`props = [prefixA: number, prefixB: number]`).
- Phase 5i §5.11 (re-homed from 5k per §5.13 r15 table) closes
  the gap via two changes in
  `crates/verter_session/src/project_semantic_dispatch/build.rs`
  (apply `mapper.name_remap` per iteration in `build_mapped_type`)
  and
  `crates/verter_session/src/project_semantic_dispatch/evaluate.rs`
  (fold `SemanticNodeData::TemplateLiteral` to a `Literal::String`
  when every expression resolves to a literal). The fixture is
  authored as a regression guard for both pieces.
