TS spec §4.4 — Predefined `Extract<T,U>` utility (distributive conditional)

`Extract<T, U>` is defined in `lib.es5.d.ts` as the distributive
conditional:

```ts
type Extract<T, U> = T extends U ? T : never;
```

When `T` is a union, the conditional distributes over each member
(TS spec §3.11). For each member `M` of `T`, `Extract` evaluates
`M extends U ? M : never` — the survivors are exactly the members
of `T` that ARE assignable to `U`. The result is the union of those
survivors (or `never` if the survivor set is empty).

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
defineProps<{ kind: Extract<'a' | 'b' | 'c', 'a' | 'b'> }>();
</script>
<template><div /></template>
```

Resolution semantics:

1. `T = 'a' | 'b' | 'c'`, `U = 'a' | 'b'`.
2. Distribution unfolds the union into three sub-conditionals:
   - `'a' extends 'a' | 'b' ? 'a' : never` -> `'a'` (literal `'a'`
     IS assignable to the union `'a' | 'b'`).
   - `'b' extends 'a' | 'b' ? 'b' : never` -> `'b'`.
   - `'c' extends 'a' | 'b' ? 'c' : never` -> `never`
     (literal `'c'` is NOT assignable to either filter literal).
3. The union of survivors is `'a' | 'b'`.
4. Verter's renderer (`render_type_signature`) prints the union
   in source order, double-quoting each literal and joining with
   ` | `, producing the canonical signature `"a" | "b"`.
5. There is exactly ONE prop on the SFC (`kind`); the snapshot
   contains exactly that prop with the canonical type signature.

Component-meta surface: one required prop `kind` of type
`"a" | "b"`. No events, slots, models, exposed bindings, or
fallthrough surface (the SFC uses no defineEmits / Slots / Model /
Expose; no defineOptions; no template content beyond `<div />`).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropMissingKey` — dropping `kind` would mean the
  resolver bypassed the macro path. Detected.
- `MutationKind::PropTypeShape` — surfacing `"a" | "b" | "c"`
  (no reduction) or `"c"` (inverted predicate) or
  `/*unknown*/ semanticMiss` (the pre-Phase-5i shell) would each
  diverge from the rule-correct signature. Detected.

Phase linkage:
- `phase-00-tier1-mismatches.md` row 2 documented the deferred
  rule-correct expected (`kind: "a" | "b"`).
- Phase 5i §5.11 closes the gap via the same `Extract` / `Exclude`
  arm in `build_builtin_utility` that closes `mapped_exclude`. The
  filter argument `U` is itself a union of literals, exercising
  the per-member relation engine path on BOTH sides — each source
  member is related against the union filter via `relate_nodes`,
  which decides assignability through the relation engine's
  union-distribution rule. The fixture is authored as a regression
  guard.
