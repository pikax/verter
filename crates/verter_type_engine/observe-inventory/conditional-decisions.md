# Conditional decision inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ConditionalBranchSelection` and the `ConditionalFact` / `ConditionalReading` a conditional read establishes (complete reduced, complete deferred, or unavailable with its partial reasons) | The conditional query's branch commit, a relation over a deferred conditional, and every consumer that must tell a complete symbolic conditional from an unavailable reduction | REQUIRED | One conditional read | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| `SurfaceMember::method_kind` of an interface or type-literal accessor (`Get` / `Set`) | Relation and inference read an accessor pair as one property of its getter's type | REQUIRED | The interned surface node | `src/semantic_query.rs` | always |
| The weak-type check on the record-shaped object route (`weak_surfaces_reject`) | The relation rejects a weak target sharing no property with the source | REQUIRED | One relation step | `src/project_semantic_dispatch/relation.rs` | always |
| The written-arm true proof (`true_through_written_arm`: the check's complete, positive plain-type read (neither absorbs nor distributes) and one arm's complete relation read, both adopted; the unused normalized operands' reuse evidence preserved) | The conditional decision takes the true branch an unreadable `extends` arm cannot take away | REQUIRED | One conditional decision | `src/project_semantic_dispatch/conditional_decision.rs` | always |
| The indexed read mode of a structural-transit demand (`indexed_read_mode`) and the relation's `Navigate` settlement of an indexed operand | The relation compares an indexed member's type, never its one-level publication surface | REQUIRED | One deferred evaluation / relation step | `src/project_semantic_dispatch/evaluate.rs`, `src/project_semantic_dispatch/relation.rs` | always |
| Wildcard indexed-access substitution and rigid indexed-access constraint reads | Permissive/restrictive conditional tests preserve generic dependence and distinguish complete deferred answers from unavailable reductions | REQUIRED | One substitution / relation step | `src/project_semantic_dispatch/substitute.rs`, `src/project_semantic_dispatch/relation.rs` | always |
| Frame-local `ColdComputeCompletenessScope` and the step-completeness taint fold before discarding that scope | Frame completion retains partial reasons on its own output; consumers inherit them through delivery without tainting the native caller that steps the frame | REQUIRED | One continuation frame step; folded taint lasts until frame completion | `src/project_semantic_dispatch/query_frames.rs` | always |

## verter_type_expr_oxc

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Type-literal `MethodSignature::method_kind`, produced by `interface_method_kind` | The shared body builder preserves getter/setter identity for relation and inference | REQUIRED | The transient lowered signature | `src/lib.rs` | always |

## verter_semantic

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Interface `MethodSignature::method_kind` and shallow `MemberHeader::method_kind` | Lazy member selection and the shared body builder preserve authored accessor identity | REQUIRED | The transient lowered signature or indexed header | `src/analysis/type_eval_build.rs`, `src/analysis/decl_headers.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Resolved-file augmentation index entries now listing package-specifier augmenters, and their resolver-free invalidation | The declaration stitch merges relative blocks and module-file package augmentations resolving to the augmented file | REQUIRED | The `FileArtifactStore` augmentation index entry | `src/file_artifact_store.rs`, `src/host_manage/source_augmentation.rs` | always |
| `FactEmissionView::module_kind` producing `ModuleAugmentationFact::declaring_file` | The augmentation matcher distinguishes package augmentations in module files from ambient declarations in scripts | REQUIRED | Shallow fact emission; the produced classification lasts with the indexed artifact | `src/fact_emission.rs` | always |

## verter_session_query

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ModuleAugmentationFact::declaring_file` | Resolved-file target matching accepts package augmentation only from module files, preserving script ambient declarations | REQUIRED | The indexed augmentation fact's content version | `src/source/augmentation.rs` | always |
| Resolved-file augmentation target matching (`fact_augments_resolved_module`) for relative and module-file package specifiers | The session discovers every contributor to the resolved target through its existing resolver; script package declarations remain ambient | REQUIRED | One augmentation fact match | `src/source/augmentation.rs`, `src/resolution/augmentation_key.rs` | always |

No optional counter, trace or decision history is added: the decision state above is required, and nothing is gated behind `semantic-observe`.
