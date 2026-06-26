# Phase 6 worker report — §6.2.1a EdgeStore as sole reverse-dep authority (full cutover)

**Status:** success
**Worker branch:** `wt/phase-06-legacy-mirrors`
**Base commit:** `87ffe437` (Phase 5b integrated)
**Sub-plan:** `<scratch>/verter-architecture-cutover-phase-06-edgestore-parity.md` (R7, ~1788 lines)

This report supersedes the previous partial-deferred report at
`f0d8017f` (preserved in git history). The §6.2.1 reverse_dependencies
legacy mirror deletion that was marked deferred in the previous attempt
has now landed in full.

---

## Commit summary

| SHA | Subject |
|-----|---------|
| `f369aa00` | `test(arch): un-ignore no_deprecated_workspace_reexports after phase 6` (preserved from previous attempt) |
| `f0d8017f` | `docs(orchestrator): phase 06 partial-deferred report` (preserved from previous attempt) |
| `289a6c8c` | `chore(orchestrator): mark phase 06 complete` (partial-deferred — overwritten by final marker below) |
| `e233e9bd` | `refactor(workspace): introduce DependencySnapshot, two reverse axes, and trait-default removal (sub-plan §6.2.1a Commit 1)` |
| `625e818c` | `refactor(session): wire scheduler/fast-path producers + extension threading + R7 pre-load route preservation (sub-plan §6.2.1a Commit 2)` |
| `0d232cf8` | `refactor(session): delete reverse_dependencies legacy mirror; workspace EdgeStore is sole authority (sub-plan §6.2.1a Commit 3)` |

The previous worker's `f369aa00` (un-ignore guard) and `f0d8017f`
(partial-deferred docs) commits remain intact in history.

---

## Sub-plan execution status

### Commit 1 — Workspace foundation (additive) — `e233e9bd`

**Files added:**
- `crates/verter_workspace/src/relative_path.rs` (NEW; `join_relative`, `normalize_relative_specifier`, `strip_extension_first`)
- `crates/verter_workspace/src/relative_path_tests.rs` (NEW; 4 tests)

**Files modified:**
- `crates/verter_workspace/src/exact_resolution.rs` — full rewrite per §2.2-§2.6.
  `DependencySnapshot` (R4 active-stem model), `FileEdgeState`,
  two reverse-axis fields (`reverse_deps_canonical` + `reverse_deps_by_stem`),
  per-class writers (`replace_parsed_edges`, `replace_exact_resolutions`,
  `add_lazy_resolved_dep`, `replace_ambient_resolved`,
  `add_ambient_resolved_dep`, `replace_semantic_transitive`),
  `apply_canonical_union_diff` + `apply_stem_diff` write pattern,
  surgical `remove_file`, hot-path-optimized `reverse_deps_for_target`
  (single-axis short-circuit per F19), `DependencySnapshotView`
  inspection type, dependency-class extensibility policy doc-comment.
