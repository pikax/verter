# Verter rule: `withDefaults` populates `default_signature` and flips `required`

Source: `./.claude/skills/component-meta` and CLAUDE.md §Component-Meta —
the resolver derives the prop surface from `defineProps<T>()` and
overlays `withDefaults` defaults onto the resulting `PropAnalysis`
records.

For the fixture

```ts
withDefaults(defineProps<{ name: string; count?: number }>(), { count: 0 });
```

Verter's contract for each declared prop is:

| Prop name | source-optional | has default | `required` | `default_value` |
|-----------|-----------------|-------------|-----------|-----------------|
| `name`    | no              | no          | `true`    | `None`          |
| `count`   | yes (`?`)       | yes (`0`)   | `false`   | `Some("0")`     |

Discriminating-test linkage (§0p.A.5):
- `MutationKind::PropDefaultDropped` — flipping `count.default_signature`
  from `Some("0")` to `None` must fail the gate. The fixture has
  exactly one prop with a non-`None` default, so the mutation always
  has something to drop.
- `MutationKind::PropRequiredFlipped` — flipping `name.required` from
  `true` to `false` must fail the gate. The fixture's `name` prop is
  the required-prop anchor.

Negative assertions (encoded in `expected.rs`):
- `name.default_signature` is `None` (it never had a withDefaults
  entry).
- `count.required` is `false` (the default makes the runtime value
  always present).
