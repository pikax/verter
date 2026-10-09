# Inline class-evaluation effects inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

This task adds no counter, trace, store or retained field. It changes which
class-declaration positions the flow skeleton records and the slice content
lowers, and it adds per-operation bookkeeping, classified here so the
inventory covers every item it changes.

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SkeletonBuilder` class-declaration hook: an inline class declaration's `extends` value recorded as a root expression site and each static block as a `Block` region of the frame | The flow graph, demand planner and proof obligations of the enclosing function (the writes, reads and calls those positions perform) | REQUIRED | Part of the `FunctionBodySkeleton` built once per function content version; retired with the function's flow-graph bundle | `src/analysis/flow/mod.rs` | always |
| `InlineClassEvaluation` (heritage expression and static-block borrows) returned by `inline_class_evaluation` | The skeleton hook above, the slice content's class-declaration statement and the local-class value read, which must agree on the inline positions | REQUIRED | An on-demand value over the borrowed AST, built per call and dropped with it; never stored | `src/analysis/flow/class_evaluation.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Class-declaration statement lowering: heritage effects as a discarded value's statement, each static block as a `SliceStatement::Block` | The evaluator's in-order write, assertion and completion application | REQUIRED | Part of the `SliceContent` one flow-slice lowering produces | `src/flow_slice_content.rs` | always |
| `evaluated_at_statement` local in `lower_class_expression` (a local class declaration read as a value whose inline positions already ran) | Skips re-applying heritage effects (`lower_evaluated_heritage`) and re-scanning class-evaluation positions at the value read | REQUIRED | One class lowering | `src/flow_slice_content_class.rs` | always |
