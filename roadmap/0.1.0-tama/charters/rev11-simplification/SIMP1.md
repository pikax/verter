<!-- unified-charter-v2
id=SIMP1
name=Retire scheduler source-shape guards
predecessors=A6
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:scheduler behavioral and ownership contracts
conflict_domains=scheduler_admission,test_maintenance
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=simple-1
implementation_effort_min=low
implementation_effort_default=medium
review_effort_min=low
review_effort_default=low
verification_effort_min=low
verification_effort_default=medium
confirmation_effort_min=low
confirmation_effort_default=low
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP1.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP1 — Retire scheduler source-shape guards

## Independently acceptable outcome and owner

Scheduler behavior stays covered while obsolete source-shape guards and their exclusive machinery are removed.

Current problem: Scheduler tests scan for deleted names, exact signatures and enum spellings instead of exercising scheduling. The known-variant counter even misses an additional enum variant.

Final owner: **scheduler behavioral and ownership contracts**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **A6:** ratified current architecture and proportionate evidence baseline, already implemented.

## Concrete surfaces and implementation

- `crates/verter_scheduler/tests/cases/dag_arch_guards.rs`
- `crates/verter_scheduler/tests/cases/mod.rs`
- `crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`

Delete dag_arch_guards.rs, module wiring and only its registry references. Keep other registry entries until SIMP3. Retain capacity, cancellation, dependency, fairness and CacheNode dispatch tests. Before deleting the no-linear-scan fingerprint check, map actual bounded-selection work evidence or add the smallest meaningful check if that contract is uncovered. Remove source walkers and brace extraction with their last consumer.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No production scheduler changes or active D3 guard/charter changes.

## Acceptance and proportionate proof

- **SIMP1-AC1:** Obsolete scheduler source-name guards and exclusive helpers are removed.
- **SIMP1-AC2:** Capacity, cancellation, priority and dispatch behavior remain tested.
- **SIMP1-AC3:** Bounded selection is supported by real work evidence instead of spelling bans.

Existing evidence to retain or extend: dag_tests.rs capacity/drop/release and dependency tests; dag_lanes_tests.rs fairness and drain/cancel; scheduler.rs::cache_node_dispatch_routes_to_execute_cache_node. A live ownership contract uses compiler evidence, not signature text.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused scheduler suites with nonzero selection, then node scripts/gate.mjs; applicable compile contracts if changed.

Apply simple-1 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
