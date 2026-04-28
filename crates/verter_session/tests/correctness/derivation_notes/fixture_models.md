# Verter rule: `defineModel<T>()` exposes a model entry per call

Source: `./.claude/skills/component-meta` plus
`crates/verter_semantic/src/analysis/macros.rs::extract_define_model_type`,
which is the authoritative production path that fills the
`AnalyzedPropField` records used to build `ModelAnalysis`.

For the fixture

```ts
defineModel<string>();
defineModel<number>('count');
```

The resolver yields two `ModelAnalysis` records:

| Source                        | Model name    | Type       |
|-------------------------------|---------------|------------|
| `defineModel<string>()`       | `modelValue`  | `string`   |
| `defineModel<number>('count')`| `count`       | `number`   |

When the call passes a string-literal first argument, the
`AnalyzedMacro::model_name` field is the literal value. Otherwise the
default name is `modelValue` (verified by `extract_define_model_type`
line "let name = model_name.as_deref().unwrap_or(\"modelValue\")").

The snapshot view sorts models alphabetically, so the projection
order is `count` (c) before `modelValue` (m).

Discriminating-test linkage (§0p.A.5):
- `MutationKind::ModelDropped` — removing one model from the view
  must fail the gate. The fixture has two models so the property is
  exercised after the mutation drops the first.

Negative assertion: `props`, `events`, `slots`, `exposed`, and
`fallthrough` are all empty / `None` — the SFC has only two
`defineModel` calls and no other macros.
