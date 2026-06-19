# Verter rule: `defineExpose({...})` surfaces every key as exposed

Source: `./.claude/skills/component-meta` plus
`crates/verter_semantic/src/analysis/macros.rs::extract_expose_fields`
and `component_meta.rs::resolve_exposed_type`, which together form
the authoritative production path that fills the `ExposedAnalysis`
records.

§0p.A.0 author-first discipline note: Vue's documented public API
for `defineExpose` is the value form `defineExpose({ ... })`. The
type-only `defineExpose<T>()` syntax mentioned in §0p.A.2's fixture
table is not part of the documented Vue 3 macro contract, and
`extract_expose_fields` is value-based. §0.6.1 (small decision)
permits choosing the equivalent form whose behaviour the
discriminating self-test row (`ExposedDropped`) is form-agnostic
about. The fixture therefore uses the value form with each binding
typed explicitly so `resolve_exposed_type` finds a non-empty
annotation:

```ts
const focus: () => void = () => {};
const reset: () => void = () => {};
defineExpose({ focus, reset });
```

The resolver yields two `ExposedAnalysis` records:

| Exposed name | `type_expr`           | rendered signature |
|--------------|-----------------------|--------------------|
| `focus`      | function `() => void` | `() => void`       |
| `reset`      | function `() => void` | `() => void`       |

The snapshot view sorts exposed entries alphabetically; both names
share the same first letter ordering as alphabetised.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::ExposedDropped` — removing one exposed entry from
  the view must fail the gate. The fixture has two exposed methods
  so the property is exercised.

Negative assertion: `props`, `events`, `slots`, `models`, and
`fallthrough` are all empty / `None` — the SFC has only
`defineExpose`.
