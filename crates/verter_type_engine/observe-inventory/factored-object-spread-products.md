# Factored object-spread product inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ObjectProjectionFormula.product` (`SpreadProduct`: root node, selector, exact-optional policy, closed flag) — replaces the materialised alternative slice | Every `ProjectObjectSpread` consumer reads alternatives from it through the dispatch cursor; the memo retains and charges it as the value's one owned allocation | REQUIRED | The formula (memo candidate or delivered read) | `src/semantic_query/object_spread_projection.rs`, `src/project_semantic_dispatch/object_spread_product.rs` | always |
| `ProductNode` / `ProductArms` / `ProductLeaf` / `NestedProduct` (the Leaf/Choice/Overlay DAG) | The cursor folds the leaves on one alternative's path; `narrowed` collapses arms a selector cannot tell apart | REQUIRED | The owning product | `src/project_semantic_dispatch/object_spread_product.rs` | always |
| `ProductArms.marks` (choice ends / overlay strides) | Index decoding of an alternative without enumerating its predecessors | REQUIRED | The owning node | `src/project_semantic_dispatch/object_spread_product.rs` | always |
| `ProductBounds` (`width`, `units`, `facts`) | `width` feeds the checker's cross-product limit (TS2590) and the cursor's end; `units`/`facts` are the prospective work and construction-byte footprint charged before each alternative is built | REQUIRED-budget | The owning node | `src/project_semantic_dispatch/object_spread_product.rs` | always |
| `ProductNode::Finished` (a one-alternative product folded at build) | Warm reads of a single-alternative formula reuse the folded alternative instead of refolding | REQUIRED | The owning product | `src/project_semantic_dispatch/object_spread_product.rs` | always |
| `ProductCursor` (`next`, `width`) | Production consumers (path walk, relation, excess prepass, keyof, raise fold, macro surfaces, one-level surface) read alternatives on demand; a connected-demand trip ends it with `Err` | REQUIRED | One read loop | `src/project_semantic_dispatch/object_spread_product.rs` | always |

No optional counter, trace or attribution history is added. The pre-existing test work counters `note_member_visit` / `note_collision_comparison` are unchanged and remain the observation consolidation's to reclassify. The retired `DISTRIBUTION_CAP` / `SPREAD_DEPTH_CAP` constants and the eager per-state clone list are deleted.
