<!-- unified-charter-v2
id=SIMP15
name=Verify simplification outcomes and coverage
predecessors=SIMP2,SIMP4,SIMP8,SIMP9,SIMP10,SIMP11,SIMP12,SIMP14
phase=rev11
train=rev11.simplification
product=rev11
kind=convergence
semantic_role=convergence
class=foundational
owner=rev11.simplification:simplification outcome verification
conflict_domains=performance_evidence,test_maintenance
resource_class=rust-mixed
gate_profile=canonical
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
charter=charters/rev11-simplification/SIMP15.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP15 — Verify simplification outcomes and coverage

## Independently acceptable outcome and owner

Completed simplification has explicit removed mechanisms, retained proof and separate truthful test/build and application-work measurements.

Current problem: Cleanup can reduce line/test counts while retaining complexity, dropping coverage or claiming unmeasured performance gains.

Final owner: **simplification outcome verification**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP2:** Browser capabilities and legacy URL restoration are tested directly without engine-name or source-layout tombstones.
- **SIMP4:** Historical-only guards and exclusive support are deleted while live schema, portability and cache-behavior evidence remain.
- **SIMP8:** Shared request mechanics have one small owner and protocol-specific responsibilities are narrower without changing cancellation, completion or failure behavior.
- **SIMP9:** One existing lifecycle owner closes stale provider surfaces with correct tokens, failure recovery and reopen ordering.
- **SIMP10:** Minimal property key/value/order information survives until final emission, removing reparsing and narrowing property assembly state access.
- **SIMP11:** One existing metadata-based check serves local and CI verification without duplicate Rust logic or hidden test-target discovery.
- **SIMP12:** Tests assert actual public output, types and built exports with less duplicate setup and no implementation-name scanner.
- **SIMP14:** Dispatcher responsibilities have narrower explicit inputs with redundant helpers/state forwarding removed and rationale held by its owner.

## Concrete surfaces and implementation

- `docs/contributing`
- `.claude/skills/testing/SKILL.md`

Verify the bounded train population and test/code/comment goals under the shared contract. Produce concise durable contributor guidance/report naming removed mechanisms, surviving owners, preserved evidence and separate production/test/comment/generated-data measures. Require delivery predecessors and a fresh cumulative train review plus checkpoint conclusions. This terminal cannot implement missing production work or silently waive required outcomes.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No new semantic/lifecycle/codegen/gate mechanisms. Svelte draft scope and #511 unplugin work are disclosed exclusions, never claimed completed cleanup.

## Acceptance and proportionate proof

- **SIMP15-AC1:** Every scoped outcome has a final owner, removed mechanism and preserved proof.
- **SIMP15-AC2:** Production, tests and comments are measured separately without quotas or unsupported runtime claims.
- **SIMP15-AC3:** Independent cumulative review passes with no unresolved required outcome.

Existing evidence to retain or extend: Delivery acceptances, current ancestry and gate summaries, before/after source measures, comparable test/build timings where available and equivalent-work evidence for runtime claims. No new evidence registry or receipt system.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Bound canonical profile and affected retained package/compile-contract evidence. L1/L2 retain final system performance/memory decisions; do not rerun unrelated corpora as a coverage quota.

Apply architecture-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
