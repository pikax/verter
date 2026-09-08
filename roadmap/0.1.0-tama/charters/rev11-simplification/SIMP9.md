<!-- unified-charter-v2
id=SIMP9
name=Unify stale provider close operations
predecessors=SIMP6
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:existing LSP provider-surface close lifecycle
conflict_domains=provider_lifecycle,lsp_publication
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
charter=charters/rev11-simplification/SIMP9.md
size=M
max_production_loc=350
max_production_files=4
max_related_packages=1
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP9 — Unify stale provider close operations

## Independently acceptable outcome and owner

One existing lifecycle owner closes stale provider surfaces with correct tokens, failure recovery and reopen ordering.

Current problem: Background drain and workspace scanning duplicate forget, per-kind close and finalize-on-success operations with repeated lifecycle prose.

Final owner: **existing LSP provider-surface close lifecycle**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP6:** Provider safety is verified through actual admission, protocol traffic, replay and close behavior without source-shape backstops.

## Concrete surfaces and implementation

- `crates/verter_lsp/src/background_drain.rs`
- `crates/verter_lsp/src/workspace_scanner.rs`
- `crates/verter_lsp/src/server/provider_state.rs`

Bind close_stale_provider_paths and close_stale_paths to current per-kind/token operations. Extract the shared operation under the existing surface owner and migrate equivalent callers. Keep caller logging context explicit; preserve forget-before-dispatch, confirmed-success finalization, epoch checks and typed declaration-overlay exclusion. Delete duplicate loops and copied rationale without retaining a forwarding chain.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No provider selection, new state machine, H2/H3 ownership change or unrelated sync scheduling refactor.

## Acceptance and proportionate proof

- **SIMP9-AC1:** Equivalent stale-close callers use one operation owner.
- **SIMP9-AC2:** Failed closes and concurrent reopen preserve correct state.
- **SIMP9-AC3:** Per-kind behavior and declaration-overlay exclusions remain intact.

Existing evidence to retain or extend: Failed replacement sync, close-after-success, stale-close/reopen ABA and declaration-overlay tests; failed closes retain recoverable state and stale tokens cannot finalize reopened generations.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

Focused LSP close/sync/reopen tests and node scripts/gate.mjs.

Apply concurrency-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
