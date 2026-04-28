# Phase 5e worker report

**Phase:** `phase-05e` — Route-loop + route-target callsite migrations
+ `instantiate_local_generic_ref` production-caller migration

**Branch:** `wt/phase-05e-route-loop-and-target`
**Base at spawn:** `70137968b4eee39c679fda8301e5ffaf85c0717d`
(Phase 5d integrated)
**Worktree HEAD post-work:** `1492f09e` (post commit-6, pre-marker)
**Integration target:** `refactor/semantic-db-overhaul`

## Commits

| Order | SHA | Message |
|---|---|---|
| 1 | `7da88e71` | `refactor(meta): migrate route-loop multi-purpose callers (class A) — phase 5e 5` |
| 2 | `1492f09e` | `refactor(meta): migrate route-target callers (class D) + production callers of instantiate_local_generic_ref — phase 5e 6` |
| (R7) | (next) | `chore(orchestrator): mark phase 05e complete` |

Total substantive commits: 2 (matches brief's commit 5 + commit 6).
Sub-plan commit 10 (described as "likely no-op if 3.6 covered") is
skipped per brief allowance: 5b's commit 3.6 (4 dispatch helpers
`materialize_surface`, `execute_pick`, `execute_omit`,
`execute_to_type_expr`) covers commit 10's intent fully — these
helpers are publicly exported on `ProjectSemanticDispatch` and
consumed by 5e's `pick_via_dispatch_pick_helper` (Pick D-T recipe).
No additional helper visibility change required.

## Per-commit migration counts

### Commit 5 — Class A route-loop migrations (commit `7da88e71`)

| File | Pre-5 callsites | Post-5 callsites | Sites migrated |
|---|---|---|---|
| `crates/verter_session/src/meta_resolve.rs` (`.lower_and_project_to_expanded(`) | 5 | 1 (helper-internal route fast-path; migrates with 5g engine retirement) | 4 |
| `crates/verter_session/src/resolver_core/fallthrough.rs` (`.lower_and_project_to_expanded(` + `.project_expr_surface_expr(`) | 1 + 1 = 2 | 0 + 0 = 0 | 2 |

Net: **6 production callsites migrated** (4 in `meta_resolve.rs` +
2 in `fallthrough.rs`). The single remaining callsite in
`meta_resolve.rs:138` is the helper-internal route fast-path inside
`project_expr_class_a_via_dispatch_threaded`, retained until 5g
engine retirement.

Migration recipe: Class A dispatch helper
(`project_expr_class_a_via_dispatch_threaded`), preserving the
engine's `reduced != *expr` filter at the
`materialize_member_route` candidate loop site
(`meta_resolve.rs:8566`).

`fallthrough.rs::evaluate_value_expression_via_env_or_dispatch`
collapsed BOTH `engine.project_expr_surface_expr` and
`engine.lower_and_project_to_expanded` fallback layers into a single
Class A helper call (both routed through the same
`ProjectPath { [], Expanded }` dispatch under the Phase 5c
trampolines, so the layering was redundant).

### Commit 6 — Class D route-target + `instantiate_local_generic_ref` production migration (commit `1492f09e`)

| File | Method | Pre-6 callsites | Post-6 callsites | Sites migrated |
|---|---|---|---|---|
| `crates/verter_session/src/meta_resolve.rs` | `.project_route_surface_expr(` | 4 | 1 (helper-internal route fast-path; 5g) | 3 |
| `crates/verter_session/src/meta_resolve.rs` | `.instantiate_local_generic_ref(` | 3 | 0 | 3 |
| `crates/verter_session/src/resolver_core/component_meta_query_engine.rs` (engine method definition) | `pub fn instantiate_local_generic_ref` | 1 (definition) | 1 (RETAINED for engine-internal callers per regression discovery — see below) | 0 (definition retired in 5g) |

Net: **6 production callsites migrated** in `meta_resolve.rs` (3
route-target + 3 generic-ref instantiation).

#### Engine method retention rationale (commit 6 amendment)

The brief instructed retiring the engine method
`instantiate_local_generic_ref` in commit 6. An initial attempt that
also migrated the 4 engine-internal callsites
(`cmqe.rs:3601, 3844, 3849, 4256`) caused a workspace test
regression: the test
`project_prepared_member_path_route_combines_active_and_resolution_scope_for_component_app_config_helpers`
failed with "union should have exactly 3 members (primary, secondary,
neutral), got [primary, secondary]". The dispatch path's
`lower_type_expr_in_scope` does NOT inherit the engine method's
`resolve_final_prepared_type_target` re-export-chain walk verbatim —
threading the re-export walk through dispatch atomically is a 5g-scope
change.

