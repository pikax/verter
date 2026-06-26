# Post-cutover review-fixes — report

## Summary

Five bundled fixes addressing HIGH and MEDIUM findings from the harsh
cutover review. The Gap 2 clippy cleanup already landed at base
`8345b6f92c51ac265cfcaf084dd4b0f6f916f7a5`; this phase covers the
remaining HIGH/MEDIUM findings:

- **Fix A (HIGH)** — three Phase 5d Class A architecture guards became
  non-discriminating after Phase 11a/d split `meta_resolve.rs` and
  `host_manage.rs` into folder modules. Rewrote each to walk the
  full module surface via `walkdir`.
- **Fix B (MEDIUM)** — Phase 9.2 MCP↔LSP dependency direction
  inventory was deferred and never picked up. Authored the
  inventory document.
- **Fix C (MEDIUM)** — CLAUDE.md "Macro Type Traversal Rule" listed
  4 `ProjectionMode` variants; the `semantic_query.rs` enum has 5.
  Updated to include `Skeleton`.
- **Fix D (MEDIUM)** — two `TODO(phase-5g)` markers in
  `meta_resolve/materialize/macro_shapes.rs` and
  `meta_resolve/materialize/field_types.rs` referenced engine
  retirement that completed in Phase 5l + 5m. Rewrote both as
  past-tense bridge documentation.
- **Fix E (MEDIUM)** — long commentary blocks in
  `architecture_guards.rs` carried the Phase 5d→5g migration narrative
  with stale "deferred to 5g" / "TODO(phase-5g)" references. Marked
  the narrative as historical context while preserving its content
  for archeology.

Pre-flight `cargo fmt --all --check` produced no drift, so no prefix
commit was needed.

## Per-fix change log

### Fix D — TODO(phase-5g) → past-tense bridge documentation

Files touched:

- `crates/verter_session/src/meta_resolve/materialize/macro_shapes.rs:1631`
  — replaced the 7-line `TODO(phase-5g)` comment with a 7-line
  post-cutover note describing the bridge approach and why ctx
  threading is load-bearing for `Partial<T>` optionality propagation.
- `crates/verter_session/src/meta_resolve/materialize/field_types.rs:1504`
  — replaced the 9-line `TODO(phase-5g)` comment with a 9-line
  past-tense note describing the §5.13a.2 bridge migration as
  complete and naming the semantic target.

No code lines changed. Commit: `d7157606`.

### Fix A — Phase 5d Class A guards walk the module surface

File touched:

- `crates/verter_session/tests/architecture_guards.rs:272-650`
  — rewrote three `#[test]` functions:
  - `phase_05d_4a_class_a_props_callers_migrated_in_meta_resolve`
  - `phase_05d_4a_class_a_props_callers_migrated_in_host_manage`
  - `phase_05d_4b_class_a_slots_callers_migrated`
  
  Each now collects every `.rs` file under the named module surface
  (shell + folder children, excluding `*_tests.rs`) via two new
  helper functions `collect_meta_resolve_module_surface()` and
  `collect_host_manage_module_surface()`, then asserts ZERO Class A
  (and for `host_manage` also ZERO Class B) engine-method callsites
  in any of them.
  
  The fourth Phase 5d guard
  `phase_05d_4a_class_a_props_callers_migrated_in_type_expansion_verter`
  reads `type_expansion_verter.rs` which was NOT split — left
  untouched per brief.

Commit: `b8bdfa6c`.

#### Discrimination proof for Fix A

Synthetic regression run #1 — meta_resolve module surface:

```
Inserted "// .project_expr_surface_expr(" at the top of
crates/verter_session/src/meta_resolve/scoring.rs.
```

Test command and observed FAIL output (excerpts from
`/tmp/p-fix-a-discrim.txt`):

```
running 4 tests
test phase_05d_4a_class_a_props_callers_migrated_in_type_expansion_verter ... ok
test phase_05d_4a_class_a_props_callers_migrated_in_meta_resolve ... FAILED
test phase_05d_4b_class_a_slots_callers_migrated ... FAILED
test phase_05d_4a_class_a_props_callers_migrated_in_host_manage ... ok

---- phase_05d_4a_class_a_props_callers_migrated_in_meta_resolve stdout ----
panicked at crates\verter_session\tests\architecture_guards.rs:334:5:
Phase 5d 4a / Phase 5l final state: meta_resolve module surface must
have ZERO Class A engine-method callsites. Violations:
crates/verter_session/src/meta_resolve/scoring.rs: 1 Class A
engine-method callsite(s) found ...

---- phase_05d_4b_class_a_slots_callers_migrated stdout ----
panicked at crates\verter_session\tests\architecture_guards.rs:572:5:
Phase 5d 4b / Phase 5l final state: meta_resolve module surface must
have ZERO Class A engine-method callsites (slots and multi-macro-kind
sites included). Violations:
crates/verter_session/src/meta_resolve/scoring.rs: 1 Class A
engine-method callsite(s) found ...

test result: FAILED. 2 passed; 2 failed; 0 ignored ...
```

Synthetic regression run #2 — host_manage module surface:

```
Inserted "// .project_type_surface_expr(" at the top of
crates/verter_session/src/host_manage/component_meta_methods.rs.
```

Observed FAIL output (excerpts from
`/tmp/p-fix-a-discrim2.txt`):

```
running 1 test
test phase_05d_4a_class_a_props_callers_migrated_in_host_manage ... FAILED

---- phase_05d_4a_class_a_props_callers_migrated_in_host_manage stdout ----
panicked at crates\verter_session\tests\architecture_guards.rs:486:5:
Phase 5d 4a / Phase 5l + 5m final state: host_manage module surface
must have ZERO Class A and ZERO Class B engine-method callsites.
Violations:
crates/verter_session/src/host_manage/component_meta_methods.rs: 1
Class B engine-method callsite(s) found ...

test result: FAILED. 0 passed; 1 failed; 0 ignored ...
```

