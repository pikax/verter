# IDE condition narrowing inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_compiler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `FlowNarrowing::conditions`: chain branches enclosing the walk position | Whether a callback is re-narrowed (`callback_guard`) | REQUIRED (generated semantics) | One IDE template walk; reset with the walk context | `src/ide/template/flow.rs` | always |
| `FlowNarrowing::scopes` (`FlowScope`: reserved prepend, shape, snapshot declarations, snapshot index by reference text): the open statement scopes | Snapshot placement and deduplication; the scope's emitted declarations | REQUIRED (generated semantics) | Pushed when a branch block, frame or lifted branch opens; popped and emitted when it closes | `src/ide/template/flow.rs` | always |
| `FlowNarrowing::next_snapshot`, `FlowNarrowing::next_frame_source`: name counters | Unique `___VERTER___oN` / `___VERTER___vN` names | REQUIRED (generated semantics) | One IDE template walk | `src/ide/template/flow.rs` | always |
| `FlowNarrowing::typescript` | Non-null steps in snapshot text (TypeScript output only) | REQUIRED (generated semantics) | One IDE template walk | `src/ide/template/flow.rs` | always |
| `CodeGenOutput` reserved ordered prepends (`reserve_ordered_unmapped` / `fill_reserved`, `ReservedPrepend`) | Snapshot declarations placed in output order before the content walked after them | REQUIRED (generated semantics and mappings) | Entries of the output accumulator; unfilled ones are dropped when it is applied | `src/template/code_gen/types.rs` | always |
| `OxcParsedExpression::statements`: the statement list of a multi-statement `v-on` value | Outer references of a wrapped multi-statement handler | REQUIRED (generated semantics) | Owned by the `OxcParsedAst` of one template parse | `src/template/oxc/types.rs` | always |
| `FlowWork` counters (conditions, chain members, scopes, callbacks, outer references, snapshots), `FLOW_WORK` thread-local, `record` hook and `take_flow_work` reader | none; tests and measurement builds only | OPTIONAL | Per thread; the reader resets it | `src/ide/template/flow.rs` | `cfg(any(test, feature = "semantic-observe"))` |

Default builds compile `record` to an empty inline function; there is no
per-operation enabled check.
