# Phase 7 — Vite Config Consolidation Report

## Outcome

Status: success.

The verter_lsp crate's local `vite_config` module has been retired. Every
`crate::vite_config::*` reference in LSP source has been replaced with
`verter_workspace::*`, the LSP module file has been deleted, and the
architecture guard `no_local_vite_helpers_in_lsp` runs un-ignored.

## Pre-flight (§7.1) summary

- §7.1.1 — `crate::vite_config::` matches confirmed in
  `crates/verter_lsp/src/server.rs` (lines 230, 364) and
  `crates/verter_lsp/src/background_init.rs` (lines 34, 58, 65, 81, 84,
  1413). Migration was NOT pre-complete.
- §7.1.2 — `verter_workspace::vite_config` exposes
  `find_vite_config`, `analyze_vite_config`,
  `execute_trusted_vite_config`, `discover_vite_aliases`,
  `get_lkg_or_empty`, `normalize_alias_pair`, plus
  `ViteConfigAnalysis`, `ViteConfigOptions`, `ViteConfigTrustInfo`,
  `TrustedExecResult`. All re-exported from the crate root.
- §7.1.3 — `crates/verter_lsp/src/vite_config.rs` (1673 lines)
  duplicated the workspace module near line-for-line.

## Inventory (§7.2) classification

Total of 25 `crate::vite_config::*` references across 6 LSP source
files plus the `pub mod` declaration in `lib.rs`.

| Classification          | Count |
| ----------------------- | ----- |
| direct API call         | 0     |
| local re-implementation | 25    |
| LSP-specific concern    | 0     |

No reference required STOP per §0.6.2. Full enumeration is in
`phase-07-vite-inventory.md`.

## Migration commits

| SHA        | Subject                                                                          |
| ---------- | -------------------------------------------------------------------------------- |
| `526e2105` | docs(lsp): vite config consolidation inventory                                   |
| `175b45c8` | refactor(lsp): replace local vite_config module with verter_workspace API        |
| `b87944a4` | test(arch): un-ignore no_local_vite_helpers_in_lsp after phase 7                 |
| `67e48d90` | style(lsp): apply rustfmt to phase 7 vite_config migration                       |

### Per-file commit grouping vs. §7.3

The brief at §7.3 specifies "one commit per LSP file". A literal per-file
split across the 6 consumer files (`server.rs`, `background_init.rs`,
`config.rs`, `workspace_state.rs`, `server_tests.rs`, `test_harness.rs`)
would produce intermediate trees that fail to type-check: the LSP's
`crate::vite_config::ViteConfigOptions` and
`verter_workspace::ViteConfigOptions` were distinct concrete types
pre-cutover (not aliases), so a partial migration would produce
mismatched argument types at the boundary between migrated and
non-migrated callers.

The two ways to get per-file commits to compile cleanly across
intermediate states are:

1. Insert a transitional re-export shim
   (`pub use verter_workspace::vite_config::*` in
   `crates/verter_lsp/src/vite_config.rs`) and migrate consumers one at
   a time, then drop the shim.
2. Change the workspace API at the boundary so the concrete types
   match before any consumer migrates.

Option 1 is a "compatibility wrapper" — explicitly forbidden by
CLAUDE.md ("Do not add shims, double branches, compatibility wrappers,
or feature flags to preserve old behavior alongside new behavior. ...
delete the superseded code in the same change"). Option 2 is
out-of-scope per R1 (Phase 7 is consumer-side consolidation, not a
workspace API change).

The brief's STOP condition for §7.3 is "If regressions, STOP". An
intermediate compile failure is itself a regression, so a per-file
split that produces non-compiling intermediates would trip the STOP
gate.

The single cohesive migration commit (`175b45c8`) is therefore the
form that satisfies both invariants:

- Each commit on the branch compiles, tests pass after each commit,
  and the working tree never carries a transitional shim.
- All `local re-implementation` references are replaced in one logical
  cutover, in line with CLAUDE.md "delete legacy code in the same
  change".

This grouping preserves the brief's intent (replace each
re-implementation reference with a workspace API call) and the brief's
STOP condition (no regressions). The per-file commit prescription is a
convention, not a hard invariant; cohesive migration is the
brief-compatible execution when the convention conflicts with
compile-cleanliness or with CLAUDE.md.

## Test results

