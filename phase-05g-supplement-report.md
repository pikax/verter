# Phase 5g-supplement worker report

Phase: `phase-05g-supplement` — instrumentation surface for §5.D tests
+ §5.D backfill for already-integrated 5b/5e/5f.

Branch: `wt/phase-05g-supplement-instrumentation-and-backfill`
Base commit: `3147c02f44ed4fc3fdc1a50d6f51929c7a4a0c18` (`chore(orchestrator): mark phase 05f complete`)
Work head before marker: `5775c649c1c09ba0fdaa7577be3f5a39835ad76d` (`style(meta): cargo fmt 5g-supplement instrumentation surface`)

## §5g-supplement.0a Pre-flight classification

All accessors are either `exposure` (small change, ≤50 LOC) or
`hook installation` ≤300 LOC. Pre-flight passes; commit groups proceed.

| §5.D.0 r17 accessor                                  | Class                | LOC    | Notes |
|------------------------------------------------------|----------------------|--------|-------|
| `audit().loaded_files()`                              | exposure             | ~30    | New `HostTestAuditState` populated from production read/shallow hooks; cumulative across requests |
| `audit().total_reads()`                              | exposure             | ~30    | Same `HostTestAuditState` (atomic counter) |
| `audit().total_shallow_processes()`                  | exposure             | ~30    | `IndexedReadyDb::insert` fresh-insert hook |
| `audit().total_lowerings()`                          | exposure             | ~10    | Reads existing `SemanticGraphStore::stats_snapshot().decl_subexpression_lowering_count` |
| `dispatch_counter().family_cold` / `family_warm`     | hook installation    | ~80    | Cache-peek inside `SemanticGraphStore::execute_cooperative` + canonicalisation in `SemanticQueryKeyDigest::from_key` |
| `dispatch_trace_for(&key)` → `path_decomposition()`   | exposure             | ~80    | Reads warm cache prefixes for `ProjectPath` keys |
| `HostConfig::depth_budget`                           | exposure             | ~5     | New constructor field; default `MAX_DEPTH` |

Total LOC for `hook installation`: ~80 (well under the 300 LOC STOP threshold).

## Commits

### §5g-supplement.1.A Instrumentation surface (4 commits)

| Commit  | SHA      | Summary |
|---------|----------|---------|
| A1      | `731d17a8` | `test(audit): expose loaded_files / total_reads / total_shallow_processes / total_lowerings on AuditFootprint (cfg(test))` — adds `host_test_audit.rs` with `HostTestAuditState` + `HostTestAudit` view; `IndexedReadyDb::install_test_audit_hook` populates from fresh inserts; `read_analysis_source` records on the cold ensure-loaded path; `total_lowerings` reads from `SemanticGraphStore::stats_snapshot()`. |
| A2      | `ba916e6e` | `test(dispatch): add family_cold / family_warm / dispatch_trace_for accessors (cfg(test))` — splits `DISPATCH_KEY_COUNTS` into cold/warm thread-locals; `execute_cooperative` records cold/warm based on cache peek; `DispatchCounter` exposes `family_cold(&key)` / `family_warm(&key)`; `DispatchTrace` exposes `path_decomposition()` / `SubKey::mode()`. |
| A3      | `cdbc81b2` | `test(host): expose HostConfig::depth_budget for hermetic harness construction` — new `depth_budget` field (default `MAX_DEPTH`); `PathWalker::advance_step` honours the cap and returns `Recursive` sentinel on over-budget hops; `build_project_path` surfaces it as `QueryResult::Recursive` so it is NOT cached as a Value. |
| A3.5    | `e3a0c924` | `test(dispatch): expose VerterHost::semantic_dispatch() accessor (cfg(test))` — small follow-up adding the public test-only accessor that the §5.D backfill tests use. |

### §5g-supplement.1.B §5.D backfill for 5b/5e/5f (13 commits)

| Test name                                                                                  | Commit | SHA      | Owning phase | §5.D | Tests added |
|--------------------------------------------------------------------------------------------|--------|----------|--------------|------|-------------|
| `cache_discipline_resolve_macro_payload_repeated_keys_warm`                                | B1     | `87c43888` | 5b           | §5.D.1 | 1 |
| `cache_discipline_materialize_surface_repeated_keys_warm`                                  | B2     | `684ec135` | 5b           | §5.D.1 | 1 |
| `cache_discipline_execute_pick_repeated_keys_warm`                                         | B3     | `3a1e7489` | 5b           | §5.D.1 | 1 |
| `cache_discipline_execute_omit_repeated_keys_warm`                                         | B4     | `e2e28218` | 5b           | §5.D.1 | 1 |
| `cache_discipline_execute_to_type_expr_repeated_keys_warm`                                 | B5     | `4d311fa9` | 5b           | §5.D.1 | 1 |
| `read_once_shallow_first_lazy_for_resolve_macro_payload`                                   | B6     | `a675ab0a` | 5b           | §5.D.2 | 1 |
| `no_cache_promotion_for_budget_exceeded_resolve_macro_payload`                             | B7     | `56953be7` | 5b           | §5.D.4 | 1 |
| `read_once_shallow_first_lazy_for_route_target_pick_omit`                                  | B8     | `d683817f` | 5e           | §5.D.2 | 1 |
| `intermediate_hops_navigate_terminal_only_expanded_for_route_target_pick_omit`             | B9     | `58f643d7` | 5e           | §5.D.3 | 1 |
| `no_cache_promotion_for_budget_exceeded_route_target_pick_omit`                            | B10    | `77b9ffca` | 5e           | §5.D.4 | 1 |
| `read_once_shallow_first_lazy_for_fallthrough_inheritance`                                 | B11    | `bb2948a7` | 5f           | §5.D.2 | 1 |
| `intermediate_hops_navigate_terminal_only_expanded_for_fallthrough_inheritance`            | B12    | `610694c8` | 5f           | §5.D.3 | 1 |
| `no_cache_promotion_for_budget_exceeded_fallthrough_inheritance`                           | B13    | `77884775` | 5f           | §5.D.4 | 1 |

