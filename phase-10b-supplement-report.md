# Phase 10b Supplement — `god_module_size_budget` body rewrite

## Summary

Phase 10b under-delivered: it added the new `no_unbounded_recursion_in_resolver_core` guard but did NOT rewrite the `god_module_size_budget` body. The original early r15/F6 form (a full `crates/verter_session/src` walkdir with an empty allow-list) would fail when un-ignored on out-of-scope files such as the in-progress Phase 11x split shells. The §10.2 r15 spec scopes the guard to the five Phase 11 god-module target roots — `meta_resolve`, `resolver_core/component_meta_query_engine`, `host_manage`, `verter_compiler/src/ide/script`, `verter_lsp/src/server` — walks each as either a single file or post-split directory module, and excludes test fixtures (`*_tests.rs`, `tests.rs`, anything under a `tests/` directory). This supplement lands the missing rewrite verbatim from the plan body (§10.2 r15 / lines 6691-6793) as a single mechanical commit. The `#[ignore = "phase-11 pending"]` attribute is preserved unchanged — the 11e-finalizer phase flips it after this supplement integrates. No other test, no other guard, and no other file is modified.

## File touched

- `crates/verter_session/tests/architecture_guards.rs` lines 136-241 — `fn god_module_size_budget` body replaced verbatim from §10.2 r15 spec; cargo fmt applied.

## Verification

- `cargo test --workspace --tests --verbose` — **10283 passed, 0 failed, 4 ignored across 45 blocks** (matches post-11e-split baseline; saved to `/tmp/p10b-supplement-workspace.txt`).
- `cargo test -p verter_session --test architecture_guards god_module_size_budget -- --ignored --nocapture` — **1 passed, 0 failed** (the new body succeeds against the current tree; saved to `/tmp/p10b-supplement-ignored.txt`). All five Phase 11 target roots (`meta_resolve`, `component_meta_query_engine`, `host_manage`, `ide/script`, `lsp/server`) exist post-split as directory modules and every production .rs file under each is ≤ 4000 LOC; test fixtures are excluded by the new `is_test_fixture` predicate.
- `cargo test -p verter_session --test correctness` — **18 passed, 0 failed, 1 ignored** (no snapshot drift).
- `cargo fmt --all --check` — clean.
- `pnpm install --frozen-lockfile` — clean.
- `cargo clippy --workspace -- -D warnings` — fails with 51 pre-existing errors in verter_session crate (unrelated unused imports / arc_with_non_send_sync), confirmed pre-existing at base commit `b33e7f32` via stash test. No clippy error references `architecture_guards.rs` or `god_module_size_budget`. Per R1, opportunistic clippy fixes are out-of-scope for this surgical mini-phase.

## Confirmation

`god_module_size_budget` PASSES with `--ignored` post-rewrite. The guard is now ready for the 11e-finalizer phase to flip `#[ignore]` to active.
