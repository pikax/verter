# Phase 05 — STUCK report

**Phase id:** 05
**Branch:** `wt/phase-05-engine-retirement`
**Base commit at spawn:** `a85586931f464bed21e932ee79d8281c9dad0fe7`
**Worktree HEAD at stop:** `151491c2` (feat(workspace): add
ProjectStableKey for ambient-lib registration)
**Worktree:** `D:/dev/wt/phase-05-engine-retirement`
**Sub-plan:** `D:/tmp/verter-architecture-cutover-phase-05.md` (r8)

## TL;DR

One foundational commit landed (`ProjectStableKey` per §6.2 / A3, 7
unit tests passing, full workspace test suite green at 10097
passed / 0 failed / 8 ignored). Rest of Phase 5 (commits 0a
remainder, 0b, 1, 2+3, 3.5, 3.6, 3.7, 4a-9, 11, N+1, N+2) NOT
landed. STOP escalation per brief.

## What landed (commit `151491c2`)

`feat(workspace): add ProjectStableKey for ambient-lib registration`

Files added/modified:
- `crates/verter_workspace/src/project_key.rs` (NEW, 270 LOC
  including 7 unit tests)
- `crates/verter_workspace/src/lib.rs` — module declaration +
  re-export
- `crates/verter_workspace/Cargo.toml` — `xxhash-rust` dep
- `Cargo.lock` — lock-file refresh

Tests:
- `configured_key_distinguishes_sibling_tsconfigs` — pass
- `configured_key_distinguishes_multi_root_workspaces` — pass
  (A3 workspace-root discriminator)
- `configured_and_fallback_at_same_path_differ` — pass
- `hex_tag_round_trips` — pass
- `hex_tag_format_is_one_letter_prefix_plus_32_hex_chars` — pass
- `parse_hex_tag_rejects_invalid_inputs` — pass (covers wrong
  prefix, too short, 31/33-char hex, non-hex chars)
- `key_is_deterministic_across_calls` — pass

Workspace test suite at this commit:
- `cargo test --workspace --tests --verbose` (logged to
  `/tmp/p05-c0a-workspace.txt`): 43 test blocks, 10097 passed, 0
  failed, 8 ignored.
- `cargo build --workspace --tests`: clean, 1m 24s.
- `cargo fmt --all --check`: clean.
- `cargo clippy -p verter_workspace -- -D warnings`: clean.

## Why I stopped

### Reason 1 — sub-plan scope materially exceeds single-worker capacity

Sub-plan §5 lists 12+ commits, each requiring a full workspace
verification cycle:

```bash
cargo test --workspace --tests --verbose
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all --check
pnpm install --frozen-lockfile
```

The full workspace test suite has 10097 tests across ~43 blocks.
After every callsite migration and every body-conversion in 11k+
line files, the dependent crates (verter_session, verter_compiler,
verter_lsp, verter_napi, verter_wasm, verter_tsc, verter_mcp) all
recompile, triggering build cycles in the order of 1-2 minutes
each. End-to-end commit verification cycle ≈ 5-10 minutes per
commit minimum.

Substantive workload remaining for Phase 5:

1. **Commit 0a remainder** (≥800 LOC of NEW code + 16 NEW tests):
   - `crates/verter_workspace/src/traits.rs` extension: 8 trait
     methods (`register_ambient_lib`, `unregister_ambient_lib`,
     `read_ambient_lib`, `ambient_virtual_canonical_id`,
     `record_ambient_dependency`, `project_stable_key`,
     `lookup_ambient_symbol` + types `AmbientLibSpec`,
     `AmbientLibError`, `AmbientSymbolHit`)
   - `crates/verter_workspace/src/engine.rs`: `ambient_libs:
     ArcSwap<AmbientLibsByProject>` field + companion types
     `AmbientLibsByProject`, `ProjectAmbientLibs`,
     `AmbientLibEntry`
   - `crates/verter_workspace/src/memory.rs` +
     `crates/verter_workspace/src/filesystem.rs` concrete
     overrides per A1
   - `parse_top_level_exports(&str)` helper via `verter_oxc_parser`
   - 16 §6.8 workspace test cases
   - `crates/verter_session/src/host_manage.rs`:
     `HostFenceValidator::validate` WholeHash arm patch (A8)
     including `validate_ambient_whole_hash` helper
   - `crates/verter_session/src/resolver_core/ambient_resolve.rs`
     (NEW module)

2. **Commit 0b**: hermetic harness `build_hermetic_host_with_lib`
   + STUB_LIB_ES5

3. **Commit 1**: 5 TDD seed tests (each FAIL pre-Phase-5,
   PASS post-Phase-5) plus programmatic baseline JSON

4. **Commit 2+3**: `SemanticQueryKey::ResolveMacroPayload` variant
   + body (~150 LOC) including `build_emits_object`,
   `build_slots_object`, `build_model_object` sub-helpers