Both inserts were reverted; `git diff` returned clean. Rerunning the
tests post-revert returned `4 passed; 0 failed; 0 ignored ...` so the
guards are correctly anchored to the absence of engine-method calls
in production siblings.

### Fix E — pre-Phase-11 guard commentary marked historical

File touched:

- `crates/verter_session/tests/architecture_guards.rs:242-264` —
  expanded the section header above the Phase 5d guards to a two-part
  shape: a "POST-CUTOVER NOTE (final state)" paragraph followed by a
  preserved "HISTORICAL CONTEXT" block.
- `crates/verter_session/tests/architecture_guards.rs:601-695` —
  rewrote the body commentary inside
  `phase_05m_class_b_callers_migrated_through_bridge_helpers` with
  the same shape: post-cutover note first, then preserved historical
  narrative covering the dispatch-vs-prepared-decl-fallback finding.
  Notes that the in-source `TODO(phase-5g)` markers were converted
  to past-tense documentation in Fix D.

No production code changes. Commit: `b5109cf1`.

### Fix C — CLAUDE.md ProjectionMode list updated

File touched:

- `CLAUDE.md:103` — updated the "Macro Type Traversal Rule" paragraph
  to list five query modes (`Identity`, `Navigate`, `Shallow`,
  `Expanded`, `Skeleton`) and added a one-sentence note describing
  `Skeleton`'s role: BFS / generic-helper traversal where unbound
  type parameters become `TypeParam` shells so Conditional branches
  do not collapse to `never` for unbound generics.

Commit: `e926effa`.

### Fix B — phase-09-mcp-direction.md inventory

New file:

- `phase-09-mcp-direction.md` (worktree root, 150 lines).

Greps run, both at the §9.2 template paths (adapted for post-Phase-11e
where `server.rs` became `server/mod.rs`) and broadened to the entire
LSP `src/`:

```
grep -n "verter_mcp\|use verter_mcp" \
    crates/verter_lsp/Cargo.toml \
    crates/verter_lsp/src/lib.rs \
    crates/verter_lsp/src/server/mod.rs

grep -n "verter_lsp\|use verter_lsp" \
    crates/verter_mcp/Cargo.toml \
    crates/verter_mcp/src/lib.rs
```

Findings recorded in the document:

- LSP → MCP edges total: 1 Cargo dep + 3 `use`/path-call sites.
- MCP → LSP edges: 0.
- The LSP **library** does not depend on `verter_mcp`; only the LSP
  **binary** does, inside `serve_mcp_http()`.
- Direction is strictly one-way (LSP→MCP).

Cross-crate dependency table:

| File:line | Edge | Symbol |
| --------- | ---- | ------ |
| `crates/verter_lsp/Cargo.toml:25` | LSP→MCP (Cargo) | `verter_mcp = { path = "../verter_mcp" }` |
| `crates/verter_lsp/src/main.rs:464` | LSP→MCP (`use`) | `verter_mcp::McpServerConfig` |
| `crates/verter_lsp/src/main.rs:464` | LSP→MCP (`use`) | `verter_mcp::VerterMcpServer` |
| `crates/verter_lsp/src/main.rs:466` | LSP→MCP (path call) | `verter_mcp::tools::diagnostics::make_lint_config` |

No architectural finding requires action. The §9.2 brief flags this
as informational.

Commit: `7f63581a`.

## Verification command output

Workspace tests (full):

```
$ cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p-review-fixes-marker.txt
```

Aggregated counts (sum across all 45 test binaries):

```
passed=10297, failed=0, ignored=2, filtered=0 (45 test binaries)
```

Matches the brief's expected `10297/0/2/45` (passed/failed/ignored/test-binaries).

Workspace clippy:

```
$ cargo clippy --workspace -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.09s
```

Zero warnings.

Workspace fmt:

```
$ cargo fmt --all --check
(no output — clean)
```

Correctness gate:

```
$ cargo test -p verter_session --test correctness
test result: ok. 18 passed; 0 failed; 1 ignored; 0 measured;
            0 filtered out; finished in 0.37s
```

Snapshot drift: none.

## Test count invariant

Brief expected workspace count to stay `10297/0/2/45`. No new tests
were added by this phase (Fix A rewrote three existing tests, did not
add new ones). Final count matches expectation exactly.

## Tests added / un-ignored

None. Fix A rewrote three existing tests in place; the test names
remain unchanged.

## Deferred

None. The Phase 9.2 deferred item from `<scratch>/verter-cutover-state.json`
is closed by Fix B's inventory document.

## Commit list

| sha       | subject |
| --------- | ------- |
| `d7157606` | refactor(meta_resolve): rewrite stale TODO(phase-5g) markers as past-tense bridge documentation |
| `b8bdfa6c` | test(architecture): rewrite Phase 5d Class A guards for post-Phase-11 folder layout |
| `b5109cf1` | test(architecture): refresh post-cutover narrative in pre-Phase-11 guard commentary |
| `e926effa` | docs(claude): list all 5 ProjectionMode variants in macro-traversal rule |
| `7f63581a` | docs(orchestrator): MCP↔LSP dependency direction inventory (closes phase-09.2 deferred) |
| `+1`     | chore(orchestrator): mark post-cutover-review-fixes complete (this commit) |

`base_commit`: `8345b6f92c51ac265cfcaf084dd4b0f6f916f7a5`
`work_head_before_marker`: `7f63581adc78193fdc8fbe6927dd6582ccb9c3da`
