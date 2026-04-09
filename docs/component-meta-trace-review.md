# Component-Meta Trace Review Log

## 2026-04-09T04:47:09.7218947+01:00 - Batch 1

- Active batch:
  - `src/runtime/components/Accordion.vue`
  - `src/runtime/components/Alert.vue`
  - `src/runtime/components/App.vue`
- Latest trace artifact directory: `tmp/batch1-trace-003`
- Full-batch trace evidence: `tmp/batch1-trace-002`
- Judgment: `FAIL`

### Findings

1. The new `store_view=false` forbidden assertions are written against the wrong field, so one of the key negative guards is ineffective.
   - `packages/benchmark/src/trace-validator.ts` matches `namePattern` against the event name and `detailPattern` against the event detail.
   - The new Batch 1 specs use:
     - `namePattern: "/store_view=false/"`
     - `detailPattern: "types/index.ts"`
   - `store_view=false` appears in trace details, not in event names, so these assertions can never match and therefore can never fail.
   - That leaves the intended guard against `types/index.ts` permissive reopening behavior effectively unenforced.

2. The new trace-check harness is not usable as a per-batch validation gate.
   - `packages/benchmark/src/trace-check.ts` scans every committed spec under `packages/benchmark/trace-specs/component-meta/`, not the active batch.
   - It treats missing trace files as `SKIP` rather than failure.
   - Confirmed behavior:
     - `npx tsx packages/benchmark/src/trace-check.ts tmp/batch1-trace-002 --strict`
       - Batch 1 traces passed, but the command still failed because Batch 2 specs are under-specified.
     - `npx tsx packages/benchmark/src/trace-check.ts tmp/batch1-trace-003 --strict`
       - `Accordion` passed, `Alert` and `App` were skipped because their traces were missing from the directory.
   - That means the harness cannot currently answer the question the campaign needs answered: whether the active batch is fully validated.

3. The stale-archive regression reported in the prior review appears fixed.
   - `fix(verter_session): prevent stale archived facts for edited untracked deps` adds strict archive validation and targeted regression coverage.
   - Verified by running:
     - `cargo test --package verter_session archived_module_facts_rejected_when_workspace_dep_changes_content --tests --verbose`
     - `cargo test --package verter_session validates_archived_rejects_untracked_file_whole_hash --tests --verbose`
   - Both targeted tests passed.

4. Workspace verification is current from the reviewer side.
   - Reviewer reran `cargo test --workspace --tests --verbose` on current `HEAD` and captured the output in `tmp/reviewer-workspace-tests-2026-04-09b.log`.
   - The log tail again shows passing test bodies, including `355 passed; 0 failed`.
   - The shell still returned non-zero on this Windows host because the `verter_napi` runner emitted host-runtime `GetProcAddress failed` lines before its tests passed.

5. Commit protection remains acceptable.
   - The new work is committed with conventional messages.
   - The worktree is clean.

## 2026-04-09T04:11:23.7033435+01:00 - Batch 1

- Active batch:
  - `src/runtime/components/Accordion.vue`
  - `src/runtime/components/Alert.vue`
  - `src/runtime/components/App.vue`
- Latest trace artifact directory: `tmp/batch1-trace-003`
- Full-batch trace evidence: `tmp/batch1-trace-002`
- Judgment: `FAIL`

### Findings

1. `fix(verter_session): accept untracked dependency files in store view validation` introduces a stale-cache correctness risk for edited dependency files.
   - `crates/verter_session/src/resolver_store.rs` now accepts untracked `FileWholeHash` and untracked `DerivedFactKind::DirectSource` facts with `None => true`.
   - `HostStoreView::checks_archive()` is still `true`, so archived entries remain visible to store-view lookups.
   - On file edits, `crates/verter_session/src/host_upsert.rs` still calls `self.resolver.runtime.evict_canonical(&canonical_id)`, which only soft-evicts provider-owned caches.
   - `ValidatedFactCache::remove()` explicitly leaves archived entries in place under the assumption that whole-hash mismatch will block stale reuse.
   - That assumption is no longer valid for untracked dependency files. After an edit, stale archived module facts, routes, imported roots, and type surfaces can validate and be reused because the new store view does not track that file and now treats the missing whole hash as valid.
   - This needs a regression test that edits an untracked dependency file between requests and proves stale archived facts are not returned.

2. Batch 1 desired-trace specs are still too weak to protect against the known bad path.
   - `packages/benchmark/trace-specs/component-meta/Accordion.json`
   - `packages/benchmark/trace-specs/component-meta/Alert.json`
   - `packages/benchmark/trace-specs/component-meta/App.json`
   - All three specs still set `forbidden` to `[]`, which leaves no committed negative assertions for legacy fallback or reopened slow-path behavior.
   - The current Batch 1 traces already provide concrete signals that should be guarded:
     - exact `current_eval_state` counts are `38` / `34` / `24`
     - exact `types/index.ts store_view=false` counts are `0` / `0` / `0`
     - exact `seed_imported_dependency_base_in_view` counts are `0` / `0` / `0`
     - exact `legacy_resolved_type_cache` counts are `0` / `0` / `0`
   - Without forbidden assertions or count thresholds on those regression signals, the new specs do not satisfy the campaign requirement for intentional negative validation.

3. I did not find any committed repo path that actually runs the new validator against the batch artifacts.
   - `packages/benchmark/src/trace-validator.ts` and its unit tests are committed.
   - Repo search only finds validator references in the validator module, its tests, and the progress/review docs.
   - `docs/component-meta-trace-progress.md` says traces are validated against desired specs, but I did not find a committed harness or command in the repo that loads the specs and checks the real `tmp/batch1-trace-*` artifacts.
   - Until that exists, spec coverage remains advisory rather than enforced.

4. The latest Batch 1 artifact directory is incomplete.
   - `tmp/batch1-trace-003` contains only `Accordion`.
   - `Alert` and `App` still rely on `tmp/batch1-trace-002` for the current proof set.
   - The Batch 1 thresholds in the committed specs are plausible against `tmp/batch1-trace-002`, but the latest rerun did not refresh the full active batch.

5. Workspace verification is current from the reviewer side, but still missing from the executor side.
   - Reviewer reran `cargo test --workspace --tests --verbose` on current `HEAD` and captured the output in `tmp/reviewer-workspace-tests-2026-04-09.log`.
   - The log tail shows passing test bodies, including `355 passed; 0 failed`.
   - The command still returned non-zero on this Windows host because `verter_napi` emitted host-runtime `GetProcAddress failed` lines before its tests passed.
   - I still did not find an executor-owned workspace test run recorded after the latest Batch 1 commits.

6. Commit protection is acceptable on this pass.
   - Progress is committed frequently with conventional messages after the earlier `interim` commit.
   - The worktree is clean.

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
   - The validator logic currently exists only as local untracked files:
     - `packages/benchmark/src/trace-validator.ts`
     - `packages/benchmark/src/trace-validator.spec.ts`
   - Because those files are not in git, they are not yet a committed campaign gate.
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
