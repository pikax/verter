# Current and successor ownership seams

Current implementations review these known successor boundaries before fixing a public shape. This is a read-only compatibility check against approved charters, not permission to implement a successor early. A disagreement requires an ordinary reviewed amendment to the owning contract.

| Current owner | Successor owner | Boundary to reconcile in the current review |
| --- | --- | --- |
| K1 host-session identity | VID0 and CAT0 | Separate content, environment and catalog identity; preserve an explicit conversion boundary for successor-qualified identities. |
| E3 public type information | TIF0, TIF1 and PUB0 | Preserve graph provenance, completeness, unsupported outcomes and lifetime ownership through public projections; avoid a second evaluator or identity authority. |
| B2/B4 and the compiler bridge | PAR0, CPF0 and CPF1 | Preserve immutable parse/artifact ownership and framework qualification; successor facets cannot create a second parser or compiler authority. Historical charters remain frozen. |
| C1 project state and IDX0 candidates | PM0–PM4 | Distinguish configured-project membership and coherent resolution proofs from candidate indexes; PM owns project facts and consumer cutover. |
| MEM0, E4, MEM1 and G4 | PER0 | Carry exact charge ownership, aggregate retention limits, request admission and externally pinned-result accounting into later performance contracts. |

K1, E3 and UAK0 packets include this reconciliation explicitly. The reviewer names the actual current API/type and its successor boundary from those charters, records compatibility or the necessary amendment, and does not weaken current acceptance because a successor exists. Implemented charters are historical records, so later reviews consult this active contract without rewriting them.

`catalogs/contract-dependencies.toml` records selected cross-train producer/consumer obligations. Strict validation proves each producer is a transitive ancestor of each declared consumer. This is a static dependency proof, not evidence that a runtime consumer has migrated, and the inventory is not inferred exhaustively from source. Amend it whenever a reviewed change introduces or removes a cross-train contract consumer.
