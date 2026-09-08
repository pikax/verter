<!-- unified-charter-v2
id=SIMP5
name=Retire semantic migration analyzers
predecessors=SIMP4,E2
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:canonical semantic types and capabilities
conflict_domains=semantic_authority,semantic_cache_store,test_maintenance
resource_class=rust-mixed
gate_profile=typeinfo-domain
review_profile=architecture-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP5.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP5 — Retire semantic migration analyzers

## Independently acceptable outcome and owner

Live types, capabilities and behavior protect semantic boundaries while residual source analyzers and their self-tests disappear.

Current problem: Residual tests implement a partial Rust analyzer with fixed anchors, taint propagation and call allowlists, preserving migration maps after canonical representations should own the invariant.

Final owner: **canonical semantic types and capabilities**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP4:** Historical-only guards and exclusive support are deleted while live schema, portability and cache-behavior evidence remain.
- **E2:** completed internal general TypeExpr transit elimination with permanent authored ingress and terminal egress preserved.

## Concrete surfaces and implementation

- `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs`
- `crates/verter_source_policy_gate/tests/cases/residual_type_expr_body_reader_inventory.rs`
- `crates/verter_source_policy_gate/tests/cases/whole_env_consumer_graph_native_inventory.rs`
- `crates/verter_source_policy_gate/tests/cases/handle_capable_consumer_guards.rs`
- `crates/verter_source_policy_gate/tests/cases/source_corpus.rs`
- `crates/verter_source_policy_gate/tests/cases/mod.rs`

After E2's actual transit cutover, retire HOT_* scanner lists, taint/call modeling, evasion tests and closed anchor inventories. First preserve/extract hot_structural_rail_not_impl_asserts and semantic_api_wire_input_witness and retain NoTypeExpr/OutputProjector compile fixtures. Remove shared source corpus and aggregate machinery only after remaining useful policy consumers are resolved. Preserve permanent authored ingress and terminal DTO output.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No semantic redesign, public DTO migration, new taint analyzer or production transit cutover; any missing production obligation returns to E2 or its owner.

## Acceptance and proportionate proof

- **SIMP5-AC1:** The residual semantic analyzer and closed migration inventories are retired.
- **SIMP5-AC2:** Live compiler/type/capability constraints survive outside deleted scanner files.
- **SIMP5-AC3:** Semantic output, complete admission and bounded materialization remain evidenced.

Existing evidence to retain or extend: Actual compile fixtures through scripts/compile-contracts.mjs, not the obsolete hot_materialize_structural_rails_smoke selector; semantic output/completeness/materialization-work evidence.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

typeinfo-domain final profile with fresh native/TS builds, node scripts/gate.mjs and node scripts/compile-contracts.mjs.

Apply architecture-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