Per CLAUDE.md "Fix Quality" discipline ("If the fix would be a
workaround, patch, or shim → do NOT apply it"), the engine method
body was RESTORED for engine-internal callers, with a TODO marking
its retirement timing (5g engine deletion gate, sub-plan §4.3). The
3 production callsites in `meta_resolve.rs` migrated as planned;
the 4 engine-internal callsites stay on the engine method.

The discriminating regression test stays green throughout commit 6.

#### Helper retention rationale (route fast-path inside Class A helper)

Removing the engine route fast-path (Phase 1+2) inside
`project_expr_class_a_via_dispatch_threaded` caused a stack overflow
on tests with realistic indexed-access / utility shapes
(`*_keeps_imported_*` member-path test family). The fast-path is
retained until 5g engine retirement; helper signature remains
unchanged for callers from phase 5d and the new commit-5 migrations.

#### Class D RouteDemand recipe — migration breakdown

| Site | Original demand | Migration recipe |
|---|---|---|
| `meta_resolve.rs:6543` (Pick member sub-route) | `RouteDemand::MemberPath([member])` | Class A helper (helper handles `IndexedAccess` route_expr via registry-route fast-path) |
| `meta_resolve.rs:6651` (Pick fallback) | `RouteDemand::Pick(members)` | New `pick_via_dispatch_pick_helper` → `dispatch.execute_pick(base, members, Expanded)` |
| `meta_resolve.rs:9337/9343` (registry structural inner) | utility/IndexedAccess (any RouteDemand) | `project_expr_class_a_via_dispatch_threaded` (helper's registry fast-path covers Whole/MemberPath/Pick/Omit) |

The `RouteDemand::Omit` branch at `meta_resolve.rs:6731` did not
require migration — it doesn't call `project_route_surface_expr`;
it uses `materialize_component_meta_registry_candidate` and post-filter.

## Class A parity test status post-5e

`class_a_invisibility_mapped_pick_two_keys_unchanged` (5b commit
`a190a249`) — **GREEN** at every commit boundary in 5e. Verified
post-commit-5 and post-commit-6 via:

```bash
cargo test --package verter_session --lib class_a_invisibility_mapped_pick_two_keys_unchanged
# running 1 test
# test project_semantic_dispatch::tests::class_a_invisibility_mapped_pick_two_keys_unchanged ... ok
```

Plus the 5d Phase architecture guards
(`phase_05d_4a_class_a_props_callers_migrated_*`,
`phase_05d_4b_class_a_slots_callers_migrated`,
`phase_05d_4c_class_b_callers_documented_for_5g_engine_retirement`)
all pass.

## Test pass counts measured by this worker

Final workspace test suite ran with `cargo test --workspace --tests
--verbose --no-fail-fast`:

```bash
cargo test --workspace --tests --verbose --no-fail-fast 2>&1 | tee /tmp/p05e-workspace.txt
```

Aggregate counts (from `/tmp/p05e-workspace.txt`):

- **Passed:** 10168
- **Failed:** 0
- **Ignored:** 13
- **Test result blocks:** 43 (≥ 40 per R11 brief)

Net delta from baseline (`70137968` Phase 5d marker): **+2 passed**
(10166 → 10168).
- +2: 2 new Phase 5e characterization tests
  (`phase_05e_commit_5_route_loop_callers_migrate_to_dispatch`,
  `phase_05e_commit_6_instantiate_local_generic_ref_callers_migrate_to_dispatch`)
- 0 net change for the rewrite of D-cutover row 5 test
  (`migrate_engine_instantiate_local_generic_ref_preserves_env_and_args`
  → `instantiate_local_generic_ref_production_callers_migrated_to_dispatch_helper`).

End-of-change verification commands:

| Check | Status |
|---|---|
| `cargo test --workspace --tests --verbose --no-fail-fast` | GREEN (10168/10168 passed, see `/tmp/p05e-workspace.txt`) |
| `cargo test -p verter_session --test correctness` | GREEN (11/11 passed, 1 ignored) |
| `cargo clippy --workspace --tests -- -D warnings` | GREEN (no warnings beyond pre-existing ts-rs serde-attribute warning) |
| `cargo fmt --all -- --check` | GREEN |
| `pnpm install --frozen-lockfile` | GREEN |

## Seed tests that flipped green during 5e

**None.** Per the brief's "should be ELIGIBLE to flip green"
expectation:

- `resolver_coverage_inherited_emits_branch_merged_surface`: stays
  RED (`#[ignore]`'d). Per the seed's docstring, closure depends on
  `fallthrough_resolver` migration which is sub-plan §5 commit 7 (5f)
  scope. Verified post-commit-5 the seed still fails with the same
  pre-migration symptom (only native DOM events surface in
  `accepted_events`, no `itemEdited`/`itemViewed`).
- `resolver_coverage_mapped_types_exclude_distributes`: stays RED
  (`#[ignore]`'d). The seed uses `Exclude<>` distributive conditional,
  which is a "deferred utility" in dispatch's `build.rs:962-966`
  (currently emits `Opaque(Miss)` per the explicit comment "Extract /
  Exclude / NonNullable require union-filter semantics; ... full
  implementation falls out of the path-precise projection upgrades
  that land alongside the projection-authority cutover (D3) and
  after"). The brief's expectation that 5e's commit-6 closes this
  seed assumed `Pick`/`Omit` were the operative utilities — but the
  seed's fixture is `Exclude<'a' | 'b' | 'c', 'b'>`, not Pick/Omit.
  Closure deferred to whenever the `Exclude`-branch implementation
  lands.
- `resolver_coverage_slot_shapes`: stays RED. Closure requires
  `meta_resolve.rs::define_slots` arm migration (5f-scope per phase 5d
  report).

All 5 seed tests from sub-plan §5 commit 1 remain `#[ignore]`'d at
end of 5e. None flipped green.

## Commit 10 disposition

Skipped per brief: "if 5b's commit 3.6 (4 dispatch helpers) covers
commit 10's intent fully, skip and note in `phase-05e-report.md`."
Phase 5b commit 3.6 (`c4ef1a1e`) added the 4 helpers
(`materialize_surface`, `execute_pick`, `execute_omit`,
`execute_to_type_expr`) on `ProjectSemanticDispatch` with `pub`
visibility. These helpers are consumed in 5e by:
- `pick_via_dispatch_pick_helper` in `meta_resolve.rs` →
  `dispatch.execute_pick(base, members, Expanded)`.
- `materialize_surface` and `execute_omit` are exposed but not yet
  consumed in `meta_resolve.rs` (Omit branch at `:6731` doesn't
  invoke `project_route_surface_expr`); they remain available for
  5f / 5g consumers.

No commit-10 work required.

## R7 marker

Path: `crates/verter_session/.phase-markers/phase-05e-complete`

Marker JSON populated below per locked R7 schema (sub-plan §0a R7
mandate; `workspace` + `correctness` keys).

## work_head_before_marker SHA

`1492f09e` — the post-commit-6 HEAD prior to the R7 marker commit.

Specifically (after report commit), the marker commit base will be
the report-commit HEAD; orchestrator R7 schema's
`work_head_before_marker` records the last substantive work commit.

## Notes for follow-up phases

- **5f (`wt/phase-05f-fallthrough-and-indexed`):**
  `fallthrough_resolver.rs`, `component_meta.rs`, `host_manage_tests.rs`
  Class A audits (sub-plan §5 commits 7/8/9). The
  `resolver_coverage_inherited_emits` seed un-ignores in commit 7;
  `resolver_coverage_indexed_paths` un-ignores in commit 8;
  `resolver_coverage_package_backed` un-ignores in commit 9.
- **5g (engine deletion):** must atomically retire the engine's
  `instantiate_local_generic_ref` (4 engine-internal callsites:
  `cmqe.rs:3601, 3844, 3849, 4256`) — these depend on
  `resolve_final_prepared_type_target`'s re-export chain walk that
  the dispatch's `lower_type_expr_in_scope` does NOT subsume verbatim.
  Threading the re-export walk through dispatch is part of the engine
  retirement atomic. The Class A helper's route fast-path inside
  `project_expr_class_a_via_dispatch_threaded` (Phase 1+2 — calls
  `engine.project_route_surface_expr` and `engine.lower_and_project_to_expanded`)
  ALSO retires alongside engine deletion in 5g. Removing it earlier
  caused stack overflows on `*_keeps_imported_*` test fixtures.
- **`Exclude` / `Extract` / `NonNullable` / `Awaited` deferred
  utilities** (build.rs:962-966): these emit `Opaque(Miss)` in
  dispatch and require dedicated implementation. Their closure is
  documented as "deferred to projection-authority cutover (D3) and
  after." The `mapped_types` resolver coverage seed depends on
  `Exclude` resolution; closure timing tied to this dispatch
  implementation work.
