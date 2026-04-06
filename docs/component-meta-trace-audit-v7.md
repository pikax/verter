# Component-Meta Trace Audit V7

Date: 2026-04-05

Artifacts:
- `/Users/carlosrodrigues/Documents/dev/verter/tmp/corpus-trace-audit-v7/full/summary.json`
- `/Users/carlosrodrigues/Documents/dev/verter/tmp/corpus-trace-audit-v7/full/traces/`
- `/Users/carlosrodrigues/Documents/dev/verter/tmp/corpus-trace-audit-v7/targeted/ChangelogVersion.trace.log`

## What Was Fixed

- Scheduler preloads no longer read raw extensionless canonical ids such as:
  - `src/runtime/types/html`
  - `src/runtime/types`
  - `src/runtime/composables/useComponentIcons`
- The scheduler now normalizes macro-type blocker ids and relative dependency ids through the same companion-canonicalization rules used by the host query path.
- Regression coverage was added in:
  - `/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/host_manage_tests.rs`
  - `upsert_normalizes_extensionless_macro_type_blockers_before_scheduler_workspace_read`

## Corpus Result

- Full traced corpus run: 176/177 completed in the sweep artifact.
- The one failed sweep entry was `src/runtime/components/ChangelogVersion.vue`.
- Isolated rerun of `ChangelogVersion.vue` completed successfully:
  - `Done in 96ms (13 props)`
- Practical status: all 177 benchmark components are traceable on the current build.

## Current State

- Raw extensionless VFS reads in the benchmark trace corpus: `0`
- Files with raw extensionless reads: `0`
- The remaining issue is not file-widening through raw loads.
- The remaining issue is shallow miss churn on `src/runtime/types/index.ts`.

Observed corpus signal:
- `3055` `cached_dependency_resolution_in_view_result ... source=miss` events
- `91` traced components hit that pattern
- Common shape:
  - owner: `.../src/runtime/types/index.ts`
  - import: `../components/<Sibling>.vue`
  - source: `miss`
  - target: `<none>`

## Interpretation

- This is a shallow route-selection inefficiency, not the old deepening bug.
- The engine is no longer reopening missing extensionless paths.
- The engine is also no longer loading large sets of unrelated files just to normalize those paths.
- The barrel still pays one shallow miss per wildcard sibling before it proves which route matters.

## Follow-Up Target

The next architectural pass should make wildcard reexport routes first-class cached data.

Desired end state:
- `export *` sources participate in the same dependency-route cache as imports
- repeated barrel symbol routing does not emit per-sibling miss churn
- route selection stays shallow and does not need repeated same-file `current_eval_state` hits while scanning the barrel

Likely touchpoints:
- `/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/host_resolve.rs`
- `/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/host_manage.rs`
- `/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/resolver_core/external_type_frontier.rs`
- `/Users/carlosrodrigues/Documents/dev/verter/crates/verter_session/src/resolver_core/export_graph.rs`

Non-goal for that pass:
- do not add another eager collector or alternate resolver path
- keep the fix within the existing shallow-first, one-deepening-path ownership model
