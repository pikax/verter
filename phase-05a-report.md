# Phase 05a — Ambient lib infrastructure (foundation)

**Phase id:** 05a
**Branch:** `wt/phase-05a-ambient-lib-infra`
**Worktree:** `<worktree>/phase-05a-ambient-lib-infra`
**Base commit at spawn:** `d9b3f90939ed45387b8d219c01ad971875b27f4c`
**Foundation commit on branch (preserved):** `db324056` (`feat(workspace):
add ProjectStableKey for ambient-lib registration`)
**Plan:** `<scratch>/verter-architecture-cutover-phase-05.md` (r9 / r13
anchor refresh) §5 commits 0a remainder + 0b, §6.1–§6.9, A1–A8.
**Disposition:** success.

## TL;DR

Phase 5a ambient lib infrastructure landed end-to-end: `WorkspaceAccess`
trait extended with a per-project ambient lib registry (sub-plan §6.1
+ A1), `Engine::ambient_libs: ArcSwap<...>` storage with lock-free CAS
(§6.3, §6.5), `MemoryWorkspace` + `FilesystemWorkspace` overrides,
oxc-based shallow-parse helper (§A6), `HostFenceValidator::validate`
ambient `WholeHash` arm (§6.6 / A8), session-side ambient global
resolver scaffold (§6.7), and the hermetic test harness wiring
including `STUB_LIB_ES5` (§5 commit 0b / §6.9). 39 new tests
authored in this phase (8 helper/default-trait, 18 §6.8 tests on
`MemoryWorkspace`, 5 `HostFenceValidator` tests, 2 `ambient_resolve`
tests, 6 `ambient_parse` tests, 3 commit-0b self-tests on the harness),
plus the 7 unit tests already on the foundation commit `db324056`. All
discriminating per R3.

The 5a `phase-05-stuck.md` was removed in the FIRST commit (commit
`43e7451b`) per parent §0.4 r13 worktree-rename clause.

## Commits landed in 5a (pre-marker)

| # | SHA | Message (subject) | Notes |
|---|---|---|---|
| (foundation, prior) | `db324056` | `feat(workspace): add ProjectStableKey for ambient-lib registration` | Held foundation, not re-authored. |
| (stuck-report, prior) | `c4096e63` | `docs(orchestrator): phase 05 STUCK report` | **Removed in `43e7451b`** per r13. |
| 1 | `43e7451b` | `feat(workspace): ambient lib trait surface and engine storage` | Removes `phase-05-stuck.md`. Adds `ambient_lib.rs` types + helpers, `Engine::ambient_libs: ArcSwap<...>`, `WorkspaceAccess` trait extension with default impls returning `Err(NotBootstrapped)` / `None`. 8 unit tests (3 helper, 5 default-trait). |
| 2 | `7de3e47f` | `feat(workspace): MemoryWorkspace and FilesystemWorkspace ambient lib registration` | Adds `ambient_parse.rs` shallow parser (oxc-based), CAS register/unregister loops, `Engine::register_ambient_lib`/`read_ambient_lib`/`lookup_ambient_symbol`/`project_stable_key`/`ambient_libs_view`/`unregister_ambient_lib`. `MemoryWorkspace` and `FilesystemWorkspace` override the trait defaults to delegate to `Engine`. 18 §6.8-style tests + 6 `ambient_parse` tests. |
| 3 | `52cc79e4` | `feat(session): HostFenceValidator ambient arm + ambient resolve scaffold` | Patches `HostFenceValidator::validate` WholeHash arm for `ambient:/<tag>/<canonical>` ids; adds `verter_session::resolver_core::ambient_resolve::resolve_ambient_global` (records reverse-dep edge, returns `ResolvedRootIdentity` with project-scoped virtual id). 5 + 2 tests. |
| 4 | `02079220` | `test(harness): build_hermetic_host_with_lib + STUB_LIB_ES5 + commit-0b self-tests` | Adds `STUB_LIB_ES5` constant (hand-derived mapped types), `build_hermetic_host_with_lib(files, lib_files)` helper with single configured project at `/ws`, mirrors libs at `/lib/<filename>` snapshot, registers via ambient API. 3 self-tests (`stub_lib_pick_resolves`, `register_ambient_lib_idempotent`, `vfs_shadowing_overlay_wins`). |

The work-head before the R7 marker is `02079220` — the marker commit
will follow.

## Confirmation

- **`phase-05-stuck.md` removed in the first commit (`43e7451b`):**
  yes — `git show 43e7451b -- phase-05-stuck.md` shows `deleted file
  mode 100644` in commit 1.
