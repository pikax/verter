# Component-Meta Non-Route Follow-Up Plan

Date: 2026-04-05

Primary context:
- Main cutover plan: `/Users/carlosrodrigues/.claude/plans/transient-booping-sunbeam.md`
- Current route audit: `/Users/carlosrodrigues/Documents/dev/verter/docs/component-meta-trace-audit-v7.md`
- Canonical repo rules: `/Users/carlosrodrigues/Documents/dev/verter/AGENTS.md`
- Canonical architecture/invariants: `/Users/carlosrodrigues/Documents/dev/verter/CLAUDE.md`

## In Scope

- Full-pass non-barrel outlier audit after the raw-load fix.
- Trace harness stability and repeatability gaps.
- Benchmark JSON / markdown observability gaps for named outliers.
- Separation of:
  - route-proof / miss-proof / root-identity costs
  - deeper type expansion / solver / metadata materialization costs

## Out Of Scope

- Replanning the main route-ownership cutover.
- Legacy barrel subsystem removal.
- Wildcard-route `current_eval_state` churn already owned by the main cutover.
- `export_graph` and imported runtime values as parallel semantic route consumers.
- Any dual-path, shim, compatibility wrapper, or staged coexistence design.

## Architectural Decision

This document assumes the strongest end-state architecture for the remaining non-route scope, regardless of breaking changes.

Sequencing below is only about ownership boundaries and measurement order. It is not an incremental architecture recommendation.

Target end state:

1. One hard-timeout component-meta trace / benchmark harness.
   - All corpus tracing and stress work runs out-of-process.
   - The parent process owns setup/query/close timeouts and final status.
   - Same-process `Promise.race` timeouts are deleted as the benchmark authority path.

2. One canonical per-component benchmark result schema.
   - Benchmark artifacts store named component rows.
   - Aggregate scenario/backend summaries are derived from those rows.
   - Anonymous latency arrays are not the primary persisted shape.

3. One host-owned non-barrel import-proof cache.
   - Direct import proof, including proven miss, is cached once per owner canonical file hash / store-view context.
   - Component-meta, solver, and store-view projection consume those facts instead of re-entering proof paths.
   - The cache stores both positive and negative answers; a miss is a first-class fact, not “absence of a result”.

4. One host-owned imported-root proof cache.
   - Imported type root resolution, including proven unresolved roots, is cached once per canonical provider file hash / store-view context.
   - `solver_host` reads that proof directly.
   - Negative roots such as unresolved package symbols are first-class cached facts, not repeated fallback work.

5. Solver and metadata materialization stay downstream of proof.
   - `dependency_resolutions_for_eval_in_view()`, `resolve_imported_type_root_in_view()`, and solver lookup paths should become readers of host-owned proof state, not recompute points.
   - Materialization remains demand-driven and query-scoped.
   - No AST/source fallback is introduced after the cache-owning pass.

6. If non-barrel residuals remain after the main barrel cutover, they should land as one clean second-pass proof-cache cutover.
   - Do not split direct-miss proof caching and imported-root negative caching into separate long-lived architectures.
   - They are two surfaces of the same remaining proof problem.

## Findings

### 1. The remaining named outliers are not one bucket

Sampled v7 trace results:

