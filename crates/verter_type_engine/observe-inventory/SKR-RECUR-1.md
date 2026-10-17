# Recursive-evaluation bookkeeping inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ProjectSemanticDispatch.free_type_variables` (one binder-aware `FreeTypeVariables` summary per node: free type parameters, `infer`-reference presence) | `type_params_within` (the conditional decision's permissive/restrictive tests, a relation's deferred-conditional checks) and `mentions_free_type_variable` (`type_is_generic`) | REQUIRED | The dispatcher (one request); a summary is a function of a rooted node's payload, which does not change while the dispatcher holds it | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `ActiveInstantiations.instances` and `.declarations` (membership indexes beside the ordered `instantiate_active` stack) | `push_instantiate_active`'s same-instantiation refusal and `is_instantiate_active`'s recursive-reference back-edge | REQUIRED | Each entry from its push to its pop or leave; empty once the outermost build ends | `src/project_semantic_dispatch/mod.rs` | always |
| Failed-prefix receipt (`DemandCostReceipt::failed_prefix`, consumed as `Nesting::Folded`, identified by its own allocation) | The consumer's sealed receipt and every replay of it (`replay_admit`'s closure charge, the footprint checks) | REQUIRED-budget | As long as the consumer's receipt that holds it | `src/project_semantic_dispatch/cost_receipt.rs`, `src/project_semantic_dispatch/connected_demand.rs` | always |

No optional counter, trace or history is added; the growth tests read the existing allocation counter of the allocator-canary binary and the existing fact-work test seam.