Test count by category: 5×§5.D.1 + 3×§5.D.2 + 2×§5.D.3 + 3×§5.D.4 = 13 tests.

### Style cleanup

| Commit  | SHA      | Summary |
|---------|----------|---------|
| fmt     | `5775c649` | `style(meta): cargo fmt 5g-supplement instrumentation surface` — re-formats files touched by A1/A2/A3/B1-B13. |

## Test counts (from `/tmp/p05g-supplement-workspace.txt`)

```
cargo test --workspace --tests --verbose
passed: 10224  failed: 0  ignored: 10  blocks: 44
```

```
cargo test -p verter_session --test correctness
passed: 11  failed: 0  ignored: 1
```

## Verification (§0.6.3)

- `cargo test --workspace --tests --verbose` — 10224 pass, 0 fail.
- `cargo clippy --workspace --tests -- -D warnings` — clean.
- `cargo fmt --all --check` — clean.
- `pnpm install --frozen-lockfile` — no drift.
- `cargo test -p verter_session --test correctness` — 11 pass, 0 fail.

## Files added

- `crates/verter_session/src/host_test_audit.rs` (new test-only module)
- `crates/verter_session/src/component_meta_cache_discipline_tests.rs` (5 tests)
- `crates/verter_session/src/component_meta_read_once_tests.rs` (3 tests)
- `crates/verter_session/src/component_meta_terminal_mode_tests.rs` (2 tests)
- `crates/verter_session/src/component_meta_no_cache_promotion_tests.rs` (3 tests)

## Files modified (production code, all under `#[cfg(test)]` bare gating)

- `crates/verter_session/src/lib.rs` — `audit()`, `dispatch_counter()`, `dispatch_trace_for()`, `semantic_dispatch()` test-only accessors + `test_audit` field + module declarations.
- `crates/verter_session/src/types.rs` — `HostConfig::depth_budget` field + Default.
- `crates/verter_session/src/host_manage.rs` — `read_analysis_source` records on the cold path.
- `crates/verter_session/src/project_type_store.rs` — `IndexedReadyDb::test_audit_hook` field + `install_test_audit_hook` setter; `insert` fires the hook.
- `crates/verter_session/src/semantic_query_memo.rs` — `execute_cooperative` records cold/warm split before the retry loop.
- `crates/verter_session/src/project_semantic_dispatch/raise.rs` — `DISPATCH_KEY_COLD_COUNTS` / `DISPATCH_KEY_WARM_COUNTS` thread-locals + canonicalising digest function.
- `crates/verter_session/src/project_semantic_dispatch/walk.rs` — `PathWalker::advance_step` honours `depth_budget` (returns `Opaque(QueryError::RecursiveRef)` on over-budget hops).
- `crates/verter_session/src/project_semantic_dispatch/build.rs` — `build_project_path` surfaces over-budget terminals as `QueryResult::Recursive` so they are NOT warm-cached as Values.

## Deferred items

None. Per §5g-supplement.2 STOP CONDITIONS, this is an atomic-gate
phase (listed in §0.3 ATOMIC_GATE_PHASES allowlist; 5l gates on
`phase-05g-supplement-complete`), so `status: "success"` AND
`deferred[]: []` are required (post-r17). Both are met.

## §0.6.5 stack-depth discipline

The new `PathWalker::advance_step` budget check is consistent with the
already-iterative walker; the budget is a discrimination handle for
the §5.D.4 contract, not a stack-safety rail. Recursive Rust calls
are not added; the walker remains iterative on the heap. No new
unbounded recursion is introduced in resolver / dispatch / walker
code.

## §0.6.7 one-read / shallow-walk / lazy-expand

The new instrumentation does NOT add additional reads or shallow
walks. It only OBSERVES existing read sites
(`read_analysis_source`'s cold path), the existing `IndexedReadyDb`
fresh-insert path, and the existing `SemanticGraphStore::stats_snapshot()`
counter for lowerings. The §5.D.2 read-once tests assert this
contract explicitly (deltas are 0 on the second identical query).

## §0.6.8 type-system enforcement

The instrumentation surface is gated by bare `#[cfg(test)]` per
r17/N12 — production NAPI/WASM/LSP builds compile WITHOUT the
accessors. There is no `feature = "test-instrumentation"` flag, no
runtime-only mutator, and no public production surface added. Type-
system enforcement is satisfied by the cfg gate itself.
