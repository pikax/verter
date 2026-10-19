# Call-argument contextual-evaluation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CallArgumentLiteralPolicy::context_keeps` (the literals a bare literal argument nests at authored positions whose combined contextual type in the candidate's parameter is a literal context for them, decided per authored member, element or conditional arm by `checked_literal_argument`, each paired as a `ContextKeptLiteral` with a candidate type parameter that position's contextual type names) | `relation_deposit` keeps a literal as written at a nested deposit into a parameter it is paired with and widens every other literal; computed per candidate and per argument relation, so a rejected candidate's reading never reaches the next | REQUIRED | One call-argument relation | `src/project_semantic_dispatch/dispatch_txn.rs` | always |
| The checked argument type `checked_literal_argument` returns (the bare literal argument with each authored position known to have no literal context for its literals holding their widened type) | `call_argument_relation` relates and deposits from it, so a position's decision stays with that position: an equal literal at another position whose contextual type is no literal context arrives widened | REQUIRED | One call-argument relation | `src/project_semantic_dispatch/call_resolve_context.rs` | always |
| Function-return occurrence priority (`inference_occurrence_for_position` keeps the enclosing occurrence's rung through a function return) | Fixation weighs a callback's or a receiver member's return beside the sibling arguments, as `inferFromSignature` does | REQUIRED | One relation position | `src/project_semantic_dispatch/relation.rs` | always |
| First-pass object-literal frame's contextual type (`ObjectEvalFrame.contextual` for `eval_object_literal_first_pass`) | A context-sensitive object literal's other members are evaluated under the parameter's contextual type, so a literal-context member keeps its literal before the first inference pass deposits it | REQUIRED | One first-pass evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |
| `CallArgKey::Eager::literal_provenance` (`ArgumentLiteralProvenance`: the argument's authored nested positions a regular contributor writes — an asserted value, a const-asserted literal or composite, a read of a binding keeping no widening literal — each with the regular literals there, resolved in the argument's scope by `resolve_argument_literal_provenance`) | `checked_literal_argument` widens only a position's fresh literals and keeps every regular one as written for each parameter the argument reaches; part of the call's key identity, as `literal_mode` is | REQUIRED | One call-resolution key | `src/semantic_query.rs`, `src/project_semantic_dispatch/call_resolve_context.rs` | always |
| `CallArgumentLiteralPolicy::first_pass` (the relation's argument is a context-sensitive literal's first-pass reading) | `relation_deposit` withholds that reading's whole deposit into a top-level type parameter of the target (`T`, the `T` arm of `T \| undefined`), keeping every nested and other-arm deposit | REQUIRED | One call-argument relation | `src/project_semantic_dispatch/dispatch_txn.rs` | always |

## verter_type_expr

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `IndexedValueCallArg::literal_provenance` (`IndexedLiteralProvenance`: per authored nested position of the argument, its regular contributors — `Pinned` type, `PinnedComposite`, `Read` — and whether a fresh contributor writes it too) | The flow call route resolves it into the call key's `ArgumentLiteralProvenance` | REQUIRED | The indexed program expression record | `src/indexed_expression.rs` | always |

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `indexed_literal_provenance` (the producer of each indexed call argument's literal provenance, read off the argument's syntax) | `IndexedValueCallArg::literal_provenance` | REQUIRED | One argument lowering | `src/analysis/type_eval_build.rs` | always |
| Shallow inference `Value::Fresh` (a produced type's fresh top-level literals) and `InferenceState::widen_fresh_positions` (a `let` / `var` initializer widens every position it closes) | Declaration widening widens only fresh literals, and a conditional's fresh literal its regular counterpart absorbs stays regular | REQUIRED | One value inference | `src/analysis/type_eval_build_shallow.rs`, `src/analysis/type_eval_build.rs` | always |

No production counter, trace or charge history is added, so nothing is gated behind `semantic-observe`.
