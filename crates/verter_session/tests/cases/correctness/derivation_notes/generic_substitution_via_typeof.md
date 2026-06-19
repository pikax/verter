# TS spec §3.6 — Generic substitution; CLAUDE.md "generic substitutions are part of semantic meaning"

When a generic type alias `IdShape<T>` is instantiated with a `typeof
<value-path>` argument, the resolver evaluates the value path to its
annotated type, binds `T` to that type, and substitutes through the
body. For `IdShape<typeof sample.id>` with `sample` typed as `Sample`
and `interface Sample { id: string }`, the projection
`typeof sample.id` evaluates to `string`. Substituting `T → string`
into the body `{ id: T }` produces `{ id: string }`, surfacing one
required prop `id: string`.

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
interface Sample { id: string }
const sample: Sample = { id: "abc" };
interface IdShape<T> { id: T; }
defineProps<IdShape<typeof sample.id>>();
</script>
<template><div /></template>
```

Resolution semantics:

1. The macro arg `IdShape<typeof sample.id>` lowers through
   `shallow_lower_type_expr` (the `TypeExpr::Ref { name: "IdShape",
   type_arguments: [TypeOf(...)] }` arm) — `IdShape` is not a
   builtin and not in `name_resolution`, so it falls through to the
   bare-name resolver, which finds the local `interface IdShape<T>`
   declaration.
2. The type argument `typeof sample.id` lowers through the
   `TypeExpr::TypeOf` arm. Per Phase 5k §5.13, the lowering attempts
   single-segment root resolution first: `value_root.name = "sample"`.
   `build_typeof` resolves the const declaration `sample: Sample`,
   substitutes the type annotation, and returns the surface
   `{ id: string }` (one level deep).
3. The remaining path `["id"]` is projected through
   `ProjectPath { mode: Navigate }`, walking into the resolved
   surface and producing the member type `string`.
4. With `T = string`, `Instantiate` substitutes the body
   `{ id: T }` → `{ id: string }`. The materialiser produces one
   required prop `id: string`.

The snapshot projection sorts props alphabetically by name, so the
surface order is `[id]`.

Component-meta surface: one required prop. No events, slots, models,
exposed bindings, or fallthrough surface (the SFC has no
defineEmits/Slots/Model/Expose; no defineOptions; no template
content beyond `<div />`).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropTypeChanged` — surfacing `id: T` (the
  unsubstituted type parameter, the pre-Phase-5k Verter behaviour)
  instead of `id: string` would mean the substitution layer never
  observed the resolved typeof argument. Detected.
- `MutationKind::PropMissingKey` — surfacing zero props would mean
  the macro argument failed to resolve at all, dropping IdShape's
  member. Detected.

Phase linkage:
- `phase-00-tier1-mismatches.md` row 4 documented the deferred
  rule-correct expected (`id: string`).
- Phase 5k §5.13 amended `shallow_lower_type_expr`'s
  `TypeExpr::TypeOf` arm
  (`crates/verter_session/src/project_semantic_dispatch/lower.rs`)
  to attempt single-segment root resolution first, falling back to
  the joined-2-segment lookup only when the single-segment root
  misses AND a longer path exists. The fallback preserves the
  namespace-member semantics for `import * as Ns; typeof Ns.Foo[.Bar...]`
  shapes; the primary path closes the value-member projection gap.
  The fixture is authored as a regression guard.
