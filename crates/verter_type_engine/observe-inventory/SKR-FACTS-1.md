# Producer fact inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ObservedFlowRead` (`value`, captured cold-compute `completeness`, captured build-local taint `frame`) | The flow-return member probe, the callee scheduler and the degraded-return check decide adopt-or-discard from it; `adopt` folds the captured completeness and frame into the enclosing build | REQUIRED | One scoped flow read: captured when the read returns, consumed by `adopt` or `discard` before the caller continues | `src/project_semantic_dispatch/flow_return_fact.rs` | always |
| Flow-return fact mapping (`flow_return_fact`, `ObservedFlowRead::fact`) | The member probe's consume-or-decline decision | REQUIRED | Computed per read; stores nothing | `src/project_semantic_dispatch/flow_return_fact.rs` | always |
| Conditional reading fact (`ConditionalFact`, `conditional_read_fact`) and its `UndecidedConditional` causes | Relation and closedness consumers branch on the exact reading; the causes are carried on the unavailable arm only | REQUIRED (compact causes) | Computed per read; stores nothing | `src/project_semantic_dispatch/conditional_decision.rs` | always |

No optional counter, trace or cause history is added: detailed cause/taint histories stay out of this change, so nothing is gated behind `semantic-observe`.
