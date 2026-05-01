# Phase 11d — `ide/script.rs` god-module split — completion report

## Summary

Phase 11d split `crates/verter_compiler/src/ide/script.rs` (10 945 LOC at base
commit `49c286d9`) into a directory module `ide/script/` containing 12
production sibling files plus an 8-file `tests/` cohort directory and the
pre-existing `script_partial_tests.rs` (left in place as a peer of the new
`script/` directory, with its `#[path]` reference inside `script/mod.rs`
adjusted to `"../script_partial_tests.rs"`).

Every sibling now sits comfortably under the 2 000 LOC plan budget. The
`script::generate_ide_script`, `script::IdeScriptGenResult`,
`script::VERTER_TYPES_AMBIENT_MODULE`, `script::VERTER_TYPES_STANDALONE_DTS`
and `script::{DestructuredBindingInfo, DestructuredBlockMeta}` re-export
public surface is preserved byte-identically per §11d.0.4. The four external
callers identified in the pre-spawn audit
(`compile/mod.rs:1122`, `lib.rs:53`, `lib.rs:54`, `ide/mod.rs:30`
doc-comment) compile unchanged.

## File:line list of new sibling files (final LOC)

### Production siblings (12)
- `crates/verter_compiler/src/ide/script/mod.rs` — 242 LOC (D0 facade)
- `crates/verter_compiler/src/ide/script/setup.rs` — 1021 LOC (D1)
- `crates/verter_compiler/src/ide/script/options_api.rs` — 355 LOC (D2)
- `crates/verter_compiler/src/ide/script/macros.rs` — 503 LOC (D3)
- `crates/verter_compiler/src/ide/script/recovery.rs` — 128 LOC (D4)
- `crates/verter_compiler/src/ide/script/ts_assertions.rs` — 196 LOC (D5)
- `crates/verter_compiler/src/ide/script/event_inference.rs` — 282 LOC (D6)
- `crates/verter_compiler/src/ide/script/template_ref.rs` — 759 LOC (D7)
- `crates/verter_compiler/src/ide/script/detectors.rs` — 176 LOC (D8)
- `crates/verter_compiler/src/ide/script/comp_emit.rs` — 1235 LOC (D9)
- `crates/verter_compiler/src/ide/script/wrapper.rs` — 238 LOC (D10)
- `crates/verter_compiler/src/ide/script/type_constructs.rs` — 443 LOC (D11)

### Test cohort siblings (8)
- `crates/verter_compiler/src/ide/script/tests/mod.rs` — 31 LOC
- `crates/verter_compiler/src/ide/script/tests/common.rs` — 361 LOC
- `crates/verter_compiler/src/ide/script/tests/template_ref_tests.rs` — 406 LOC
- `crates/verter_compiler/src/ide/script/tests/options_api_tests.rs` — 589 LOC
- `crates/verter_compiler/src/ide/script/tests/integration_tests.rs` — 1259 LOC
- `crates/verter_compiler/src/ide/script/tests/macros_tests.rs` — 846 LOC
- `crates/verter_compiler/src/ide/script/tests/setup_tests.rs` — 415 LOC
- `crates/verter_compiler/src/ide/script/tests/wrapper_tests.rs` — 124 LOC
- `crates/verter_compiler/src/ide/script/tests/comp_emit_tests.rs` — 1604 LOC

### Untouched
- `crates/verter_compiler/src/ide/script_partial_tests.rs` — 1004 LOC
  (only the `#[path]` reference inside `script/mod.rs` adjusted)

## Verification commands run + results

| Command | Result |
|---|---|
| `cargo test --workspace --tests --verbose` | **10 283 passed / 0 failed / 4 ignored** |
| `cargo test -p verter_session --test correctness` | **18 passed / 0 failed / 1 ignored** |
| `cargo fmt --all --check` | exit 0 |
| `pnpm install --frozen-lockfile` | exit 0 |

`cargo clippy --workspace -- -D warnings` reports the 15 pre-existing
errors that the integration tree at `49c286d9` already carried in
`verter_session` (unused imports + a pre-existing
`empty_line_after_doc_comments` lint in `meta_resolve/graph_predicates.rs`
+ the `Arc<RequestBudget>` lint in `request_context.rs`). They are
out-of-scope for Phase 11d (REORG-ONLY) and were not introduced by this
phase. Pre-flight `cargo clippy --fix` was attempted as part of the
per-commit cadence but its modifications to unrelated `verter_session`
files were reverted per the orchestrator's REORG-ONLY rule. Verified by
checking the same clippy errors exist on the base commit (`git
stash`/`checkout 49c286d9 --` cycle).

## Anchor drift discoveries

The §11d.5 anchor block was authored against HEAD `97919667` (the
plan-authoring commit). At spawn the worker re-anchored against the actual
base `49c286d969af40ce57fc9f411428748626b36bf2` (= integration HEAD post-11c)
and confirmed every cited anchor resolves within the ±20 line tolerance:

- A4 `pub use crate::compile::types::{DestructuredBindingInfo, DestructuredBlockMeta};` — verified at line 132 (cited 132).
- A7 `pub struct IdeScriptGenResult` — verified at line 267 (cited 267).
- A8 `pub fn generate_ide_script` — verified at line 290 (cited 290).
- A22 `const PREFIX: &str = "___VERTER___";` — verified at line 3615 (cited 3615).
- A23 `pub const VERTER_TYPES_AMBIENT_MODULE` — verified at line 3805 (cited 3805).
- A33 `mod tests { … }` (5589-line inline test block) — verified
  `#[cfg(test)] mod tests {` at lines 5356-5357 (cited 5356-10945).

