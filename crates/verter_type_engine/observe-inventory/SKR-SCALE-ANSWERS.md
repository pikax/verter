# Scale-answer inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RelationDomainRuntime::collecting_ids` (the ids of the sessions that may still be collecting, in id order) | `active_session`, `active_session_mut` and `collecting_session_infers` search only these for the innermost visible collecting session, instead of every session the transaction opened | REQUIRED | One dispatch transaction; an id is pushed when its session opens and dropped once its session is found finished on top (a session suspended off the stack keeps its id) | `src/project_semantic_dispatch/dispatch_txn.rs` | always |

No counter, trace or charge history is added; the scale tests read the existing `SemanticGraphStats` counters, the connected-work capture and the test-only `DISPATCH_TRACE`, so nothing is gated behind `semantic-observe`.
