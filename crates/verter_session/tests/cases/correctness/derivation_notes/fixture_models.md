Verter rule `./.claude/skills/component-meta` (Vue model props) — value and modifiers membership

The pinned Vue 3.6.0-rc.5 compiler's `genModelProps` emits a value prop and an
optional modifiers prop for every `defineModel` call. The names are
`modelValue` / `modelModifiers` by default, or `<name>` / `<name>Modifiers`.
The pinned declaration generator uses `Partial<Record<M, true>>` for an explicit
second type argument; absent M, the record is `Record<string, true | undefined>`.
See `.claude/skills/component-meta/references/native-facts.md` and the pinned
runtime oracle in `tests/fixtures/vue_macro_runtime_oracle.json`.

The source fixture calls `defineModel<string>()` and
`defineModel<number>('count')`. Neither call supplies M, `required`, or a default.
Its independently authored expected surface therefore has:

- Four optional props with no defaults: `count: number`,
  `countModifiers: Record<string, true | undefined>`,
  `modelModifiers: Record<string, true | undefined>`, and `modelValue: string`.
- Two value-only models: `count: number` and `modelValue: string`.
- Two events: `update:count` with `[value: number | undefined]`, and
  `update:modelValue` with `[value: string | undefined]`.
- No slots, exposed bindings, or fallthrough surface.

Native value prop sources remain bare T. Optionality is a separate required flag;
compat applies the optional read projection. `SnapshotView` sorts rows by name,
which gives the order above. Union member order follows the shared semantic
canonical order and does not change the Vue record's accepted values.

The Class A JSON snapshot is generated from `expected::fixture_models()` through
`generate_class_a_snapshots_from_expected`, rather than captured from Verter.
The correctness comparison and its mutations detect omitted props, changed
required flags, missing events, and missing model bindings.