5. **Commit 3.5**: Class A dispatch parity (every fixture,
   byte-equal) + characterizations + interning budget +
   Navigate integrity

6. **Commit 3.6**: 4 dispatch helpers (`materialize_surface`,
   `execute_pick`, `execute_omit`, `execute_to_type_expr`) +
   trivial helpers (`lower_path_segments`,
   `intern_string_literal_union`,
   `resolve_decl_identity_in_scope`,
   `get_analyzed_macro_by_index`) + builtin decl identity helpers

7. **Commit 3.7**: trampoline conversion (atomic body swap of all
   retired engine methods, ~50+ methods) + counter test rewrite
   per A9 in same commit (15 sites in `meta_resolve_tests.rs`)

8. **Commits 4a, 4b, 4c, 5, 6, 7, 8, 9**: 280 callsite migrations
   across 10 files:
   - `meta_resolve.rs`: 47 occurrences (38 §C-classified sites)
   - `component_meta_query_engine.rs`: 180 internal occurrences
   - `meta_resolve_tests.rs`: 18 sites
   - `d_cutover_characterization_tests.rs`: 17 sites
   - `host_manage.rs`: 7 sites (4 §C sites)
   - `host_manage_tests.rs`: 1 site
   - `parity_tests.rs`: 3 sites
   - `meta_tests.rs`: 1 site
   - `type_expansion_verter.rs`: 2 sites
   - `fallthrough.rs`: 4 sites

9. **Commit 11**: ≈4500-5500 LOC deletion from
   `component_meta_query_engine.rs` (currently 11119 lines).
   Plus `#[cfg(test)]` counter-field removal. Plus
   `tools/phase05-callgraph.rs` (NEW `syn::Visit` script) for
   §4.3 deletion gate.

10. **Commit N+1**: `MyPick ≡ Pick` lib parity + userland
    shadowing test per parent §5.C

11. **Commit N+2**: 7 deferred fixtures (5 from
    `phase-00-tier1-mismatches.md` rows 1-5 + 2 from
    `phase-00b-tier1-mismatches.md` rows 1-2) authored with
    rule-correct expected derived from TS spec citations, run
    `--ignored generate_class_a_snapshots_from_expected`. Each
    fixture must pass post-Phase-5 (else STOP per brief).

Realistic minimum: ~2000-3000 LOC of new production code across
~25 files, ~600 LOC of new tests, careful migration of 280
callsites in two ~11k-line files, and 11+ verified-green commit
cycles.

### Reason 2 — TDD discipline contract is non-negotiable

Per CLAUDE.md and brief R2: "Each test must FAIL on the
pre-change tree and PASS on the post-change tree." For 5 seed
tests in commit 1, each requires:

1. Author against documented Phase 0a/0b defect.
2. Run pre-Phase-5 and capture failure mode (programmatic baseline
   JSON to `/tmp/phase-05-seed-baseline.json`).
3. Verify failure is for documented root-cause reason (NOT for a
   different reason — STOP otherwise per brief).
4. Implement fix (touches resolver core internals).
5. Re-run and verify it now passes.
6. Add negative-regression mutation tests that re-fail on the
   inverse mutation.

Each seed alone is multi-hour TDD.

### Reason 3 — sub-plan §6.5 anchor mismatch (small, but exemplary)

Sub-plan §6.5 references `published.snapshot.workspace_root`.
That field does not exist on `WorkspaceSnapshot`
(`crates/verter_workspace/src/workspace_snapshot.rs:38-46` has
`projects`, `resolver`, `generation` only). The actual
`workspace_root` is per-project, on
`OwnershipProject.workspace_root: CanonicalPath` (line 56). The
fix is mechanical (use the per-project workspace_root from the
selected `OwnershipProject` instead) — this is the kind of
"small drift" §0.6.1 explicitly permits. NOT a blocker on its
own; flagged here as evidence that some §2/§7 anchors are out of
sync with HEAD `a85586931f`.

The §7 worker pre-flight item #1 explicitly says to verify this:
"WorkspaceSnapshot.workspace_root field exact location (anchored
at `:38+` in plan; verify exact line)". The audit found it: the
field is on `OwnershipProject`, not on `WorkspaceSnapshot`.

### Reason 4 — `access.rs` does not exist; trait is in `traits.rs`

Sub-plan §6.1 references `verter_workspace/src/access.rs` (extends
existing WorkspaceAccess trait). The actual module is
`crates/verter_workspace/src/traits.rs` (verified file listing).
Trivial drift; flagged for sub-plan currency.

## What I attempted before stopping

1. Verified worktree state at base commit `a85586931f` (clean,
   phase 01 marker present).
