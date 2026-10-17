# Call-argument contextual-evaluation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `InferenceInfoSetup::literal_context` / `InferenceInfo::literal_context` (`LiteralContext`: the literal kinds a candidate type parameter's constraint makes a literal context for) | `relation_deposit` keeps a literal a call argument nests at that parameter's position as written when its kind is admitted, and widens it otherwise; set from the candidate's own clause by `call_inference_infos`, so a rejected candidate's context never reaches the next | REQUIRED | The candidate's collecting session | `src/project_semantic_dispatch/inference/session.rs` | always |
| Call-argument return-position priority (`relation_deposit` reads a call argument's `ReturnType` occurrence at the `Argument` rung) | Fixation weighs a callback's return beside its sibling arguments, as `inferFromSignature` does | REQUIRED | One deposit | `src/project_semantic_dispatch/inference/collect.rs` | always |
| First-pass object-literal frame's contextual type (`ObjectEvalFrame.contextual` for `eval_object_literal_first_pass`) | A context-sensitive object literal's other members are evaluated under the parameter's contextual type, so a literal-context member keeps its literal before the first inference pass deposits it | REQUIRED | One first-pass evaluation | `src/project_semantic_dispatch/flow_return.rs` | always |

No production counter, trace or charge history is added, so nothing is gated behind `semantic-observe`.
