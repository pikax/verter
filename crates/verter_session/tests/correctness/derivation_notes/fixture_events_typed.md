# Verter rule: `defineEmits<T>` preserves T's parameter shape

Source: `./.claude/skills/component-meta` (event resolution) plus
`crates/verter_semantic/src/analysis/component_meta.rs::event_raw_signature_from_evaluated_and_source`,
which is the authoritative production path that fills
`EventAnalysis::raw_signature`.

For the fixture

```ts
defineEmits<{ click: [evt: string] }>();
```

The resolver yields one `EventAnalysis`:

| Event name | `payload` (TypeExpr) | `raw_signature` (Option) |
|------------|----------------------|--------------------------|
| `click`    | tuple `[evt: string]` | `Some("[evt: string]")`  |

The snapshot view's `event_view_from` prefers `raw_signature` over
the rendered `TypeExpr` when present, so the projected event
`params_signature` is the literal source-form `[evt: string]` —
labelled element with primitive type.

Discriminating-test linkage (§0p.A.5):
- `MutationKind::EventSignatureChanged` — corrupting the first
  event's `params_signature` must fail the gate. The fixture has
  exactly one event so the mutation always has something to flip.

Negative assertion: `props`, `slots`, `models`, `exposed`, and
`fallthrough` are all empty / `None` — the SFC has only
`defineEmits`.