2. Verified `.integration-tests/repos/nuxt-ui` junction in place.
3. Read sub-plan §0–§9 in full.
4. Read `phase-00-tier1-mismatches.md` (5 fixtures) and
   `phase-00b-tier1-mismatches.md` (2 fixtures) in full.
5. Pre-flight audit of §7 anchors: confirmed
   - `WorkspaceSnapshot.workspace_root`: actually on
     `OwnershipProject`, not `WorkspaceSnapshot` (Reason 3).
   - `verter_workspace/src/access.rs`: doesn't exist; trait is
     in `traits.rs:43` (Reason 4).
   - 280 engine-method callsites confirmed via Grep.
   - `cargo build --workspace --tests` baseline: 1m 29s.
   - `Hash16` re-export path: actually
     `verter_scheduler::invalidation::Hash16`, not
     `verter_scheduler::Hash16` (sub-plan §6.3 stale).
   - `xxhash-rust` `xxh3_128` is the appropriate Hash16 producer
     (already used in `verter_session::component_meta_audit::footprint_miner`).
6. Implemented `ProjectStableKey` (sub-plan §6.2) with 7 unit
   tests covering all A3 invariants.
7. Verified workspace test suite still green: 10097 passed, 0
   failed, 8 ignored (`/tmp/p05-c0a-workspace.txt`).
8. Verified `cargo fmt --all --check` clean.
9. Verified `cargo clippy -p verter_workspace -- -D warnings`
   clean.
10. Committed `151491c2`.

## Recommendation

**Re-amend or split the sub-plan into smaller phases.** Suggested
split:

- **Phase 5a** (commit 0a remainder, 0b): workspace ambient-lib
  infrastructure. `AmbientLibAccess` methods on `WorkspaceAccess`,
  `Engine::ambient_libs` ArcSwap, `MemoryWorkspace`/
  `FilesystemWorkspace` overrides, `parse_top_level_exports`,
  `ambient_resolve.rs`, `HostFenceValidator` patch, hermetic
  harness, 16 workspace tests. (Foundation already laid by
  `151491c2`'s ProjectStableKey.)
- **Phase 5b** (commits 1, 2, 3, 3.5, 3.6): 5 TDD seed tests +
  `ResolveMacroPayload` variant + body + 4 dispatch helpers +
  Class A parity. Closes seeds for slots/models. Authors 2 of the
  7 deferred fixtures.
- **Phase 5c** (commit 3.7): engine trampoline conversion +
  counter-test rewrite per A9.
- **Phase 5d** (commits 4a, 4b, 4c): Class A and B callsite
  migrations.
- **Phase 5e** (commits 5, 6): route-loop + route-target
  migrations + retire `instantiate_local_generic_ref`.
- **Phase 5f** (commits 7, 8, 9): fallthrough + indexed-paths +
  package-backed migrations. Closes remaining seeds.
- **Phase 5g** (commits 11, N+1, N+2): engine deletion + lib
  parity + remaining 5 fixture authorship.

Seven smaller phases each landing 2-3 commits with full workspace
verification would be tractable. The current Phase 5 sub-plan
expects a single worker to land 12+ commits with 11k+ line file
edits and 280 callsite migrations and 7 new fixture authorings in
a single session.

## Status

- Branch `wt/phase-05-engine-retirement` HEAD: `151491c2` (1
  commit ahead of base `a85586931f`).
- Workspace tests: green (10097/0/8).
- 0 of the 5 seed defects from §5.A closed.
- 0 of the 7 deferred fixtures from §5.B.5 authored.
- Engine LOC reduction: 0 (out of estimated ≈4500).
- Snapshot drift: none observed (no migrations made yet).

The brief's R7 explicitly forbids landing the phase-05-complete
marker for a partial implementation: "LAST commit. No work after
the marker." Marking complete now would be a gate-bypass per the
stub-prevention rule. Escalating per the brief's STOP path.

## Files to read for continuation

- This document.
- Sub-plan: `D:/tmp/verter-architecture-cutover-phase-05.md`.
- Mismatch registers: `D:/dev/personal/verter/phase-00-tier1-mismatches.md`,
  `D:/dev/personal/verter/phase-00b-tier1-mismatches.md`.
- Anchor audit findings: see "What I attempted" above; relevant
  re-discovered anchors:
  - `Hash16` re-exports from `verter_scheduler::invalidation::Hash16`
  - `WorkspaceSnapshot.workspace_root` does NOT exist; field is on
    `OwnershipProject.workspace_root`
  - Trait `WorkspaceAccess` lives in
    `crates/verter_workspace/src/traits.rs:43`, not in
    `access.rs` (which doesn't exist)
  - `xxhash_rust::xxh3::xxh3_128` is the in-repo `Hash16`
    producer
- Test logs: `/tmp/p05-c0a-workspace.txt`,
  `/tmp/p05-c0a-build.txt`, `/tmp/p05-c0a-clippy.txt`.
