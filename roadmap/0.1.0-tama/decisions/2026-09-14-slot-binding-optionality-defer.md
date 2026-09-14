# DEFER ruling and debt row: slot-binding row optionality on the public surface

- Status: proposed — awaiting maintainer ratification
- Date: 2026-09-14
- Adds: no DAG node. Records the debt row the ECRV4 candidate leaves behind so the deferral is a disposition, not a TODO.
- Scope: one finding raised during the ECRV4 candidate's review, following the convention of `2026-09-14-materialize-structure-wire-mirror-defer.md`.

## Context

ECRV4 repairs one localized representation cause in the slot-binding
publication: a concrete binding value fell through every publication arm onto
the `SyntheticSlotBinding` carrier, whose rendering IS the binding name, so the
public component-meta surfaces reported a binding's NAME as its type
(`{ open: open; }`, `{ meta: meta; }`). Publication now selects the complete
closed fact the value node already decided — `Closed(Leaf)` / `Closed(LeafUnion)`
for a leaf value, and the bounded depth-closed `Synthesized(Object)` surface for
a payload object whose own members are all leaves — before the carrier fallback.

A binding's DECLARED OPTIONALITY (`default(props: { open?: boolean })`) is a
separate channel from its type, and it is lost on a different path.

## Debt row — `SLOT-BINDING-ROW-OPTIONALITY`

- **Finding:** the `?` on a slot-binding parameter member does not reach any
  public surface. `publish_merged_bindings` has the graph binding's `optional`
  flag, but `SlotBindingAnalysis` (`crates/verter_semantic/src/analysis/component_meta.rs`)
  carries no `optional` field, so the native `_verter.slots[].bindings[]` row has
  none either (`NativeSlotBindingMeta`), and the compat projection
  (`buildSlotBindingsDescriptor` in `packages/component-meta/src/compat/checker.ts`)
  hardcodes `optional: false` — the compat schema reports `required: true` for an
  optional binding. The binding's TYPE is unaffected: an optional binding
  publishes its declared leaf exactly like a required one, and an optional
  MEMBER of a bounded payload object does keep its optionality (the synthesized
  member fact carries it).
- **Why deferred:** this is a distinct plumbing cause from the representation
  cause ECRV4 repairs. Carrying binding optionality end-to-end adds a field to
  the semantic analysis row, the component-meta proto message and its generated
  Rust/TS bindings, the napi surface, and the compat projection — a public
  wire/schema widening across more than the two related packages ECRV4 is scoped
  to, and one ECRV4's acceptance explicitly forbids introducing.
- **Durable owner block:** the component-meta public-surface block that next
  bumps the component-meta payload schema (to be named by the maintainer at
  ratification).
- **Resolution gate:** no later than plan close; it lands with the next
  component-meta schema bump so the wire changes once.
- **Acceptance ID/test:** the retirement carries `optional` from
  `publish_merged_bindings` to `SlotBindingAnalysis`, the native binding row, and
  the compat descriptor, and extends the current optional controls — the
  `optional(props: { flag?: boolean })` slot in
  `packages/component-meta/src/native-eval.spec.ts` and the optional payload
  member in `bounded_leaf_member_payload_slot_binding_publishes_closed_surface`
  (`crates/verter_session/src/meta_resolve/slot_binding_graph_tests.rs`) — to
  assert the binding row itself reports `required: false` through the compat
  schema. Until then those controls pin the current contract: the declared type
  survives, the row-level `?` does not.
- **Ruling reference:** this decision (ECRV4 candidate review, conformance P2).

## Decision

1. The debt row above is the disposition of record for the binding-level
   optionality gap; it is not a TODO in source.
2. Ratification assigns the row its owner block; until then it is an open
   deferral counted at plan close.