- **Foundation `db324056` intact:** yes — `git log
  --oneline d9b3f909..HEAD` shows it on the branch unchanged
  (foundation commit hash unchanged, file `crates/verter_workspace/
  src/project_key.rs` unmodified across 5a commits 1–4).
- **No `--amend`, no force-push.** No pushes performed.
- **Workspace-green discipline at every commit:**
  * Commit 1 → `cargo test --workspace --tests --verbose` 10,105 passed,
    0 failed, 8 ignored, 43 blocks (`/tmp/p05a-c1-workspace.txt`).
  * Commit 2 → 10,128 passed, 0 failed, 8 ignored, 43 blocks
    (`/tmp/p05a-c2-workspace.txt`).
  * Commit 3 → 10,135 passed, 0 failed, 8 ignored, 43 blocks
    (`/tmp/p05a-c3-workspace.txt`).
  * Commit 4 → 10,138 passed, 0 failed, 8 ignored, 43 blocks
    (`/tmp/p05a-workspace.txt`).
- **Correctness gate green:** `cargo test -p verter_session --test
  correctness` → 11 passed, 0 failed, 1 ignored
  (`/tmp/p05a-correctness.txt`).
- **`pnpm install --frozen-lockfile` succeeds:** yes (no lockfile
  drift; one-line "Done in 19.6s").

## Test results (measured by this worker)

The test counts below are measured by this worker running `cargo test
--workspace --tests --verbose` on the post-`02079220` tree, tee'd to
`/tmp/p05a-workspace.txt`.

| Scope | Command | Passed | Failed | Ignored | Blocks |
|---|---|---|---|---|---|
| `workspace` | `cargo test --workspace --tests --verbose` | 10,138 | 0 | 8 | 43 |
| `correctness` | `cargo test -p verter_session --test correctness` | 11 | 0 | 1 | n/a |

Tee paths cited:
- Workspace post-c4: `/tmp/p05a-workspace.txt`
- Correctness gate: `/tmp/p05a-correctness.txt`
- pnpm install: `/tmp/p05a-pnpm.txt`

## Snapshot drift

None. No correctness or audit snapshots were regenerated. No
`UPDATE_SNAPSHOTS=1` invocations.

## Guards un-ignored

None. No guards or `#[ignore]`'d tests were touched in 5a.

## Deferred items (sub-plan §0.5.1)

The 5a brief explicitly defers the following items into later
sub-phases (5b onwards):

- **Sub-plan §6.7 full session-side scheduler submission for ambient
  lib lazy parse**: `resolve_ambient_global` records the reverse-dep
  edge and returns a `ResolvedRootIdentity` with the project-scoped
  virtual canonical id, but does NOT yet submit a `Scheduler::Request`
  to drive the lazy parse → analysis → type lowering. This was
  intentionally deferred per the brief: "Lazy session-side scheduler
  submission for full parsing per A6 (sub-plan §6.7). NOT done eagerly
  at registration." First production caller (5b's bare-name resolver
  fallback) will extend the function in place.
- **§6.8 Test 9 (`register_ambient_lib_lazy_parse_via_session_scheduler`)
  and Test 10 (`register_ambient_lib_per_project_no_collision_in_scheduler`):**
  these depend on the §6.7 scheduler integration. Authored in 5b
  alongside the resolver-fallback caller.
- **§6.8 Test 5 (`register_ambient_lib_invalidates_dependents_via_fence_validator`)
  full E2E variant:** the 5 `HostFenceValidator` unit tests in
  `host_manage_tests::ambient_fence_validator_tests` cover the
  validator API directly (matching hash, stale hash after
  re-registration, unknown canonical, malformed virtual id, plain ids
  still go through `shallow_file_state`). The full
  end-to-end-through-a-real-cache-entry version belongs in 5b once
  there is at least one cache consumer that actually emits ambient
  `WholeHash` dep facts.
- All commits 1, 2+3, 3.5, 3.6, 3.7, 4a, 4b, 4c, 5, 6, 7, 8, 9, 11,
  N+1, N+2 from the parent §5 migration table — these are 5b–5g per
  parent §5.3 SERIAL split.

## Architecture notes (per CLAUDE.md)

- **Single shared owner crate:** ambient-lib registration types and
  Engine storage live in `verter_workspace`; session-side resolver
  scaffold lives in `verter_session::resolver_core`; harness wiring
  lives in `crates/verter_session/tests/component_meta_audit/harness.rs`.
  No consumer-local duplicates.