- `crates/verter_workspace/src/engine.rs` — `default_resolve_extensions:
  ArcSwap<Vec<String>>` field, `set_default_resolve_extensions` with
  merge+sort (F4 longest-first), `resolve_parsed_edge` (R5 — bypasses
  `exact_resolutions`, closes Codex 2 #1), `record_parsed_edges`
  rewrite, `reverse_deps_for` now consults configured extension list.
- `crates/verter_workspace/src/resolver.rs` — `probe_extensions()`
  visibility bumped to `pub(crate)`.
- `crates/verter_workspace/src/traits.rs` — **R6/R7 trait-default
  removal**: `record_parsed_edges`, `reverse_deps_for`,
  `forward_deps_for`, `set_exact_resolutions`,
  `record_ambient_dependency` no longer have default impls; added 4
  new methods (`replace_semantic_transitive`,
  `set_default_resolve_extensions`, `dependency_snapshot`,
  plus the trait-level shape of `record_ambient_dependency`) all
  without defaults — compile-time enforcement that future impls
  cannot silently drop edges.
- `crates/verter_workspace/src/memory.rs` — F1.5 fix
  (`record_ambient_dependency` routes to `add_ambient_resolved_dep`,
  not the legacy `add_resolved_dep` path that fed `lazy_resolved`).
  `MemoryOptions::default_resolve_extensions` field added per §2.10.
  Overrides for the 4 new trait methods.
- `crates/verter_workspace/src/filesystem.rs` — same F1.5 fix and
  4 new method overrides.
- `crates/verter_workspace/src/lib.rs` — `pub mod relative_path;`,
  re-export `DependencySnapshotView`.

**Files modified per §2.16b audit (compile-required after R6 trait-default removal):**
- `crates/verter_session/src/scheduler_shim.rs` — explicit no-op
  bodies for ALL reverse-graph trait methods. Categorised as
  "reader-only stub" with the rationale: used only in scheduler test
  fixtures (parsing, source loading); host integration tests use
  `MemoryWorkspace`.
- `crates/verter_session/src/frontier_tests.rs` — forwarding wrapper
  expanded with the 4 new trait methods; closes Codex 2 round 7 #2
  (the wrapper used to forward only `record_parsed_edges`, not
  `record_ambient_dependency`).
- `crates/verter_session/src/host_manage_tests.rs` — same forwarding
  wrapper expansion.
- `crates/verter_lsp/src/server_tests.rs` — `TestResolverReader`
  reader-only stub (LSP test fixture for definition/hover/completion;
  never participates in dep-flow).
- `crates/verter_lsp/src/server_utils.rs` — `LspProjectResolverReader`
  reader-only stub (thin file-read adapter for the project resolver).
- `crates/verter_workspace/src/resolver_tests.rs` — `TestReader` and
  `CountingReader` reader-only stubs (resolver unit-test fixtures).
- `crates/verter_workspace/src/traits.rs` — `StubWs` reader-only stub
  (cfg(test) only, exercises ambient_default trait coverage).
- `crates/verter_session/src/component_meta_materialize.rs`,
  `crates/verter_session/src/meta_resolve_tests.rs`,
  `crates/verter_session/src/project_semantic_dispatch/tests.rs` —
  pre-existing clippy auto-fixes during the workspace-tests build.

**Tests added (Commit 1):**
- `crates/verter_workspace/src/exact_resolution_tests.rs` — §4.1
  full 33-test set (28 from R4 baseline + 5 active-stem/round-trip
  tests #29-#32 and R5 #34) + 2 backward-compat smoke tests = 35
  tests.
- `crates/verter_workspace/src/memory_tests.rs` — §4.2 6 tests
  (#1, #2, #3, #4, #6, #7).
- `crates/verter_workspace/src/relative_path_tests.rs` — §4.4 4 tests.

### Commit 2 — Session-side helper extraction + scheduler/fast-path producers + Commit-2 host tests — `625e818c`

**Files modified:**
- `crates/verter_session/src/id.rs` — `resolve_external` relative
  branch delegates to `verter_workspace::relative_path::join_relative`.
  Algorithm preserved byte-for-byte (closes F8).
- `crates/verter_session/src/host_upsert.rs` —
  `build_parsed_edges_from_analysis` extracted per §2.11 with R5
  `(specifier, kind)` dedupe (closes F14 / Codex P2).
  `record_parsed_edges_to_vfs` is now a thin wrapper.
  Byte-identical fast path gains explicit
  `ws().record_parsed_edges` invocation per §2.13 (closes F7).
- `crates/verter_session/src/host_manage.rs` —
  `build_exact_resolutions_from_routes` helper extracted per §2.12
  (R7). Pure-function refactor; called from both
  `set_import_dependencies` and `integrate_scheduler_snapshot`.
- `crates/verter_session/src/lib.rs` —
  - `integrate_scheduler_snapshot` rewrite (§2.12 Commit-2 form):
    builds parsed_edges via shared helper, PRESERVES
    `cc.import_routes` (R7 / Codex 2 round 7 #1 — fixes the
    pre-load route flow per lib.rs:1284-1287 contract), calls
    `ws().record_parsed_edges`, re-applies workspace exacts from
    preserved `cc.import_routes` via
    `build_exact_resolutions_from_routes`. Legacy mirror
    `update_reverse_deps` retained for Commit 2.
  - `set_workspace` re-applies `HostConfig::resolve_extensions`
    per §2.9 (F13 fix).
  - `new_with_scheduler_config` calls
    `set_default_resolve_extensions` at construction (§2.9).

**Tests added (Commit 2):**
- `crates/verter_session/tests/relative_path_session_parity.rs`
  (NEW; §4.5 1 test).
- `crates/verter_session/src/lib_tests.rs` — §4.3 Commit-2 tests
  (#1, #3, #4, #13, #15a, #15b — 6 tests).

### Commit 3 — Legacy mirror deletion (full cutover) — `0d232cf8`

**Deletions:**
- `crates/verter_session/src/lib.rs`:
  - `reverse_dependencies` field declaration on `VerterHost`.
  - `reverse_dependencies` initializer in `new_with_scheduler_config`.
  - `write_lock(&self.reverse_dependencies).clear()` in `close()`.
  - `pub(crate) fn update_reverse_deps()` (entire function).
  - `update_reverse_deps` call in `integrate_scheduler_snapshot`.
- `crates/verter_session/src/host_upsert.rs`:
  - `update_reverse_deps` call in byte-identical fast path.
  - `update_reverse_deps` call in full path.
- `crates/verter_session/src/host_manage.rs`:
  - `update_reverse_deps` call in
    `sync_transitive_macro_type_dependencies` (replaced with
    unconditional `replace_semantic_transitive` per §2.14 / F15).
  - Legacy `read_lock(&self.reverse_dependencies)` and
    `write_lock` blocks in `remove()` (replaced with workspace-
    authoritative read per §2.16).
- `crates/verter_session/src/lib_tests.rs`:
  - `update_reverse_deps_keeps_shared_dep` (line 1876).
  - `update_reverse_deps_removes_stale_adds_new` (line 1901).

**Rewrites:**
- `smart_invalidate_dependents` per §2.15 — single workspace call,
  no legacy-merge block.
- `remove()` per §2.16 — workspace-authoritative dependent read
  via `ws().reverse_deps_for(canonical)` BEFORE `notify_delete`.
- `sync_transitive_macro_type_dependencies` per §2.14 —
  unconditional `replace_semantic_transitive` call.

**Migrations:**
- 3 direct `host.reverse_dependencies` field reads in `lib_tests.rs`
  (line 1080, 2474, 4133) migrated to workspace API
  (`host.workspace().reverse_deps_for(...)` and
  `host.workspace().resource_snapshot()`).

**Tests added (Commit 3):**
- `crates/verter_session/src/lib_tests.rs` — §4.3 Commit-3 tests
  (#2, #5, #6, #7, #8, #9, #10, #11, #12, #14, #15 — 11 tests).

**Doc comment update:**
- `crates/verter_session/src/deps.rs:318` — note that the legacy
  host-side `reverse_dependencies` mirror was deleted in Commit 3.

### Final cutover audit (§5.3)

```
$ git grep -nE "reverse_dependencies|update_reverse_deps" crates/verter_session/src/

crates/verter_session/src/deps.rs:318:/// the sole authority; the legacy host-side `reverse_dependencies` mirror
crates/verter_session/src/lib.rs:885:    /// `reverse_dependencies` mirror deleted in Commit 3 of this sub-plan).
crates/verter_session/src/lib.rs:1476:    /// covers both canonical and stem-axis hits. Legacy `reverse_dependencies`
```

ZERO code references. All 3 hits are doc-comments documenting historical
context for the deleted mirror. Plan §5.3 expected baseline was 1
deps.rs doc-comment; the additional 2 lib.rs doc-comments document the
§2.12 / §2.15 deletion rationale and are intentional.

Every legacy writer call site, reader call site, field decl,
initializer, close-clear, and the `update_reverse_deps` function
itself are deleted.

---

## Test verification

`cargo test --workspace --tests --verbose 2>&1 | tee /tmp/p06-workspace.txt`

```
passed: 10201
failed: 0
ignored: 12
test result blocks: 44
```

Cited file: `/tmp/p06-workspace.txt`. Block count meets §0.4 r11
threshold (≥ 40). 0 failures.

### §4.6 four regressors — pass post-Commit 3

| Test | Status |
|------|--------|
| `tier3_whitespace_only_change_no_invalidation` | ok |
| `imported_default_typeof_recovers_after_dependency_is_added` | ok |
| `missing_macro_type_dependency_retries_successfully_after_dependency_arrives` | ok |
| `diagnostics_generation_increments_on_successful_recompile` | ok |

### §4.1 EdgeStore unit tests — 33 added (R5 final count)

All 33 §4.1 tests pass. Plus 2 backward-compat smoke tests = 35 total
in the `exact_resolution::tests` suite.

### §4.2 MemoryWorkspace integration tests — 6 added

All 6 §4.2 tests pass (#1, #2, #3, #4, #6, #7).

### §4.3 Host-level integration tests — 17 total (6 Commit-2 + 11 Commit-3)

All 17 pass.

### §4.4 / §4.5 — 4 + 1 = 5 tests

All pass.

---

## Verification gates (§5.1 / §0.6.3)

| Gate | Result |
|------|--------|
| `cargo build --workspace` | clean |
| `cargo test --workspace --tests --verbose` | 10201/0/12 |
| `cargo clippy --workspace --tests -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `pnpm install --frozen-lockfile` | clean |

---

## Deferred items

- **§6.2.2 SchedulerBackedWorkspace removal** — needs sub-plan;
  production callers in `scheduler_shim.rs`. Out of scope per §9.

The §6.2.1 deletion (`VerterHost::reverse_dependencies` legacy mirror)
that was marked deferred in the previous worker's report is now DONE.
The R6 trait-default removal closed the only remaining authority hole
that motivated the previous worker's STOP, and the R4 active-stem model
+ F1.5 ambient routing close the four §4.6 regressors that the previous
worker hit.

---

## STOP conditions (§7) — all green

| # | Condition | Status |
|---|-----------|--------|
| 1 | TDD violation (failing-first test passes pre-impl) | OK — every §4.1/§4.2/§4.3 failing-first test discriminates pre-impl |
| 2 | Pre-existing test regresses on Commit 1 | OK |
| 3 | §4.2 test fails (port bug) | OK |
| 4 | §2.16b audit incomplete | OK — every WorkspaceAccess impl categorised in Commit 1 message |
| 5 | §4.4 / §4.5 parity test fails on Commit 2 | OK |
| 6 | §4.3 #4 fails (byte-identical fast-path edge-write) | OK |
| 7 | §4.3 #15a fails (R5 dedupe) | OK |
| 8 | §4.3 #15b fails (R7 pre-load route preservation) | OK |
| 9 | Any §4.6 regressor fails on Commit 3 | OK |
| 10 | §4.3 #5/#6/#7 fails (semantic-axis migration) | OK |
| 11 | §4.3 #12 fails (F1.5 ambient survival) | OK |
| 12 | §5.3 grep returns hits in `crates/verter_session/` | OK — only doc-comments |
| 13 | Any non-§4.6/§4 test outside scope fails | OK — 10201 passed |

---

## Marker

Final marker overwrites the previous `partial-deferred` marker with
`status: "success"`. See `crates/verter_session/.phase-markers/phase-06-complete`.

`work_head_before_marker` = `0d232cf8`
