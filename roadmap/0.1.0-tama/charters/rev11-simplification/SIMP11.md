<!-- unified-charter-v2
id=SIMP11
name=Consolidate integration-test layout verification
predecessors=SIMP3,TCM1
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:existing Cargo-metadata layout tool
conflict_domains=test_maintenance,release_orchestration
resource_class=rust-mixed
gate_profile=targeted-domain
review_profile=semantic-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=medium
review_effort_default=medium
verification_effort_min=medium
verification_effort_default=high
confirmation_effort_min=medium
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
external_requirements=
charter=charters/rev11-simplification/SIMP11.md
size=M
max_production_loc=250
max_production_files=3
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP11 — Consolidate integration-test layout verification

## Independently acceptable outcome and owner

One existing metadata-based check serves local and CI verification without duplicate Rust logic or hidden test-target discovery.

Current problem: An 829-line Rust test duplicates the Node Cargo-metadata layout check already run before Rust CI, maintaining two implementations for one build-cost invariant.

Final owner: **existing Cargo-metadata layout tool**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP3:** Test guidance assesses actual proof value without recursive guard-registration machinery or mandatory AI-authorship markers.
- **TCM1:** landed compact mapping products and current CI/mapping consumers.

## Concrete surfaces and implementation

- `crates/verter_session/tests/cases/integration_test_layout_guard.rs`
- `scripts/check-integration-test-layout.mjs`
- `scripts/integration-test-layout-allowlist.json`
- `scripts/gate.mjs`
- `.github/workflows/ci.yml`

Compare structural/discovery cases, preserve unique valid cases in the existing Node tool and focused fixtures, remove the Rust mirror/module wiring, and reuse the tool once per owning local/CI invocation. Preserve SG0's restored lane and TCM1's CI changes, both ancestors.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No replacement gate framework, unrelated CI changes, changed primary test universe or production compiler/runtime behavior.

## Acceptance and proportionate proof

- **SIMP11-AC1:** One implementation owns integration-target layout checks.
- **SIMP11-AC2:** Local and CI discovery still reject hidden or duplicate binaries.
- **SIMP11-AC3:** Duplicate checking is removed without weakening shipped or compile-contract lanes.

Existing evidence to retain or extend: Actual Cargo metadata and existing cases for extra binaries, hidden autotests, duplicate targets and stale exceptions; demonstrate the remaining real entry-point invocation without a new workflow-string scanner.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

node scripts/check-integration-test-layout.mjs; focused existing tool/entry-point tests; node scripts/gate.mjs.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
