# Relation recovery inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RelateWork::AllOf` (the items every one of which must hold, and how many have run) | The relation worklist relates a union source's arms, an intersection target's arms and a distribution's pairs in order and ends the sequence false at the first item decidedly not related (`eachTypeRelatedToType`), outside an inference session | REQUIRED | One relation worklist | `src/project_semantic_dispatch/relation.rs` | always |
| `ShallowDiagnostic::RelationTooComplex` (the refused check's source and target) | A relation check refused at its structured-comparison allowance names its two subjects (TS2859) on the read that refused it; `CacheRead::walker_diagnostics` carries it to every consumer, the sealed refusal summary replays it, and component metadata projects it as a budget stop | REQUIRED | One read; a sealed refusal summary's read | `src/project_semantic_dispatch/walk.rs`, `relation.rs` | always |
| `BuildLocalTaint::operation_refusals` | Every cold build reports the operation refusals raised or read under it with its own diagnostics, so a refusal reaches each read composed over the refused operation, cold or replayed from a sealed refusal | REQUIRED | One cold-build-local taint frame | `src/project_semantic_dispatch/mod.rs` | always |

No optional counter, trace or attribution history is added: the logical comparison count, its receipts and the refusal's subjects are required state, so nothing is gated behind `semantic-observe`.
