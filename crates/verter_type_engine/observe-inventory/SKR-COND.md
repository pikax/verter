# Conditional decision inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ConditionalBranchSelection` and the `ConditionalFact` / `ConditionalReading` a conditional read establishes (complete reduced, complete deferred, or unavailable with its partial reasons) | The conditional query's branch commit, a relation over a deferred conditional, and every consumer that must tell a complete symbolic conditional from an unavailable reduction | REQUIRED | One conditional read | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `SurfaceMember::method_kind` of an interface or type-literal accessor (`Get` / `Set`) | The relation and inference read an accessor pair as one property of its getter's type | REQUIRED (semantic identity) | The interned surface node | `src/semantic_query.rs` (lowered by `verter_type_expr_oxc::interface_method_kind`) | always |
| The weak-type check on the record-shaped object route (`weak_surfaces_reject`) | The relation rejects a weak target sharing no property with the source | REQUIRED | One relation step | `src/project_semantic_dispatch/relation.rs` | always |
| The written-arm true proof (`true_through_written_arm`: one arm's complete relation read, adopted; the unused normalized operands' reuse evidence preserved) | The conditional decision takes the true branch an unreadable `extends` arm cannot take away | REQUIRED | One conditional decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| The indexed read mode of a structural-transit demand (`indexed_read_mode`) and the relation's `Navigate` settlement of an indexed operand | The relation compares an indexed member's type, never its one-level publication surface | REQUIRED | One deferred evaluation / relation step | `src/project_semantic_dispatch/evaluate.rs`, `src/project_semantic_dispatch/relation.rs` | always |

## verter_session / verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Resolved-file augmentation index entries now listing package-specifier augmenters (`specifier_augments_resolved_module`), and their resolver-free invalidation | The declaration stitch merges every `declare module` block resolving to the augmented file | REQUIRED (cache validity) | The `FileArtifactStore` augmentation index entry | `verter_session_query/src/source/augmentation.rs`, `verter_session/src/file_artifact_store.rs`, `verter_session/src/host_manage/source_augmentation.rs` | always |

No optional counter, trace or decision history is added: the decision state above is required, and nothing is gated behind `semantic-observe`.
