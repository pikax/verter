# Verter rule: `defineSlots<T>` surfaces every key of `T` as a slot

Source: `./.claude/skills/component-meta` (slots resolution + binding
extraction) plus the precedent test
`crates/verter_session/src/host_manage_tests.rs::enrich_slot_bindings_from_imported_type`,
which establishes the resolver's contract for typed slots.

For the fixture

```ts
defineSlots<{
  default(props: { item: string }): any;
  named(props: { row: number }): any;
}>();
```

Each key of the type literal becomes one `SlotAnalysis`. The slot's
`bindings` come from the function's first parameter (the `props`
object). The slot view's `payload_signature` (per
`tests/correctness/snapshot_view.rs::slot_view_from`) renders the
binding map as `{ name: type[; ...] }` when bindings are non-empty.

Two slots, one binding each:

| Slot name | binding | type | rendered payload |
|-----------|---------|------|------------------|
| `default` | `item`  | `string` | `{ item: string }` |
| `named`   | `row`   | `number` | `{ row: number }`  |

Discriminating-test linkage (§0p.A.5):
- `MutationKind::SlotDropped` — removing one slot from the view must
  fail the gate. The fixture has two slots, both have non-empty
  bindings, so the property is exercised.
- `MutationKind::SlotPayloadChanged` — corrupting `default`'s
  `payload_signature` (e.g., to `__mutated__{ item: string }`) must
  fail the gate. The expected fingerprint is the binding-projected
  shape, not the raw return type.

Negative assertion: `props`, `events`, `models`, and `exposed` are
all empty (the SFC has only `defineSlots`).
