<!-- unified-charter-v2
id=SIMP4
name=Retire historical source and campaign guards
predecessors=SIMP3,TCM1,CCA1O4D
phase=rev11
train=rev11.simplification
product=rev11
kind=implementation
semantic_role=delivery
class=foundational
owner=rev11.simplification:current contract evidence and contributor guidance
conflict_domains=test_maintenance
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
charter=charters/rev11-simplification/SIMP4.md
size=S
max_production_loc=0
max_production_files=0
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
-->

# SIMP4 — Retire historical source and campaign guards

## Independently acceptable outcome and owner

Historical-only guards and exclusive support are deleted while live schema, portability and cache-behavior evidence remain.

Current problem: Deleted names, old file moves, roadmap wording and completed campaign prose remain enforced by source tests with parsers, exception lists and self-tests.

Final owner: **current contract evidence and contributor guidance**. Read the binding [codebase simplification contract](../../contracts/codebase-simplification.md). This block owns one independently landable outcome; deletion and its evidence form one coherent change.

## Exact predecessor contracts

- **SIMP3:** Test guidance assesses actual proof value without recursive guard-registration machinery or mandatory AI-authorship markers.
- **TCM1:** landed compact mapping products and current CI/mapping consumers.
- **CCA1O4D:** landed native legacy compile-profile deletion.

## Concrete surfaces and implementation

- `crates/verter_session/tests/cases/architecture_guards.rs`
- `crates/verter_session/tests/cases/g_block/cache_runtime_singleflight_rehome.rs`
- `crates/verter_session/tests/cases/g_block/block_1_i_discriminators.rs`
- `crates/verter_source_policy_gate/tests/cases/output_projector_residual_guards.rs`
- `crates/verter_source_policy_gate/tests/cases/scanners_replacement.rs`
- `crates/verter_source_policy_gate/tests/cases/tracked_paths_no_machine_roots.rs`
- `scripts/verify-scanners-replacement.mjs`
- `scripts/verify-scanners-replacement.test.mjs`

Retire retired_utility_shape_cluster_*, phase_05l_engine_resolver_methods_deleted, parser_type_surface_engine_files_are_absent, from_analysis_inner_name_is_retired_in_session and no_production_route_owned_shallow_system. Retire phase-archaeology scans/allowlists, historical split/line budgets, singleflight rehome checks and construct-then-match mirrors. Remove retired_kind_b_bridge_* and closed campaign/retired-extension checks from mixed files. Remove source-reading fact-invalidation checks while keeping actual publish/invalidate tests. Retire the 64 historical-machine-root tombstone after separating any required live config validation. Check real callers before retiring the manual campaign tool. Remove exclusive helpers, fixtures and stale owning-doc references together.

Bind current paths/symbols before mutation. Keep distinct fixture behavior and isolated state; no universal test DSL. Source comments explain current non-obvious behavior, not roadmap history.

## Exact deletions and exclusions

The named obsolete mechanisms and their exclusively used helpers, tests, imports and owning-document references are removed together. Preserve useful assertions in mixed files. No hot-materialization analyzer removal (SIMP5), Svelte-specific guards/integration manifests, production APIs, unplugin, locks or CI workflow changes.

## Acceptance and proportionate proof

- **SIMP4-AC1:** Deleted identities, historic layouts and completed campaign wording no longer require runtime source guards.
- **SIMP4-AC2:** Exclusive scanner helpers and exception bookkeeping are deleted with their consumers.
- **SIMP4-AC3:** Live schema, portability, parse/build dedup and invalidation evidence remain.

Existing evidence to retain or extend: cold_artifact_dedup_tests.rs; lazy_decl_body_tests.rs; semantic_memo_invalidate_drains_fact_canonical_entry; live schema validation; tracked_paths_are_portable checkout checks. Mixed files are not whole-file deletion approvals.

A retired spelling, filename or prose convention needs no replacement test. A real uncovered behavioral boundary requires the smallest failing discriminating test before a behavior change. Compiler/type/capability evidence and bounded inspection are valid for non-behavioral changes; do not add scanner or mutation companions to populate acceptance IDs.

## Identity, cutover and work laws

Retain exact source/view/project identity, position units, read-set validity, complete-only admission, bounded retention and cancellation wherever applicable. Characterize current behavior, switch the scoped callers once, and remove their old path in the same candidate. No dual authority or compatibility route is retained to ease the migration.

Report production, tests, comments and removed mechanisms separately. Test retirement claims maintenance/build benefit, not shipped runtime speed. Touched hot paths use equivalent-work/allocation/retention evidence and the controlling metrics in contracts/resource-and-finalization.md. Preserve applicable disabled/inapplicable zero-work. No arbitrary percentage or LOC quota; size fields are planning references.

## Verification, review and completion

node scripts/gate.mjs; affected existing tool tests and compile contracts; pnpm docs:build when shared guidance changes.

Apply semantic-3 fresh review and the ordinary train checkpoints/final review from APPLICATION.md. Execute every bound final gate; disclose selected/executed work, prerequisites and unexpected skips. Core PASS cannot stand for an omitted shipped, provider or compile-contract lane.

Stop and amend if the current owner is different, an active reservation must be changed, or another independently landable outcome appears. Missing substantive mechanisms return to their owner; do not silently narrow acceptance or build a framework. If predecessors already removed a named population, verify that result without manufacturing new work.

This amendment leaves the implementation row pending. Only the later implementation candidate transitions it under APPLICATION.md with ordinary locator hints.