| Component | Query ms from stdout | Dominant repeated signal | Likely bucket | Disposition |
| --- | ---: | --- | --- | --- |
| `ChatPrompt.vue` | `202ms` | `src/runtime/types/index.ts` miss churn (`129` misses) | Barrel route-proof | Main cutover |
| `CheckboxGroup.vue` | `432ms` | `src/runtime/types/index.ts` miss churn (`129` misses) | Barrel route-proof | Main cutover |
| `CommandPalette.vue` | `512ms` | owner-local miss proof (`43` owner misses), `72` imported-root calls | Non-barrel route-proof | Validate after cutover, then second pass |
| `SelectMenu.vue` | `459ms` | owner-local miss proof (`42` owner misses), `84` imported-root calls, large payload (`133.9KB`) | Mixed, non-barrel | Validate after cutover, then second pass |
| `InputMenu.vue` | `459ms` | owner-local miss proof (`43` owner misses), `76` imported-root calls, large payload (`132.5KB`) | Mixed, non-barrel | Validate after cutover, then second pass |
| `Select.vue` | `440ms` | owner-local miss proof (`37` owner misses), `78` imported-root calls | Mixed, non-barrel | Validate after cutover, then second pass |
| `Checkbox.vue` | `382ms` | owner-local miss proof (`25` owner misses), `26` imported-root calls | Mixed, non-barrel | Validate after cutover, then second pass |
| `NavigationMenu.vue` | `430ms` | owner-local miss proof (`34` owner misses), `74` imported-root calls | Mixed, non-barrel | Validate after cutover, then second pass |
| `ChangelogVersion.vue` | `578330ms` in full sweep, `96ms` isolated rerun | Trace/runner mismatch | Harness instability | Separate second pass now |

Key point:

- Outside the main barrel cutover, the sampled traces are still dominated by proof/lookup events:
  - `cached_dependency_resolution_in_view_result`
  - `current_eval_state`
  - `dependency_resolutions_for_eval`
  - `resolve_imported_type_root`
  - `seed_imported_dependency_base_in_view`
- The deeper materialization events are not repeating in the same way:
  - `component_meta_parts` appears once per sampled query
  - `extract_component_meta` is tiny in the sampled `ChangelogVersion.vue` traces

Conclusion:

- The largest residual outside the main cutover is not broad deep expansion.
- It is repeated proof of direct unresolved imports and repeated negative imported-root work on non-barrel paths.

### 2. The main cutover should own only the barrel-dominated residuals

Confirmed barrel-dominated residuals from the sampled set:

- `ChatPrompt.vue`
- `CheckboxGroup.vue`

Evidence:

- Their miss owner is overwhelmingly `src/runtime/types/index.ts`.
- Their current cost is still aligned with the main cutover's barrel-route ownership problem.

These should not be re-planned here.

### 3. There is a separate non-barrel route-proof problem on direct unresolved imports

For the non-barrel outliers, the top miss imports are mostly direct unresolved proofs on the owner file:

- `#imports`
- `#build/ui/<component>`
- `vue`
- `@nuxt/schema`
- `reka-ui`
- `../types`
- `../types/html`
- `../types/input`
- `../types/utils`
- `../composables/useComponentIcons`

This is not the wildcard-barrel problem.

It points at repeated direct miss proof work in:

- `crates/verter_session/src/host_resolve.rs`
- `crates/verter_session/src/host_manage.rs`

The current shape still re-enters:

- `current_eval_state_in_view()`
- `dependency_resolutions_for_eval_in_view()`
- structural dependency merge / store-view merge

to rediscover the same unresolved direct imports.

Best architectural fix:

- Make direct import proof a host-owned fact keyed by owner canonical file hash plus store-view context.
- `cached_dependency_resolution_in_view()` and `dependency_resolutions_for_eval_in_view()` should project from that fact store instead of re-proving misses through eval-state reads.

### 4. There is a separate imported-root / solver-negative-caching problem around `tv.ts`

Across the sampled non-barrel outliers:

- `resolve_imported_type_root` is called `46-84` times
- `solver_resolve_prepared_type_decl_result` is called `43-82` times
- repeated misses appear on `tailwind-variants`

Representative trace shape from `ChangelogVersion.vue`:

- `prepared_type_decl_in_view_result source=missing_shallow hit=false`
- `resolve_imported_type_root_result ... source=fallback_self target_canonical=tailwind-variants target_symbol=ClassValue`
- `solver_resolve_prepared_type_decl_result ... source=root_resolve_same hit=false`

This is not the main barrel problem.

