TS spec §4.4 — Predefined `Exclude<T,U>` utility (distributive conditional)

`Exclude<T, U>` is defined in `lib.es5.d.ts` as the distributive
conditional:

```ts
type Exclude<T, U> = T extends U ? never : T;
```

When `T` is a union, the conditional distributes over each member
(TS spec §3.11). For each member `M` of `T`, `Exclude` evaluates
`M extends U ? never : M` — the survivors are exactly the members
of `T` that are NOT assignable to `U`. The result is the union of
those survivors (or `never` if the survivor set is empty).

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
defineProps<{ kind: Exclude<'a' | 'b' | 'c', 'b'> }>();
</script>
<template><div /></template>
```

Resolution semantics:

1. `T = 'a' | 'b' | 'c'`, `U = 'b'`.
2. Distribution unfolds the union into three sub-conditionals:
   - `'a' extends 'b' ? never : 'a'` -> `'a'` (string literal `'a'`
     is NOT assignable to `'b'`).
   - `'b' extends 'b' ? never : 'b'` -> `never`.
   - `'c' extends 'b' ? never : 'c'` -> `'c'`.
3. The union of survivors is `'a' | 'c'`.
4. Verter's renderer (`render_type_signature`) prints the union in
   source order, double-quoting each string literal and joining
   with ` | `, producing the canonical signature `"a" | "c"`.
5. There is exactly ONE prop on the SFC (`kind`); the snapshot
   contains exactly that prop with the canonical type signature.

Component-meta surface: one required prop `kind` of type
`"a" | "c"`. No events, slots, models, exposed bindings, or
fallthrough surface (the SFC uses no defineEmits / Slots / Model /
Expose; no defineOptions; no template content beyond `<div />`).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropMissingKey` — dropping `kind` would mean the
  resolver bypassed the macro path. Detected.
- `MutationKind::PropTypeShape` — surfacing `"a" | "b" | "c"`
  (no reduction) or `"b"` (inverted predicate) or
  `/*unknown*/ semanticMiss` (the pre-Phase-5i shell) would each
  diverge from the rule-correct signature. Detected.

Phase linkage:
- `phase-00-tier1-mismatches.md` row 1 documented the deferred
  rule-correct expected (`kind: "a" | "c"`).
- Phase 5i §5.11 closes the gap via the new
  `Extract` / `Exclude` arms in `build_builtin_utility`
  (`crates/verter_session/src/project_semantic_dispatch/build.rs`)
  which dispatch each source-union member through `relate_nodes`
  against the filter argument and reconstitute survivors via
  `intern_normalized_union_or_intersection`. The fixture is
  authored as a regression guard.
