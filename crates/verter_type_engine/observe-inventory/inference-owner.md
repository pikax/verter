# Inference-owner inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `InferenceSession` (per-parameter candidate journals, reverse-projection journals, fresh-literal deposits, staged bindings, lifecycle state) — moved, unchanged | Fixation reads the winning candidates; a losing alternative rolls them back through `SessionCheckpoint`; publication reads the staged bindings | REQUIRED | One collecting session on the transaction's session stack, retired at commit or abandonment | `src/project_semantic_dispatch/inference/session.rs` | always |
| `InferenceSessionSetup` / `InferenceInfoSetup` — moved, unchanged | The relation key's inference context and the session opened for it are built from the same value | REQUIRED | The relation key / the session | `src/project_semantic_dispatch/inference/session.rs` | always |
| Binding barrier around `execute_relate_pair` / `execute_relate_pair_kind` (the existing `binding_disabled_session_barriers` stack, now pushed by every pure pair question) | Hides every session open before a pure assignability question, so it deposits nothing; a relation whose key carries an inference context opens its own session above the barrier | REQUIRED | One pair question | `src/project_semantic_dispatch/relation.rs` | always |
| `CallArgumentLiteralPolicy::argument` | `relation_deposit` keeps the argument's own literal as written when it is deposited whole, wherever in the target it lands | REQUIRED | One call-argument relation | `src/project_semantic_dispatch/dispatch_txn.rs` | always |
| `ClauseBaseSubstitution` (a clause's base constraints) | `base_signature` derives it once per signature and applies it to every parameter and the return | REQUIRED | One base-signature or clause instantiation | `src/project_semantic_dispatch/build.rs` | always |
| `BINDER_COLLECTION_VISITS` (thread-local count of the nodes the per-name binder collection visited) | none — read by the base-signature work-growth test | OPTIONAL (test measurement) | The test thread | `src/project_semantic_dispatch/build.rs` | `cfg(any(test, feature = "test-support"))`; absent from default and `semantic-observe` builds |

No production counter, trace or charge history is added, so nothing is gated behind `semantic-observe`.
