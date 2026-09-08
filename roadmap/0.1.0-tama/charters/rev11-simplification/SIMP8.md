<!-- unified-charter-v2
id=SIMP8
name=Simplify provider transport bookkeeping
predecessors=SIMP6,SIMP7
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:transport-local pending-request ownership
conflict_domains=provider_lifecycle
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
charter=charters/rev11-simplification/SIMP8.md
size=L
max_production_loc=900
max_production_files=8
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP8 — Simplify provider transport bookkeeping

## Independently acceptable outcome and owner

Shared request mechanics have one small owner and protocol-specific responsibilities are narrower without changing cancellation, completion or failure behavior.

Current problem: Large IPC modules repeat pending-request insertion/removal/completion logic while mixing process setup, framing, document state and feature decoding.

Final owner: **transport-local pending-request ownership**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP6:** Provider safety is verified through actual admission, protocol traffic, replay and close behavior without source-shape backstops.
- **SIMP7:** Coordinate conversion reuses state for the exact source snapshot or batch with fewer full-source scans and copies and unchanged encoding/range behavior.

## Concrete surfaces and implementation

- `crates/verter_type_runtime/src/tsgo/ipc.rs`
- `crates/verter_type_runtime/src/tsserver/ipc.rs`
- `crates/verter_type_runtime/src/lib.rs`

Compare PendingRequests insert/take/pending_since, cancellation, completion and shutdown. Consolidate identical mechanics under the existing runtime, keeping protocol framing and cancellation adapters explicit. Extract coherent private responsibilities with narrow inputs, preserve SIMP7's snapshot conversions and consolidate duplicate local tests/prose.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No ProviderHub implementation, process acquisition, protocol unification or new broad context/service locator; H2 retains the lifecycle cutover.

## Acceptance and proportionate proof

- **SIMP8-AC1:** Equivalent pending-request bookkeeping has one implementation.
- **SIMP8-AC2:** IPC responsibilities narrow without a generic transport framework.
- **SIMP8-AC3:** Completion, cancellation, shutdown and request retention remain correct and bounded.

Existing evidence to retain or extend: Existing out-of-order response, cancellation/preemption, failure, shutdown and restart fixtures; no double completion or leaked request; non-owning transports remain non-owning.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused type-runtime lifecycle/transport suites and node scripts/gate.mjs; existing real-provider smoke when framing/launch changes warrant it.

Apply concurrency-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
