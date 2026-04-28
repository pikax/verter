# Phase 6 — Kill legacy mirrors — REPORT

**Branch:** `wt/phase-06-legacy-mirrors`
**Base:** `6b1cd1e967bca2d0993c67bae220608e4429fecd`
**Status:** **partial-deferred** (per the marker schema)

## Sub-step outcomes

| Sub-step | Status | Notes |
|----------|--------|-------|
| §6.2.1 — Delete `VerterHost::reverse_dependencies` | **DEFERRED** (STOP per §6.3) | 4 pre-existing tests regress on the deletion; the legacy mirror's stem-matching for not-yet-resolved deps is NOT equivalent to `WorkspaceAccess::reverse_deps_for`. Migration requires §0.6.2 architectural decision (extending the workspace EdgeStore to track stem variants for missing files, or migrating tests to publish a workspace root). |
| §6.2.2 — `SchedulerBackedWorkspace` removal | **DEFERRED** (per brief) | Brief explicitly defers; production consumers exist in `scheduler_shim.rs`, needs sub-plan. |
| §6.2.3 — Drop deprecated workspace re-exports | **NO-OP / CONFIRMED** | At spawn base `6b1cd1e9`, `ProjectGraph`/`ProjectRank`/`VfsProjectConfig` are already absent from `verter_session::lib`. No external consumers found via `grep -rn "verter_session::(ProjectGraph|ProjectRank|VfsProjectConfig)"`. Sub-step is a confirmation-only no-op. |
| §6.2.3a — Flip `no_deprecated_workspace_reexports` `#[ignore]` | **DONE** | Commit `8ff3b611`. Guard runs un-ignored and PASSES against the post-spawn tree. |

## Pre-flight (§6.1) — all anchors verified

```
crates/verter_workspace/src/engine.rs:88
  pub(crate) published_state: ArcSwapOption<PublishedRoot>,

crates/verter_workspace/src/exact_resolution.rs:51
  pub struct EdgeStore {

crates/verter_session/src/host_manage.rs:7315 + 7320
  let rev = read_lock(&self.reverse_dependencies);
  let mut rev = write_lock(&self.reverse_dependencies);

crates/verter_session/src/scheduler_shim.rs (≥5 production refs)

crates/verter_workspace/src/exact_resolution.rs:191
  pub fn reverse_deps(&self, canonical_id: &str) -> Vec<String>
```

## §6.2.1 STOP CONDITION fired — root cause

§6.3 explicitly declares: "Any deletion regresses a pre-existing test — STOP."

I attempted the deletion in full per the brief:

1. Migrated read sites in `host_manage.rs::remove()` from `read_lock(&self.reverse_dependencies)` to `host.workspace().reverse_deps_for(&canonical)`.
2. Stripped the legacy-mirror merge block in `lib.rs::smart_invalidate_dependents`.
3. Deleted `update_reverse_deps()` and removed all three callsites (`host_upsert.rs:177`, `host_upsert.rs:292`, `lib.rs:916`, `host_manage.rs:6385` inside `sync_transitive_macro_type_dependencies`).
4. Removed the field declaration, initializer, and `close()` clear.
5. Removed the unused `read_lock` import in `host_manage.rs`.
6. Migrated `set_import_dependencies_adds_to_reverse_deps` to query the workspace API.
7. Deleted `update_reverse_deps_keeps_shared_dep` and `update_reverse_deps_removes_stale_adds_new` (probed deleted internals; their behaviour is covered by `verter_workspace::exact_resolution_tests`).
8. Removed the `close()`-clearing assertions in `lib_tests::test_close_full_cleanup` and the integration close-test.

Workspace built clean. Then `cargo test -p verter_session --tests` produced **4 regressions**:

```
host_resolve::host_resolve_tests::diagnostics_generation_increments_on_successful_recompile
host_resolve::host_resolve_tests::missing_macro_type_dependency_retries_successfully_after_dependency_arrives
lib_tests::tier3_whitespace_only_change_no_invalidation
meta::meta_tests::imported_default_typeof_recovers_after_dependency_is_added
```

### Diagnosis

I traced `tier3_whitespace_only_change_no_invalidation` end-to-end with `eprintln!` instrumentation in both the BEFORE and AFTER trees:

