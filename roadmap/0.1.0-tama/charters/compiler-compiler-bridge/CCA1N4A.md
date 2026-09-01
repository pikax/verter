<!-- unified-charter-v2
id=CCA1N4A
name=Truthful runtime-render route-record verification note
phase=compiler
train=compiler.compiler-bridge
product=compiler_bridge
kind=repair
semantic_role=delivery
class=compiler
predecessors=CCA1N4
owner=compiler.compiler-bridge:accurate verification statement on the runtime-render route record
conflict_domains=compiler_execution
resource_class=rust-mixed
review_profile=public-3
gate_profile=targeted-domain
implementation_effort_min=low
implementation_effort_default=low
review_effort_min=medium
review_effort_default=medium
verification_effort_min=low
verification_effort_default=low
confirmation_effort_min=low
confirmation_effort_default=low
size=S
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/compiler-compiler-bridge/CCA1N4A.md
max_production_loc=5
max_production_files=1
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# CCA1N4A — Truthful runtime-render route-record verification note

## Independently acceptable outcome and owners

The committed runtime-render route record states only the verification its consumers actually perform. Today it asserts that its route string is equality-checked against structured observations by two named behavioral suites. No test reads that record: its single consumer loads it through `include_str!` and its only route assertion is non-emptiness, and neither named suite opens the file. The record therefore overstates its own verification, and a reader who trusts it would believe a drifting route string is caught by a check that does not exist. After this node the record's verification sentence describes the coverage that is really there. Current ownership is the false equality-check sentence; final ownership is the truthful derived-prose statement the sibling host-backed record already uses. Reverting restores only that sentence.

## Exact production population and boundary

- Production surface: `crates/verter_session/src/framework/framework_product_surface_inventory.json`, the `compile.batch.runtime-render` case's `routeEvidence` closing sentence only.
- The replacement adopts the formulation already carried by the sibling `compile.batch` record: the behavioral suites assert the structured observations the prose describes, no test compares the prose byte-wise, and a route that drifts is caught by those suites failing on the described behavior rather than by a string diff. The two rows read consistently.
- The structured-fact enumeration preceding that sentence is accurate and is not rewritten. Route strings, product lists, profile axes, aliases, and every other case are untouched.
- Excluded: adding a new test, changing the inventory's consuming test, altering any executed route, and extending route evidence with observations the lane does not record.

## Exact predecessor contract

- **CCA1N4:** the runtime-render lane already executes through bound framework host backends and owns this route record; this node corrects only the verification claim that landing left behind.

## Invariants and acceptance

- The record states only what is actually proven: no sentence claims a comparison, equality check, or byte-wise assertion that no test performs.
- Every named suite the record cites is a suite that really covers the cited behavior; a suite is not credited with reading a file it never opens.
- The structured observations, executed route, products, axes, and aliases are unchanged, and the inventory remains parseable by its one consumer.
- No test, executed route, or public behavior changes.

## Deletions, budget, and verification

Delete only the false equality-check sentence. Ceiling: 5 production LOC, 1 production file, 1 crate; abort if a second surface, a test change, or a route change enters. Run the product-surface inventory suite and `targeted-domain`. The native host-integration convergence join consumes the corrected record.