It is negative imported-root proof churn and solver retry churn on unresolved package symbols, primarily owned by:

- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/resolver_core/solver_host.rs`

Best architectural fix:

- Make imported-root resolution a host-owned fact keyed by canonical provider file hash plus store-view context.
- Cache proven unresolved roots the same way successful roots are cached.
- `solver_host` should consume the root proof directly and should not re-run the same `root_resolve_same` negative path.

### 5. The trace harness is not trustworthy enough for second-pass perf work

`ChangelogVersion.vue` is the clearest proof:

- Full sweep summary: `elapsed_ms=578516`, `status=failed`, `signal=SIGTERM`
- Full sweep stdout: `Done in 578330ms`
- Full sweep native trace: `resolve_component_meta dur_ms=387.822`
- Isolated rerun stdout: `Done in 96ms`
- Isolated rerun trace: `resolve_component_meta dur_ms=57.651`

That means the current trace workflow cannot distinguish:

- real semantic slowness
- blocked event loop / suspension / process-starvation
- shutdown / close instability
- external termination after partial success

Current harness weaknesses:

- `packages/benchmark/src/_trace-component.ts` uses `Promise.race` in-process for timeout.
- `packages/benchmark/src/meta-ui-stress.ts` uses the same soft timeout pattern.
- `packages/benchmark/src/query-timeout.ts` does not cancel or isolate the underlying query.
- `scripts/benchmark/run-hard-timeout.mjs` exists, but there is no repo-owned corpus runner that standardizes its use and writes structured status.

Best architectural fix:

- Promote the hard-timeout parent/child model to the only trace authority path.
- Reuse one runner shape for corpus tracing and stress execution rather than keeping separate timeout models.

Important consequence:

- A same-process timeout is not a hard watchdog for native or event-loop-blocking hangs.
- This must be fixed before using the corpus sweep as the authority for second-pass non-route work.

### 6. Benchmark JSON and reports lose named outliers

Current schema problem:

- `packages/benchmark/src/meta-ui-report.ts` stores only `componentLatenciesMs: number[]`
- repeats also store only `orderStart`
- the run JSON does not store `componentPaths`
- the run JSON does not store per-component outcomes
- the markdown report renders only aggregate scenario/backend tables

Concrete example:

- `packages/benchmark/tmp/current-perf-repo-first-pass/meta-ui-verter-repo_first_pass.json`
  - `p95 = 51.9625ms`
  - `p99 = 657.075ms`
  - `max = 1954.03675ms`
- From the JSON alone, there is no way to know which component produced the `1954ms` max.
- Reconstructing against the current checkout order reveals:
  - `Toast.vue` = `1954.03675ms`
  - `Accordion.vue` = `657.075ms`
  - `FileUpload.vue` = `115.128ms`

That reconstruction depends on replaying local discovery order against the same target SHA.

That is an observability gap, not acceptable benchmark output.

Best architectural fix:

- Persist one named `componentResults[]` row set per repeat and derive aggregates from it.
- Reuse the existing `ComponentResult` direction already present in `packages/benchmark/src/meta-ui-stress.ts` instead of inventing a second anonymous result shape.

## Route-Proof Costs Vs Deeper Costs

### Route-Proof / Identity-Proof Costs

These are the repeated costs that should be counted separately from deep expansion:

- repeated unresolved direct-import proof on owner files
- repeated negative direct-import proof on imported dependency owners
- repeated `current_eval_state_in_view()` entry for the same unresolved import
- repeated `dependency_resolutions_for_eval_in_view()` merges for the same owner
- repeated negative imported-root proof such as `tailwind-variants::ClassValue`

Primary file owners:

- `crates/verter_session/src/host_resolve.rs`
- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/resolver_core/solver_host.rs`

### Deeper Expansion / Solver / Materialization Costs

These should only be tackled after the proof layers are measured cleanly:

- repeated prepared-decl solve over the same already-proven root
- large payload / materialization cost for components like `SelectMenu.vue` and `InputMenu.vue`
- post-solve metadata projection and reportable output size