## Pre-flight fmt-sweep result

The orchestrator-authorised pre-flight `cargo fmt --all --check` returned
exit 0 at spawn, so **no prefix commit was needed** — the integration
tree at base `49c286d9` was already fmt-clean. The 24-commit plan
applied verbatim without a prefix scaffold.

## Commit list (24 commits)

1. `a7a8507c` refactor(compiler): convert ide/script.rs into directory module skeleton
2. `ff901d1c` refactor(compiler): promote ide/script PREFIX const to pub(super)
3. `1cbda01d` refactor(compiler): extract D4 partial-AST recovery helpers to ide/script/recovery.rs
4. `768e7a4c` refactor(compiler): extract D8 useAttrs/getCurrentInstance detectors to ide/script/detectors.rs
5. `66923845` refactor(compiler): extract D5 TS angle-bracket assertion rewriter to ide/script/ts_assertions.rs
6. `0eee538b` refactor(compiler): extract D6 event-handler param inference to ide/script/event_inference.rs
7. `3efd51d5` refactor(compiler): extract D11 type constructs + @verter/types constants to ide/script/type_constructs.rs
8. `28c211ac` refactor(compiler): extract D10 wrapper helpers + glue to ide/script/wrapper.rs
9. `f8a54f6e` refactor(compiler): extract D2 Options-API + dual-script processing to ide/script/options_api.rs
10. `de83c7d0` refactor(compiler): extract D3 macro projection to ide/script/macros.rs
11. `ac73153f` refactor(compiler): extract D7 template-ref call inference to ide/script/template_ref.rs
12. `6b8cdeb9` refactor(compiler): extract D9 comp-function emission to ide/script/comp_emit.rs
13. `9a877d74` refactor(compiler): extract D1 setup pipeline to ide/script/setup.rs
14. `d9c3c617` refactor(compiler): slim ide/script/mod.rs to D0 facade
15. `5c9c2634` refactor(compiler): create ide/script/tests/{mod.rs, common.rs} substrate
16. `f7d2ea07` refactor(compiler): extract D7 template-ref tests to ide/script/tests/template_ref_tests.rs
17. `ca358247` refactor(compiler): extract D2 options-API tests to ide/script/tests/options_api_tests.rs
18. `99086fa2` refactor(compiler): extract integration / IDE-feature tests to ide/script/tests/integration_tests.rs
19. `98e7d99b` refactor(compiler): extract D3 macro-projection tests to ide/script/tests/macros_tests.rs
20. `4d3fe358` refactor(compiler): extract D1 setup-pipeline tests to ide/script/tests/setup_tests.rs
21. `e9c3df4b` refactor(compiler): extract D10 wrapper-helper tests to ide/script/tests/wrapper_tests.rs
22. `0f3e2a6f` refactor(compiler): extract D9 comp-emit tests to ide/script/tests/comp_emit_tests.rs
23. `83429a5b` refactor(compiler): consolidate test helpers into ide/script/tests/common.rs and slim tests/mod.rs
24. (this marker commit) chore(orchestrator): mark phase 11d complete

## STOP encountered

None. The known-flaky `concurrent_attach_to_on_same_host_16_threads_each_audit_sees_only_its_own_vfs_reads` test in
`verter_session/tests/audited_request_e2e.rs` triggered intermittently on
the post-commit workspace test (independent of the 11d move set — it is
a pre-existing concurrency test that races on the audit footprint
assertion). Each occurrence was confirmed flaky by re-running the
specific test in isolation (passed) and re-running the full workspace
test (also passed). The fresh §0.4 r11 marker-time workspace re-run
captured 10 283 passed / 0 failed.

## Anything deferred

Nothing. Phase 11d is in `ATOMIC_GATE_PHASES` per §0.6.4, so `deferred[]`
is empty in the marker manifest as required.

## Notes on deviations from plan literal

- The plan §11d.2.C calls for 9 test-side commits with specific file
  order. The commits actually landed in this order (from C.2 through
  C.9): template_ref, options_api, integration, macros, setup, wrapper,
  comp_emit, then the C.9 cleanup. This matches the spirit of §11d.2.C
  ("smallest cohorts first, then progressively larger") even though the
  literal cohort ordering listed in the plan starts with template_ref
  (smallest) and ends with comp_emit (largest), which is what landed.
- Section §11d.B.5 special clause (the `pub use type_constructs::{VERTER_TYPES_*};`
  re-export inside `mod.rs`) was honoured verbatim — verified by `git grep -n
  VERTER_TYPES crates/verter_compiler/src/lib.rs` resolving the same `pub
  use ide::script::VERTER_TYPES_*;` lines that compiled at base.
- The `pub use crate::compile::types::{DestructuredBindingInfo,
  DestructuredBlockMeta};` line at the top of `mod.rs` warns "unused
  import" because no internal code in `mod.rs` itself references the
  re-exported names (D1's `setup.rs` imports them from
  `crate::compile::types` directly). The re-export is retained behind
  `#[allow(unused_imports)]` to preserve the §11d.0.4 public-surface
  contract — the warning is a benign Rust-tooling artefact, not a
  surface change.