- **Authority chain respected:** registration mutates only
  `Engine::ambient_libs` via lock-free CAS; consumers read through
  `WorkspaceAccess::ambient_libs_view` / `read_ambient_lib` /
  `lookup_ambient_symbol` (no shadow state, no per-request mirrors).
  `HostFenceValidator` reaches the registry through
  `host.workspace().ambient_libs_view()` — no host-local cache.
- **Cache invalidation:** `register_ambient_lib` bumps
  `content_generation` only on actual content change (idempotent
  re-registration is a no-op). `HostFenceValidator`'s ambient arm
  rejects stale `WholeHash`es so downstream caches invalidate
  correctly.
- **Path normalization (A7):** at the public API boundary
  (`register_ambient_lib`, `read_ambient_lib`,
  `ambient_virtual_canonical_id`), all canonical_ids are normalised
  via `normalize_canonical_id` (`\` → `/`, trim leading `/`).
- **VFS shadowing (A5):** `register_ambient_lib` rejects with
  `NonAmbientCollision` when `WorkspaceAccess::file_exists` reports
  a non-ambient file at the canonical_id; `read_ambient_lib` returns
  `None` when an overlay or snapshot exists at the same canonical_id
  (immediate, no eventual-consistency window).
- **No new `SemanticQueryKey` variants in 5a.** The §0 binding
  amendment requires exactly ONE new variant (`ResolveMacroPayload`)
  to be added in 5b — 5a's scope is foundation only.

## Anchor verifications applied (sub-plan §7)

All `file:line` anchors quoted by the sub-plan and consumed in 5a
are at HEAD as documented (sub-plan r13 anchor refresh already in
effect):
- `WorkspaceAccess` trait at `crates/verter_workspace/src/traits.rs:43`
  (NOT `access.rs`).
- `OwnershipProject.workspace_root: CanonicalPath` per-project
  (NOT a top-level `WorkspaceSnapshot` field).
- `verter_scheduler::invalidation::Hash16` (16-byte alias, NOT a
  top-level re-export).
- `EdgeStore::add_resolved_dep(canonical_id, dep_id) -> bool` at
  `crates/verter_workspace/src/exact_resolution.rs:178` — used by
  `record_ambient_dependency`.
- `HostFenceValidator::validate(canonical_id, version)` at
  `crates/verter_session/src/host_manage.rs:470` — patched in-place,
  not replaced.
- `ResolvedRootIdentity::new(canonical_id, symbol_name)` at
  `crates/verter_semantic/src/analysis/type_solver/host.rs:25` —
  used by `resolve_ambient_global`.

## Pre-existing clippy issues (NOT from 5a)

- `crates/verter_session/src/component_meta_materialize.rs:1799`
  fires `clippy::manual_contains` (`trace.iter().any(|s| *s ==
  "Instantiate")` should be `trace.contains(&"Instantiate")`).
  Confirmed pre-existing on base `d9b3f909` via `git stash` /
  `cargo clippy` baseline; identical message before my changes.
- `crates/verter_session/src/meta_resolve_tests.rs:10082` unused
  import `NodeScopeId`. Same baseline.

These are not blocking 5a. The pre-existing tree fails
`cargo clippy --workspace -- -D warnings` independently of my
changes; flagged for orchestrator awareness.

## Files of interest

- `crates/verter_workspace/src/ambient_lib.rs` — types, helpers,
  CAS register/unregister.
- `crates/verter_workspace/src/ambient_parse.rs` — oxc-based shallow
  parser (`parse_top_level_exports`).
- `crates/verter_workspace/src/ambient_lib_tests.rs` — 18
  integration-style tests on `MemoryWorkspace`.
- `crates/verter_workspace/src/engine.rs` — `Engine::ambient_libs`
  storage + register/read/lookup orchestrators.
- `crates/verter_workspace/src/traits.rs` — `WorkspaceAccess` trait
  extension with defaults.
- `crates/verter_workspace/src/memory.rs`, `filesystem.rs` — backend
  overrides.
- `crates/verter_workspace/src/project_key.rs` — `ProjectStableKey`
  (foundation, unmodified in 5a).
- `crates/verter_session/src/host_manage.rs` —
  `HostFenceValidator::validate` ambient WholeHash arm patch +
  `validate_ambient_whole_hash` helper.
- `crates/verter_session/src/host_manage_tests.rs` — 5
  `ambient_fence_validator_tests`.
- `crates/verter_session/src/resolver_core/ambient_resolve.rs` —
  session-side `resolve_ambient_global` scaffold.
- `crates/verter_session/tests/component_meta_audit/harness.rs` —
  `STUB_LIB_ES5`, `build_hermetic_host_with_lib`, 3 self-tests.
