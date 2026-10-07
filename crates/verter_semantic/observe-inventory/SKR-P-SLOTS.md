# Slot-binding partition inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SlotBindingIndex::by_slot`: evaluated slot-binding rows partitioned once by exact slot identity (typed `(slot_name, binding_name)` pairs; flat `slot.binding` keys under every `.` prefix), each bucket in evaluator row order | Every slot join in `extract_component_meta` (authored, shape-only and framework-native slot lanes) | REQUIRED | Borrowed from the caller's `ExpandedComponentTypes`; built once per `extract_component_meta` call and dropped when it returns | `src/analysis/component_meta.rs` | always |
| Slot-binding row binding identity and status (binding name, authority including `Failed` positions, authored evidence, expansion metadata) carried through the partition into `SlotBindingAnalysis` | Published slot bindings in component metadata | REQUIRED | Owned by the published `ComponentMetaAnalysis` | `src/analysis/component_meta.rs` | always |
| `SLOT_BINDING_ROW_VISITS` thread-local counter and `take_slot_binding_row_visits` reader: rows inspected while partitioning plus bucket entries read while joining | none; tests and benchmark readers only | OPTIONAL | Per thread; the reader resets it | `src/analysis/component_meta.rs` | `cfg(any(test, feature = "semantic-observe"))` |

Slot joins read only their own bucket, so no slot, duplicate-name lookup or
template-slot merge rescans the whole binding or slot population. The counter
is added to once per partition and once per slot join, never once per row.
