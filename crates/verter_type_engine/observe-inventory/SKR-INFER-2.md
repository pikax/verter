# Call-argument contextual-evaluation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CallArgumentLiteralPolicy::context_keeps` (the literals a bare literal argument nests at authored positions whose contextual type in the candidate's parameter is a literal context for them, decided per authored member or element by `nested_literal_context_keeps`) | `relation_deposit` keeps those literals as written at a nested deposit and widens every other literal; computed per candidate and per argument relation, so a rejected candidate's reading never reaches the next | REQUIRED | One call-argument relation | `src/project_semantic_dispatch/dispatch_txn.rs` | always |
| Function-return occurrence priority (`inference_occurrence_for_position` keeps the enclosing occurrence's rung through a function return) | Fixation weighs a callback's or a receiver member's return beside the sibling arguments, as `inferFromSignature` does | REQUIRED | One relation position | `src/project_semantic_dispatch/relation.rs` | always |
| First-pass object-literal frame's contextual type (`ObjectEvalFrame.contextual` for `eval_object_literal_first_pass`) | A context-sensitive object literal's other members are evaluated under the parameter's contextual type, so a literal-context member keeps its literal before the first inference pass deposits it | REQUIRED | One first-pass evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |

No production counter, trace or charge history is added, so nothing is gated behind `semantic-observe`.