Primary file owners:

- `crates/verter_session/src/resolver_core/solver_host.rs`
- `crates/verter_session/src/meta_resolve.rs`
- `crates/verter_session/src/component_meta_host.rs`

Current conclusion:

- The sampled traces do not justify a broad deep-expansion rewrite yet.
- The second pass should first eliminate repeated proof churn and fix harness fidelity.

## Exact Files Involved

### Non-Barrel Direct Miss Proof / Imported-Root Follow-Up

- `crates/verter_session/src/host_resolve.rs`
- `crates/verter_session/src/host_manage.rs`
- `crates/verter_session/src/resolver_core/solver_host.rs`
- `crates/verter_session/src/meta_resolve.rs`
- `crates/verter_session/src/host_manage_tests.rs`
- `crates/verter_session/src/meta_resolve_tests.rs`

### Trace Harness Stability / Repeatability

- `packages/benchmark/src/_trace-component.ts`
- `packages/benchmark/src/_test-hang.ts`
- `packages/benchmark/src/meta-ui-stress.ts`
- `packages/benchmark/src/query-timeout.ts`
- `packages/benchmark/src/query-timeout.spec.ts`
- `scripts/benchmark/run-hard-timeout.mjs`

Recommended new repo-owned corpus runner:

- `scripts/benchmark/trace-component-corpus.mjs`

Recommended test file for the hard-timeout wrapper:

- `scripts/benchmark/run-hard-timeout.spec.ts`

### Benchmark JSON / Reporting Observability

- `packages/benchmark/src/meta-ui-bench.ts`
- `packages/benchmark/src/meta-ui-report.ts`
- `packages/benchmark/src/meta-ui-report.spec.ts`
- `packages/benchmark/src/meta-ui-bench.spec.ts`
- `packages/benchmark/src/meta-ui-core.ts`
- `packages/benchmark/README.md`

## Acceptance Criteria

### A. Barrel-Dominated Residuals

Applies to:

- `ChatPrompt.vue`
- `CheckboxGroup.vue`

Disposition:

- Main cutover owns the fix.
- Re-measure after the main cutover lands.

Acceptance:

- No second-pass implementation starts for these until post-cutover traces still show residual cost.
- Post-cutover traces no longer show `src/runtime/types/index.ts` as the dominant miss owner.

### B. Direct Non-Barrel Miss-Proof Memoization

Applies to:

- `CommandPalette.vue`
- `SelectMenu.vue`
- `InputMenu.vue`
- `Select.vue`
- `Checkbox.vue`
- `NavigationMenu.vue`

Disposition:

- Validate after main cutover.
- If still present, handle in the shared non-barrel proof-cache cutover.

Acceptance:

- Direct import proof becomes a host-owned cached fact keyed by owner canonical file hash plus store-view context.
- Within one request, each identical unresolved direct import is proven once.
- Warm identical requests reuse the negative answer instead of re-entering `current_eval_state_in_view()` and `dependency_resolutions_for_eval_in_view()`.
- New coverage lands in `crates/verter_session/src/host_manage_tests.rs` for direct unresolved imports on:
  - owner-local virtual aliases
  - bare package imports
  - imported dependency owners
- `dependency_resolutions_for_eval_in_view()` becomes a projection/read path over cached proof state, not a recompute owner.
- No new fallback resolver path is introduced.

### C. Negative Imported-Root / Solver Retry Churn

Applies to:

- `SelectMenu.vue`
- `InputMenu.vue`
- `Select.vue`
- `NavigationMenu.vue`
- `CommandPalette.vue`
- `ChangelogVersion.vue`

Disposition:

- Validate after main cutover.
- If still present, handle in the shared non-barrel proof-cache cutover.

Acceptance:

