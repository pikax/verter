Verter rule `./.claude/skills/type-resolution` — TS-first resolution priority + user shadowing wins

Verter's macro resolver follows TypeScript's lexical-scope rule for
type names: a userland `type Pick<T, _K> = T` declared in the SFC's
script-setup scope SHADOWS the ambient lib's `Pick<T, K extends
keyof T>`. The shared cross-file type resolver (CLAUDE.md "Macro
Type Traversal Rule") performs an outward lexical-scope walk before
falling back to ambient lib declarations, so the userland alias
wins over the same-named builtin utility.

Source SFC fixture (`/c.vue`):

```vue
<script setup lang="ts">
type Pick<T, _K> = T;
interface Cfg {
  alpha: string;
  beta: number;
  gamma: boolean;
}
defineProps<Pick<Cfg, 'alpha'>>();
</script>
<template><div /></template>
```

Resolution semantics:

1. The userland `Pick<T, _K> = T` is declared in the same-file
   scope BEFORE the `defineProps<Pick<Cfg, 'alpha'>>()` call.
2. `Pick` resolves to the userland alias (TS-first / user-shadowing
   rule). Substituting `T = Cfg` and discarding `_K`, the type
   evaluates to the bare `Cfg` interface.
3. `defineProps<Cfg>()` therefore declares all three members of
   `Cfg`: `alpha: string`, `beta: number`, `gamma: boolean`. All
   three are non-optional in the source, so all three surface as
   required props.
4. The snapshot projection sorts props alphabetically by name, so
   the surface order is `[alpha, beta, gamma]`.

Component-meta surface: three required props. No events, slots,
models, exposed bindings, or fallthrough surface (the SFC has no
defineEmits/Slots/Model/Expose; no defineOptions; no template
content beyond `<div />`).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropMissingKey` — surfacing only `alpha` (the
  lib's mapped Pick output, filtered by the second type argument)
  would mean the resolver bypassed the userland shadow. Detected.
- `MutationKind::PropExtraKey` — surfacing extra members would
  mean the resolver pulled in unrelated declarations. Detected.

Phase linkage:
- `phase-00-tier1-mismatches.md` row 5 documented the deferred
  rule-correct expected (`[alpha, beta, gamma]`).
- Phase 5h §5.10 introduced the resolver-context `ScopeShadowing`
  struct (`crates/verter_session/src/resolver_core/scope_shadowing.rs`)
  and threaded it through both the dispatch-lowering entry
  (`shallow_lower_type_expr`) and the materialise-path registry
  route fast-path (`project_expr_class_a_via_dispatch_threaded`),
  closing the gap. The fixture is authored as a regression guard.