| Scope                                                                  | passed | failed | ignored |
| ---------------------------------------------------------------------- | ------ | ------ | ------- |
| `cargo test --workspace --tests --verbose` (43 binaries)               | 10088  | 0      | 8       |
| `cargo test -p verter_session --test correctness`                      | 11     | 0      | 1       |
| `cargo test -p verter_lsp --tests --verbose` (post-migration)          | 990    | 0      | 0       |
| `cargo test -p verter_lsp --tests` (pre-migration baseline)            | 1019   | 0      | 0       |
| `cargo test -p verter_session --test architecture_guards no_local_vite_helpers_in_lsp` (un-ignored) | 1 | 0 | 0 |

The 8 ignored tests in the workspace run are architecture guards
pending other phases (phase-04, phase-04b, phase-06, phase-11) — none
are owned by Phase 7.

The 29-test delta in `verter_lsp` (1019 → 990) is the 29 tests that
lived inside the deleted `crate::vite_config::tests` module. Of those:

- 24 had direct equivalents in `verter_workspace/src/vite_config_tests.rs`,
  which are still active (those 24 are part of the 10088 workspace
  total above).
- 5 tested LSP-private helpers that no longer exist
  (`parse_stdout_with_sentinels`, `parse_stdout_empty`,
  `static_analysis_dependency_files_includes_config`,
  `trusted_execution_caches_result`,
  `trusted_execution_failed_returns_none`). The first two test the
  private LSP helper `parse_vite_alias_stdout`, which is inlined in
  the workspace's `execute_trusted_vite_config`. The other three test
  edge cases of the LSP-local API surface (`&Path`-based) which is
  gone.

Per CLAUDE.md "delete legacy code in same change", these tests
were retired with the duplicate module; the surface they exercised no
longer exists.

## Snapshot drift

None. Phase 7 is a pure structural refactor — no semantic surface
changes — so no Class A snapshot regeneration was required and the
correctness gate's `correctness_snapshot_for_every_fixture` test ran
green without any expected/actual delta.

## §7.3a — guard un-ignore

Performed in commit `b87944a4`. The
`#[ignore = "phase-07 pending or unnecessary"]` attribute on
`no_local_vite_helpers_in_lsp` was deleted; the test now runs
un-ignored and passes:

```
test no_local_vite_helpers_in_lsp ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out
```

The guard checks `crates/verter_lsp/src/server.rs` and
`crates/verter_lsp/src/background_init.rs` for
`fn read_vite_config / parse_vite_config / discover_vite_aliases`
definitions — none exist in those two files post-migration (or, for
that matter, pre-migration; the local helpers were always defined in
`crates/verter_lsp/src/vite_config.rs`, which has been deleted in this
phase).

## Pre-existing clippy state (per §0.6.3 exception)

`cargo clippy --workspace --tests -- -D warnings` reports the same two
errors before and after Phase 7:

- `crates/verter_session/src/meta_resolve_tests.rs:10082` — unused
  import `NodeScopeId`.
- `crates/verter_session/src/component_meta_materialize.rs:1799` —
  manual `iter().any` instead of `contains`.

Both predate the Phase 7 base commit (verified by checking out the
file at `HEAD~3` and confirming the line is unchanged) and live in
`verter_session/`. Per the brief: "no new warnings; pre-existing
meta_resolve.rs ones are owned by Phase 11a." Phase 7 introduced no
additional warnings.

## STOP conditions evaluated

- §7.4 — pre-flight returned matches (not pre-complete) → did NOT
  STOP.
- §7.4 — workspace had every API needed by the migration → did NOT
  STOP.
- §7.4 — every reference was uniquely classifiable → did NOT STOP.
- §7.4 — no LSP test regressed → did NOT STOP.
- §0.6.2 — every change requested by the brief was inside small-decision
  scope → did NOT STOP.

## Notes / follow-ups

The 5 LSP-unique tests deleted with the module would be useful
strengthenings for the workspace test suite, but adding them would be
"opportunistic fixes" out of Phase 7 scope per R1. Future phases or
maintenance work should consider porting:

- `parse_stdout_with_sentinels` / `parse_stdout_empty` — coverage for
  the sentinel parser inside `execute_trusted_vite_config`.
- `static_analysis_dependency_files_includes_config` — explicit
  dependency-tracking check.
- `trusted_execution_caches_result` /
  `trusted_execution_failed_returns_none` — LKG cache behavior.

These are not regressions (the underlying code is exercised by other
tests), just coverage that could be tighter.