- Imported-root proof becomes a host-owned cached fact keyed by canonical provider file hash plus store-view context.
- Negative imported roots such as `tailwind-variants::ClassValue` are cached and reused the same way successful roots are reused.
- `solver_resolve_prepared_type_decl_result source=root_resolve_same hit=false` does not repeat for the same unresolved root in one request.
- `solver_host` reads host-owned root proof state instead of treating fallback-self as repeatable work.
- New regression coverage lands in:
  - `crates/verter_session/src/host_manage_tests.rs`
  - `crates/verter_session/src/meta_resolve_tests.rs`
- No AST/source fallback or widened traversal is added.

### D. Trace Harness Hard Timeout / Repeatability

Disposition:

- Separate second pass now.
- Independent of the main cutover.

Acceptance:

- Corpus tracing runs each component in an isolated child process.
- Timeout enforcement is parent-owned and hard, not `Promise.race` in the query process.
- Result status is structured and explicit:
  - `ok`
  - `query_timeout`
  - `setup_timeout`
  - `close_timeout`
  - `crash`
  - `external_signal`
- Each result records:
  - `wall_ms`
  - `query_ms_from_stdout`
  - `trace_resolve_component_meta_ms`
  - `exit_code`
  - `signal`
  - `stdout_path`
  - `stderr_path`
  - `trace_path`
  - `saw_done_line`
  - `saw_closed_line`
- `ChangelogVersion.vue` no longer lands as an ambiguous `Done ...` + `SIGTERM` artifact.

### E. Benchmark JSON / Report Outlier Visibility

Disposition:

- Separate second pass now.
- Independent of the main cutover.

Acceptance:

- `MetaUiBenchmarkRunRepeat` stores named component result rows and does not rely on anonymous latency arrays as the authoritative persisted shape.
- Each component result row includes:
  - `relativePath`
  - `componentName`
  - `latencyMs`
  - `outcome`
  - per-component deviation summary
- `MetaUiAggregateReport` and markdown output include:
  - top named slow components per backend/scenario
  - any degraded / error / crash components
  - named max / p99 outliers
- A consumer can identify the `repo_first_pass` max outlier from the JSON artifact alone, without replaying component discovery.
- New report coverage lands in:
  - `packages/benchmark/src/meta-ui-report.spec.ts`
  - `packages/benchmark/src/meta-ui-bench.spec.ts`

## Recommended Next Actions

1. Do not fold any of this back into the route-ownership cutover.
2. Land the trace harness hard-timeout / structured-result pass first.
3. Land the benchmark JSON / reporting observability pass second.
4. After the main cutover lands, rerun the hardened trace corpus.
5. If non-barrel residuals remain, land one clean proof-cache cutover across:
   - direct unresolved import proof
   - imported-root negative proof
   - solver consumption of those facts
6. Reclassify residuals:
   - if `ChatPrompt.vue` / `CheckboxGroup.vue` clear, keep them closed with the main cutover
   - if `CommandPalette.vue`, `SelectMenu.vue`, `InputMenu.vue`, `Select.vue`, `Checkbox.vue`, `NavigationMenu.vue`, or `ChangelogVersion.vue` still show non-barrel proof churn, execute the shared non-barrel proof-cache cutover

## Implementation Notes For The Next Agent

- Reuse the existing owner files above; do not introduce a parallel benchmark schema or a second timeout model.
- Prefer replacing the current same-process timeout helpers in `_trace-component.ts` and `meta-ui-stress.ts` with one hard-timeout path rather than preserving both models.
- For benchmark output, reuse the `ComponentResult` idea already present in `packages/benchmark/src/meta-ui-stress.ts` instead of inventing another per-component record shape.
- For non-barrel semantic follow-up, treat direct import proof and imported-root proof as one shared host-owned cache cutover, not as two unrelated optimizations.
- For Rust follow-up work, preserve the repository invariants:
  - one shallow owner
  - one deepening path
  - cache-owned imported state
  - no fallback AST/source walk after the cache-owning pass
