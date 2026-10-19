# Intersection distribution inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `admit_cross_product` (the one admission of an intersection's cross product: weighed from the factors' widths, then paid for whole before its first constituent is built) | The canonical construction (`intersect_over_unions`) and a member read's or narrowing's distribution (`ProjectSemanticDispatch::intersection_parts`) build a product only once it is admitted; a refused product is its TS2590 recovery, an unpaid one stands as the written intersection | REQUIRED-budget | One construction | `src/project_semantic_dispatch/canonical_algebra.rs` | always |
| `ConstructionMeter` (the ledger a canonical construction pays: the connected-demand ledger's work units and construction bytes, or none outside a demand) | Every dispatch-owned canonical union/intersection construction pays its distributions' products through `ProjectSemanticDispatch::construction_meter`; the charges accrue to the open cost scope, so a sealed refusal or a warm receipt replays them | REQUIRED-budget | One construction call; the charges live on the connected demand's ledger and its cost receipts | `src/project_semantic_dispatch/canonical_algebra.rs`, `src/project_semantic_dispatch/mod.rs` | always |
| `CanonicalEvidence::exhausted` (the rails a distribution could not pay for) | `ProjectSemanticDispatch::deposit_canonical_evidence` and the composite-reduction builders fold it as the result's partial reasons, and it marks the written composite `CanonicalUnproven` | REQUIRED | One canonicalization's evidence, until deposited | `src/project_semantic_dispatch/canonical_algebra.rs` | always |
| `presimplify_factors` (the checker's strategies ahead of a cross product, on a distribution held as factors) | `ProjectSemanticDispatch::distribute_intersection` weighs and builds the factors it returns, with any nullish member added back | REQUIRED | One distribution | `src/project_semantic_dispatch/canonical_algebra.rs` | always |
| `ProjectSemanticDispatch::intersection_parts` per-combination work charge (replaced) | The combinations are paid for at once through `admit_cross_product` before the first is formed; the per-combination charge is gone | REQUIRED-budget | One distribution | `src/project_semantic_dispatch/flow_return.rs` | always |

No optional counter, trace or allocation attribution is added: the work
units and construction bytes a product is charged are required budget state,
and the allocation canaries count through the integration binary's own
counting allocator rather than any production hook, so nothing is gated
behind `semantic-observe`.