**BEFORE (legacy mirror live), step 2 — first `types.ts` upsert:**
```
ws.reverse_deps_for("/src/types.ts") = {}
legacy[/src/types.ts]                = None
legacy[stem=/src/types]              = Some({/src/Comp.vue})  ← captured by Comp.vue's prior upsert
merged owners                        = {/src/Comp.vue}
=> should_invalidate_dependent_view runs, populates cc.resolved_type_hashes
```

**AFTER (legacy mirror deleted), step 2 — first `types.ts` upsert:**
```
ws.reverse_deps_for("/src/types.ts") = {}    ← workspace can't resolve missing types.ts
ws.reverse_deps_for(stem)            = {}    ← workspace doesn't store stem-keyed reverse deps
owners                               = {}
=> should_invalidate_dependent_view never runs, cc.resolved_type_hashes stays empty
```

**Step 3 (compile):** workspace's lazy `resolve_import` finally succeeds (types.ts now exists), and `add_lazily_resolved_dep("Comp.vue", "/src/types.ts")` populates the workspace edge.

**Step 4 — whitespace-only re-upsert of `types.ts`:**
- BEFORE: `cc.resolved_type_hashes` populated. Tier 3 hash compare → hashes match → `should_clear=false`.
- AFTER: `cc.resolved_type_hashes` empty. Tier 3 sees no prior hash → `any_shape_changed=true` → `should_clear=true` → Comp.vue's `compile_slots.clear()`.

### Why this is architectural, not a worker-fixable detail

The legacy mirror was keyed by `id::resolve_external(canonical_id, &imp.source)` — a path-join that yields the EXTENSIONLESS `/src/types` even before `types.ts` is loaded. The workspace's `EdgeStore.reverse_deps` only records edges to canonical IDs whose `resolve_import` succeeded. With files arriving out-of-order in `MemoryWorkspace` test setups (no published `IdeProjectConfig` root), the workspace has no edge for the not-yet-existing dep, while the legacy mirror did.

Restoring the test scenario via the workspace API requires one of:
- (a) Extending `EdgeStore` to record stem-keyed reverse deps for unresolved relative imports (changes a public type's invariant — §0.6.2).
- (b) Changing `MemoryWorkspace` so `resolve_import` succeeds via path-join even without a published project root (changes resolver semantics — §0.6.2).
- (c) Migrating ~130 `new_standalone()`-based tests to publish workspace roots (mass test refactor — §0.6.2).

All three are explicit §0.6.2 architectural decisions outside this worker's scope. Per the spawn instructions ("If any STOP condition fires (§6.3 or §0.6.2), write `phase-06-stuck.md` ... and return"), §6.2.1 is the part that's stuck. §6.2.3 / §6.2.3a are independent and were completed cleanly.

## Verification (post-§6.2.3a-only commit)

```
cargo test --workspace --tests --verbose         → 10117 pass, 0 fail
cargo test -p verter_session --test correctness  → 11 pass, 0 fail, 1 ignored (Class A regen)
cargo clippy --workspace -- -D warnings          → clean (lib)
cargo fmt --all --check                          → clean
pnpm install --frozen-lockfile                   → clean
```

`--tests` clippy emits two pre-existing warnings in `meta_resolve_tests.rs` and `component_meta_materialize.rs` that the brief explicitly notes are "owned by Phase 11a, not this phase".

`no_deprecated_workspace_reexports` runs un-ignored and **PASSES** post-`8ff3b611`.

## Commits

| SHA | Message |
|-----|---------|
| `8ff3b611` | `test(arch): un-ignore no_deprecated_workspace_reexports after phase 6` |

## Deferred for follow-up

1. **§6.2.1** — Legacy `reverse_dependencies` mirror deletion. Needs §0.6.2 sub-plan addressing the stem-matching gap between the legacy mirror and `WorkspaceAccess::reverse_deps_for`. Suggested route: extend `EdgeStore::record_parsed_edges` to record stem-keyed reverse-dep buckets for unresolved Relative edges so the workspace can answer "who imports `./types`?" before `types.ts` is loaded.
2. **§6.2.2** — `SchedulerBackedWorkspace` removal. Production consumers in `scheduler_shim.rs`. Needs migration sub-plan per the brief.
