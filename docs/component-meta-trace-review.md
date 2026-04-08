# Component-Meta Trace Review Log

## 2026-04-08T22:18:00.9567383+01:00 - Batch 1

- Active batch:
  - `src/runtime/components/Accordion.vue`
  - `src/runtime/components/Alert.vue`
  - `src/runtime/components/App.vue`
- Latest trace artifact directory: `tmp/first3-alpha-trace-rerun7`
- Judgment: `FAIL`

### Findings

1. No trustworthy post-change trace proof exists for current `HEAD`.
   - The latest first-batch artifact directory on disk is `tmp/first3-alpha-trace-rerun7` with timestamps around `2026-04-08 21:08`.
   - The latest executor commits landed after that artifact:
     - `43def380` `refactor(verter_session): remove component-meta legacy cache fallbacks` at `2026-04-08 22:05 +01:00`
     - `a668873a` `refactor(verter_session): remove imported type alias leftovers` at `2026-04-08 21:37 +01:00`
   - Batch 1 cannot be accepted until traces are regenerated from current code into a new artifact directory.

2. Desired trace specs for the active batch are missing.
   - The validator exists in `packages/benchmark/src/trace-validator.ts` and has coverage in `packages/benchmark/src/trace-validator.spec.ts`.
   - No committed per-component desired-trace specs were found for `Accordion.vue`, `Alert.vue`, or `App.vue`.
   - That means there is no normalized gate covering required patterns, forbidden patterns, max count thresholds, max duration thresholds, and assertion notes/rationale for Batch 1.

3. `tmp/first3-alpha-trace-rerun7` is not trustworthy as a comparison baseline.
   - The campaign's recorded known-bad baseline for that same directory says:
     - `Accordion`: `current_eval_state=137979`, `types/index.ts store_view=false=17544`
     - `Alert`: `current_eval_state=159630`, `types/index.ts store_view=false=18576`
     - `App`: `current_eval_state=70460`, `types/index.ts store_view=false=4644`
   - The current files at that path now show materially different traces:
     - `Accordion`: `current_eval_state=47`, `types/index.ts store_view=false=0`, `authoritative_import_route_in_view_result=148`, `source=module_facts=152`
     - `Alert`: `current_eval_state=47`, `types/index.ts store_view=false=0`, `authoritative_import_route_in_view_result=107`, `source=module_facts=114`
     - `App`: `current_eval_state=27`, `types/index.ts store_view=false=0`, `authoritative_import_route_in_view_result=67`, `source=module_facts=72`
   - The file sizes also changed sharply:
     - `rerun6`: ~`38.8MB` / `45.8MB` / `27.1MB`
     - `rerun7`: ~`1.9MB` / `1.3MB` / `1.6MB`
   - Because the same artifact path now represents a different trace surface, this is a trace-trust problem until the batch is rerun, normalized, and validated against committed specs.

4. New fuse gates create a fake-win risk that is not yet covered by correctness tests.
   - `crates/verter_session/src/resolver_core/fuses.rs` now introduces default budgets for wildcard-route, imported-root, registry-deepening, projection, structural slow-lane, and union-member work.
   - `crates/verter_session/src/meta_resolve.rs` now breaks or skips work when `allow_registry_deepening()`, `allow_imported_root()`, or `allow_union_member()` refuses more work.
   - Existing tests prove fuse accounting and some cache behavior, but I did not find Batch-1-level component-meta correctness tests showing that these bailouts preserve the published metadata shape when they trip.
   - Faster traces are not sufficient evidence while this gap remains.

5. Negative tests exist for several forbidden legacy paths, but the active batch still lacks batch-specific negative validation.
   - Existing negative coverage observed:
     - `prepared_type_decl_in_view_does_not_require_import_route_shadow_materialization`
     - `route_and_root_resolution_do_not_fall_back_through_frontier`
     - `component_meta_queries_do_not_populate_legacy_resolved_type_cache`
     - the slow-lane guard path in `meta_tests.rs` using `forbid_import_route_shadow_for_tests()` and `forbid_structural_slow_lane_for_tests()`
   - Missing for Batch 1:
     - committed forbidden trace assertions proving legacy fallback is absent
     - committed forbidden trace assertions proving raw snapshot / repeated `current_eval_state` reopening is absent where published facts should suffice
     - negative tests around newly fuse-gated bailout paths that could hide wrong answers

6. TDD was not demonstrated for this batch.
   - New tests exist in the touched areas, but the available commit history does not show a clear failing-test-first step before the fixes.
   - Until that evidence exists, treat this as a process failure rather than assuming TDD happened.

7. Workspace verification evidence from the executor is stale.
   - The older repo log `tmp/workspace-tests-after-session-fix.log` predates the latest Batch-1 commits and does not prove current `HEAD` is green.
   - Reviewer reran `cargo test --workspace --tests --verbose` and captured the output in `tmp/reviewer-workspace-tests-2026-04-08.log`.
   - The log shows passing test bodies, including the tail result `355 passed; 0 failed`.
   - The command still returned non-zero on this Windows host because the `verter_napi` runner emitted many `Load Node-API [...] failed: GetProcAddress failed` lines before its tests passed.
   - Regardless, there is still no executor-owned post-commit workspace run attached to Batch 1.

8. Commit protection is mixed, not clean.
   - Progress is at least protected by several recent commits and the worktree is clean.
   - Commit frequency is acceptable for the current slice.
   - Commit naming is not fully acceptable because `efe39961` is `interim`, which does not follow the repository's conventional-commit rule.

9. Progress/follow-up docs for the active batch are stale.
   - I found `docs/component-meta-trace-audit-v7.md` and `docs/component-meta-non-route-follow-up-plan.md`, both dated `2026-04-05`.
   - I did not find a newer Batch-1 progress log or follow-up ledger showing what remains open after the latest executor commits.

### Missing Validation Before Batch 1 Can Pass

- Regenerate Batch-1 traces from current `HEAD` into a new artifact directory.
- Commit desired-trace specs for `Accordion.vue`, `Alert.vue`, and `App.vue` using the validator schema:
  - required patterns
  - forbidden patterns
  - max count thresholds
  - max duration thresholds
  - note/rationale for each assertion
- Add or point to negative tests that prove newly fuse-gated bailouts do not hide wrong component-meta results.
- Record an executor-owned `cargo test --workspace --tests --verbose` run after the relevant Batch-1 commits.
- Update the progress/follow-up docs with the current batch state instead of relying on the older v7 audit documents.
