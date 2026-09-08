<!-- unified-charter-v2
id=SIMP6
name=Retire provider source-shape backstops
predecessors=SIMP3
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:provider admission, protocol and lifecycle behavior tests
conflict_domains=provider_lifecycle,lsp_publication,test_maintenance
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
charter=charters/rev11-simplification/SIMP6.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=3
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP6 — Retire provider source-shape backstops

## Independently acceptable outcome and owner

Provider safety is verified through actual admission, protocol traffic, replay and close behavior without source-shape backstops.

Current problem: Provider tests infer admission, witness use and lifecycle ordering from source strings and hand-written body parsers, duplicating real protocol/lifecycle coverage.

Final owner: **provider admission, protocol and lifecycle behavior tests**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP3:** Test guidance assesses actual proof value without recursive guard-registration machinery or mandatory AI-authorship markers.

## Concrete surfaces and implementation

- `crates/verter_session/tests/g_extts`
- `crates/verter_lsp/tests/cases/editor_liveness_guards.rs`
- `crates/verter_lsp/tests/cases/owned_binding_gate.rs`
- `crates/verter_lsp/src/server_tests.rs`
- `crates/verter_type_runtime/src/tsgo/ipc_tests.rs`
- `crates/verter_type_runtime/src/resilient_tests.rs`

Retire scanner portions of no_fallback_to_inferred_anywhere, non_owning_attach_lifecycle, in_band_witness_feeds_gate, resilient_single_writer_actor_shape, ledger_is_off_the_serve_path and editor_liveness_guards. Inspect g_extts siblings by real contract, retain behavioral cases in mixed files, and remove exclusive parsers/allowlists/decoy fixtures. Reuse isolated domain-local setup; cover an actual missing witness/protocol boundary before deleting its only useful check.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No production provider lifecycle, selection, project binding or H2/H3 ownership changes.

## Acceptance and proportionate proof

- **SIMP6-AC1:** Provider source scanners and exclusive parsers are retired.
- **SIMP6-AC2:** Configured-project, non-owning protocol and close/replay boundaries retain discriminating evidence.
- **SIMP6-AC3:** Shared test setup stays local and isolated with useful failure diagnostics.

Existing evidence to retain or extend: owned_binding_gate counted provider delegation; server_tests failed-sync retention/close-after-success/reopen races; initialized transport protocol and restart membership tests. Explicitly map detach/no-exit and malformed-witness behavior: hover coverage alone is insufficient.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused hermetic LSP/runtime/session suites and node scripts/gate.mjs; existing real-provider lane only for affected boundaries that require it.

Apply concurrency-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
