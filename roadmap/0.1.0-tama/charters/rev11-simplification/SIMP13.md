<!-- unified-charter-v2
id=SIMP13
name=Simplify surviving semantic memo coordination
predecessors=SIMP5,G4
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:existing query runtime and typed memo families
conflict_domains=semantic_cache_store,semantic_authority
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=concurrency-3
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
charter=charters/rev11-simplification/SIMP13.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP13 — Simplify surviving semantic memo coordination

## Independently acceptable outcome and owner

Any surviving equivalent memo coordination uses the existing runtime while typed family preconditions and publication semantics remain explicit.

Current problem: Memo families repeat claim, abort, completion and publication coordination. Current migrations may remove some copies, so cleanup must inspect the final owners instead of restoring old abstractions.

Final owner: **existing query runtime and typed memo families**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP5:** Live types, capabilities and behavior protect semantic boundaries while residual source analyzers and their self-tests disappear.
- **G4:** converged query/cache production and bounded fact-validated retention.

## Concrete surfaces and implementation

- `crates/verter_session/src/semantic_query_memo/relation_memo.rs`
- `crates/verter_session/src/semantic_query_memo/flow_return_memo.rs`
- `crates/verter_session/src/semantic_query_memo/resolve_call_memo.rs`
- `crates/verter_session/src/project_semantic_dispatch/carrier.rs`

Reinspect final D3/G1/G2/G4 state. Use existing FlightCell/shared SCC publication for surviving duplicate orchestration, preserving typed family inputs, fact/taint scopes and complete-only admission. Remove redundant intermediate state and wrappers; simplify repeated sibling test setup and comments where their contracts match.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No redo of D3C, E2, G2 or G4 authority. If those predecessors already removed the population, verify that outcome without manufacturing work.

## Acceptance and proportionate proof

- **SIMP13-AC1:** Surviving equivalent memo coordination uses one existing runtime owner.
- **SIMP13-AC2:** Family semantics, cancellation and complete-only publication remain correct.
- **SIMP13-AC3:** Duplicate coordination/state disappears without another cache or memo framework.

Existing evidence to retain or extend: Same-key production, cancellation, reentrancy/cycles, SCC publication, partial/complete results, fact invalidation and retention tests; compare equivalent work/allocations.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused memo/query tests, node scripts/gate.mjs, applicable G4 retention/work proof and live compile contracts.

Apply concurrency-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
