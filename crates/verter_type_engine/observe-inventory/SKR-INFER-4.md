# Library reference inference inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `runtime_nominal_declaration` (the global declaration a runtime-nominal application such as `Promise<T>` or `Map<K, V>` refers to in its node's project, read through `first_global_declaration`, which records the library dependency; a scope-less carrier reads the demand's project and suppresses the enclosing memo) | Variance and type-argument inference (`generic_reference`), structural expansion (`unwrap_identity_carrier_for_relation`, `unwrap_identity_carrier_one_step`), relation recursion identity, and the lib `Awaited` conditional's declaration read (`declaration_carrier_key`) | REQUIRED | Stateless | `src/project_semantic_dispatch/relation_variance.rs` | always |
| `references_one_declaration` (two references to one generic declaration, `isTypeCloselyMatchedBy`) | The closely-matched pass of union inference and the recursion identity of an interface or class reference | REQUIRED | Stateless | `src/project_semantic_dispatch/relation_variance.rs` | always |
| `NakedUnionInference::{Relate, Matched}::closely` (source members paired with the fixed target member referencing their declaration) | The union relation infers and relates each pair before inferring the remainder to the naked parameter | REQUIRED | One relation step | `src/project_semantic_dispatch/relation.rs` | always |
| The interface and class arm of `relation_recursion_identity` (a reference to a declaration other than the pair's other operand's) | `isDeeplyNestedType` over generic-method interfaces whose defaults mint a new instantiation at every level | REQUIRED | One relation chain frame | `src/project_semantic_dispatch/relation.rs` | always |
| The standing checkpoint of `relate_pair_alternatives_with_freshness` (the one undecided alternative of a union target keeps its inference deposits) | Inference of a source signature's parameters through an optional callback union whose function arm holds an outer `infer` or call parameter | REQUIRED | One alternatives step | `src/project_semantic_dispatch/relation.rs` | always |
| The re-discharge re-entry stop in `relation_discharge_and_route` (a component closing cyclic inside a re-discharge is released without publish) | Termination of a self-returning method inference | REQUIRED | Stateless | `src/project_semantic_dispatch/relation.rs` | always |
| `signature_inference_inputs` keeping an undecided pair's deposits (`inferTypes` never fails; only a budget stop abandons) | `instantiate_signature_in_context_of` when the target's positions hold an outer `infer` or call parameter | REQUIRED | One inference session | `src/project_semantic_dispatch/signature_instantiation.rs` | always |
| `contextual_signature` over a union (arms without a call signature contribute none) | Contextual typing of a callback argument at an optional `((value: T) => R) \| null \| undefined` parameter | REQUIRED | Stateless | `src/project_semantic_dispatch/flow_return_contextual.rs` | always |
| `global_wrapper_surface` instantiating in carrier-preserving `Navigate` transit | A runtime nominal's or primitive's member read keeps the interface's references to itself (`then(): Promise<…>`) as references | REQUIRED | Stateless | `src/project_semantic_dispatch/apparent_type.rs` | always |

No production counter, trace or charge history is added, so nothing is gated behind `semantic-observe`.
