<!-- unified-charter-v2
id=SIMP14
name=Reduce semantic dispatcher responsibilities
predecessors=SIMP13
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:existing semantic dispatch and prepared declarations
conflict_domains=semantic_authority,host_service_graph
resource_class=rust-mixed
gate_profile=targeted-domain
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
charter=charters/rev11-simplification/SIMP14.md
size=L
max_production_loc=1200
max_production_files=10
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP14 — Reduce semantic dispatcher responsibilities

## Independently acceptable outcome and owner

Dispatcher responsibilities have narrower explicit inputs with redundant helpers/state forwarding removed and rationale held by its owner.

Current problem: Large dispatch files mix declaration preparation, traversal and terminal projection with broad state access and duplicated helpers. Arbitrary file splitting would preserve that coupling.

Final owner: **existing semantic dispatch and prepared declarations**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP13:** Any surviving equivalent memo coordination uses the existing runtime while typed family preconditions and publication semantics remain explicit.

## Concrete surfaces and implementation

- `crates/verter_session/src/project_semantic_dispatch/build.rs`
- `crates/verter_session/src/project_semantic_dispatch/raise.rs`
- `crates/verter_session/src/project_semantic_dispatch/walk.rs`
- `crates/verter_session/src/project_semantic_dispatch/tests.rs`

Bind surviving declaration preparation, node traversal and terminal projection after semantic/cache convergence. Separate coherent responsibilities, narrow context/state access and delete repeated conversions/helpers already owned elsewhere. Preserve authored lowering, query evaluation and public output boundaries. Consolidate local repeated fixture setup and historical comments with the implementation that made them redundant; K3 retains host/service construction.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No second resolver, replacement catch-all context, wire change, eager traversal, K3 host authority or file/LOC quota.

## Acceptance and proportionate proof

- **SIMP14-AC1:** Dispatcher responsibilities use narrower inputs and fewer independent mechanisms.
- **SIMP14-AC2:** Duplicate helpers/state forwarding and repeated rationale are removed.
- **SIMP14-AC3:** Public results, lazy work, invalidation and cancellation remain equivalent.

Existing evidence to retain or extend: Existing public resolution/typeinfo/metadata, lazy/shallow work, incremental=fresh, cancellation and exact identity/mapping cases. Show fewer independent mechanisms/state dependencies; file moves alone do not count.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

node scripts/gate.mjs; relevant public metadata/typeinfo suites with fresh prerequisites; existing operation/allocation/retention evidence for touched hot paths.

Apply architecture-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
