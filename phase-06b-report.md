# Phase 6b Worker Report (Continuation — Replacement)

**Status:** SUCCESS
**Branch:** `wt/phase-06b-cache-mirror-inventory`
**Base HEAD at spawn:** `23bff701` (rebased equivalent of prior partial-deferred branch tip `6364b5a9`)
**Final HEAD before marker:** `3f6b754f`
**Continuation worker scope:** §6b.D2a steps 1, 2, 3, 4, 5, 7 + §6b.D2b (all 8 steps) + §6b.D3 + replacement completion marker.

This report **REPLACES** the prior partial-deferred report referenced by §6b.8 — the prior worker's foundation commits (6b.A through 6b.D2a-step6 / D9) were rebased onto integration; this continuation worker built the unfinished 60% on top and produced a `status: "success"` marker per §6b.8.3 hard-stop constraints.

---

## 1. Commits added (continuation)

| SHA | Phase step | Title |
|---|---|---|
| `c6e7fbeb` | 6b.D2a | refactor(session): migrate F6/F7 readers/writers to RouteOwnedShallowDb materializer |
| `5ced1e8f` | 6b.D2b | refactor(session): close host.workspace() bypass via WorkspaceRead trait split |
| `3f6b754f` | 6b.D3  | test(session): final regression sweep + cached_external_type_analysis_entry deletion verification |

Foundation commits (already on the branch at spawn — preserved unchanged):

| SHA | Phase step | Title |
|---|---|---|
| `79fbad38` | 6b.A   | docs(session): annotate Phase 6b legitimate-authority and scratch fields |
| `8f461ea6` | 6b.B1  | refactor(session): introduce UnifiedResolverRuntime::for_tests helper |
| `cb6f5bf1` | 6b.B2  | refactor(session): rehome RouteDb / ImportedRootDb authority into ProjectTypeStore |
| `cfa856d0` | 6b.B3  | test(session): regression test for F3 Arc-shared eviction cascade |
| `e0b7eb60` | 6b.D1  | refactor(session): add RouteOwnedShallowDb destination + extend evict_canonical |
| `23bff701` | 6b.D2a-step6 | refactor(session): extend host-wrapper cascades with route_owned_shallow eviction |

Total commits on branch: 9 (6 foundation + 3 continuation).

---

## 2. All 13 characterization tests landed

Per §6b.0.2 lands-in matrix:

| # | Test name | Lands in commit | Classification |
|---|---|---|---|
| T1 | `route_db_and_imported_root_db_share_arc_identity_across_runtime_and_store` | `cb6f5bf1` (6b.B2) | discriminating |
| T2 | `route_owned_shallow_cache_field_absent_from_verter_host` | `c6e7fbeb` (6b.D2a) | discriminating |
| T3 | `route_owned_shallow_does_not_pollute_indexed_ready` | `c6e7fbeb` (6b.D2a) | discriminating |
| T4 | `route_owned_shallow_invalidated_by_content_hash_only` | `c6e7fbeb` (6b.D2a) | discriminating |
| T5 | `route_owned_shallow_concurrent_cold_callers_read_once_and_collapse` | `c6e7fbeb` (6b.D2a) | discriminating |
| T6 | `route_export_resolution_terminates_on_barrel_cycle` | `c6e7fbeb` (6b.D2a) | REGRESSION |
| T7 | `evict_canonical_cascade_includes_route_owned_shallow` | `e0b7eb60` (6b.D1) | discriminating |
| T8 | `route_owned_shallow_evicts_via_host_set_exact_resolutions_wrapper` | `5ced1e8f` (6b.D2b) | discriminating |
| T9 | `route_owned_shallow_clears_on_host_configure_projects` | `23bff701` (6b.D2a step6) | discriminating |
| T10 | `host_notify_close_evicts_route_owned_shallow` | `5ced1e8f` (6b.D2b) | discriminating |
| T11 | `workspace_accessor_visibility` (trybuild compile-fail) | `5ced1e8f` (6b.D2b) | REGRESSION |
| T12 | `route_owned_shallow_tiered_gate_invalidates_on_workspace_generation_bump` | `c6e7fbeb` (6b.D2a) | REGRESSION |
| T13 | `route_owned_shallow_tier3_rejects_stale_publish_after_route_resolution_change` | `c6e7fbeb` (6b.D2a) | REGRESSION |

Plus extra cascade-coverage tests landed in foundation commits:
- `route_db_eviction_visible_via_both_handles_after_close` (B3 regression).
- `route_owned_shallow_clears_on_host_clear_compile_cache` (D2a step 6).
- `route_owned_shallow_clears_on_host_set_workspace` (D2a step 6).
- `upsert_invalidation_matrix_evicts_route_owned_shallow` (D3 regression).

Total phase-6b-tagged tests: 17 (13 enumerated + 4 cascade-coverage).

---

## 3. Workspace verification

```
cargo test --workspace --tests --verbose:
  passed: 10241 / failed: 0 / ignored: 11 / blocks: 45

cargo test -p verter_session --test compile_fail (T11 trybuild):
  passed: 1 / failed: 0

cargo test -p verter_session --test correctness:
  passed: 11 / failed: 0 / ignored: 1

cargo clippy --workspace --tests -- -D warnings: clean

cargo fmt --all --check: clean

pnpm install --frozen-lockfile: in sync
```

Counts captured to `/tmp/p06b-cont-d3.txt`. Up from baseline 10230 (foundation-tip) by +11 (10 new tests landed in the continuation across D2a/D2b/D3 + 1 from C6e7fbeb-cascade-coverage). All workspace tests pass.

---

## 4. Files touched (continuation)

### `verter_workspace`
- `traits.rs` — split `WorkspaceAccess` into `WorkspaceRead + WorkspaceAccess: WorkspaceRead`; updated `StubWs` test impl.
- `lib.rs` — re-export `WorkspaceRead`.
- `memory.rs` — split `MemoryWorkspace` into `impl WorkspaceRead` + `impl WorkspaceAccess`.
- `filesystem.rs` — same split for `FilesystemWorkspace`.
- `resolver.rs` — retyped 23 `&dyn WorkspaceAccess` → `&dyn WorkspaceRead` (read-only callers).
- `config.rs` — retyped 13 `&dyn WorkspaceAccess` → `&dyn WorkspaceRead` (read-only helpers).
- `resolver_tests.rs` — split `TestReader` and `CountingReader`.
- `memory_tests.rs`, `filesystem_tests.rs`, `ambient_lib_tests.rs` — added `WorkspaceRead` import.

### `verter_session`
- `lib.rs` — demoted `workspace()` to `pub(crate)`; added `workspace_read()`; added `notify_close`, `notify_upsert(canonical, source)`, `set_exact_resolutions` host wrappers; deleted `external_type_analysis_cache` and `route_owned_shallow_cache` fields.
- `host_resolve.rs` — added `ensure_route_owned_shallow_entry` materializer (three-layer pattern + tiered staleness gate + pre-publish fence); added `route_owned_entry_is_fresh` helper + `route_owned_entry_is_fresh_for_test` test accessor; rewrote `cached_route_owned_*` helpers to delegate to materializer; rewrote `invalidate_route_owned_shallow_cache` and `snapshot_route_owned_shallow_cache_entries` to consult the project-store DB; preserved `RouteOwnedShallowStateSnapshot` as a projection with `from_entry` constructor; deleted `RouteOwnedShallowStateCacheKey`/`Entry` types and `route_owned_shallow_state_cache_key`/`cached_route_owned_shallow_state*`/`cache_route_owned_shallow_state` helpers.
- `host_manage.rs` — deleted `cached_external_type_analysis_entry` helper, `ExternalTypeAnalysisCacheKey`/`Entry` types; rewrote `external_type_resolution_inputs` to drive route-only fall-through through the materializer; removed the epoch-bump `external_type_analysis_cache.lock().clear()` and rewrote the comment to document the new discipline.
- `project_type_store.rs` — added `for_each_entry` iteration helper on `RouteOwnedShallowDb` (preserves stable iteration surface for `resolver_store::derived_hashes` fact-capture).
- `scheduler_shim.rs` — split `SchedulerBackedWorkspace` impl.
- `cross_file.rs:624`, `host_manage_tests.rs:2591`, `host_resolve_tests.rs:1184`, `lib_tests.rs:2876, 4425, 4528-4532` — rerouted bypass call sites through host wrappers (`configure_projects`, `set_exact_resolutions`, `notify_upsert`).
- `host_manage_tests.rs:118`, `frontier_tests.rs:115` — split `CountingWorkspace` impl.
- `phase_6b_characterization_tests.rs` — added `CountingWs` fixture (split impl); added T2-T6, T8, T10, T12, T13 + 6b.D3 regression test.
- `Cargo.toml` — added `trybuild = "1"` to `[dev-dependencies]`.
- `tests/compile_fail.rs` — trybuild driver.
- `tests/compile-fail/workspace_accessor_visibility.rs` — T11 fixture.
- `tests/compile-fail/workspace_accessor_visibility.stderr` — T11 expected stderr.
- `tests/component_meta_audit/harness.rs` — read consumers route through `host.workspace_read()`; mutator (`notify_upsert`) routes through `host.notify_upsert`.
- `tests/audited_request_e2e.rs` — added `WorkspaceRead` import.

### `verter_lsp`
- `documents/mod.rs:213-217` — `host.workspace().notify_upsert(...)` → `host.notify_upsert(...)`.
- `documents/mod.rs:322` — `host.workspace().notify_close(...)` → `host.notify_close(...)`.
- `server_utils.rs:243/248/255` — read calls → `host.workspace_read()`.
- `server_utils.rs:303/353/383/438/831` — function signatures retyped to `&dyn WorkspaceRead`.
- `server_utils.rs:237` — `LspProjectResolverReader` impl split into `WorkspaceRead` + `WorkspaceAccess`.
- `server_tests.rs:1025` — `TestResolverReader` impl split.
- `server_tests.rs:6457` — read handoff via `documents.host().workspace_read()`.
- `workspace_scanner.rs:594` — let-bind via `host_clone.workspace_read()`.

### `verter_mcp`
- `server.rs:3862, 3873` — read calls → `host.workspace_read()`.

### `verter_napi`
- `lib.rs` — `clippy --fix` removed redundant `use verter_workspace::WorkspaceAccess` shadows now that the import is at module scope (16 fixes).

### Workspace root
- `Cargo.toml` — MSRV `rust-version = "1.86"` added (authorized in §6b.5).
- `Cargo.lock` — auto-updated by trybuild dependency addition.

---

## 5. Anchor drift log

Re-anchor verification per §6b.0.1 was performed before each commit. No anchor drift > ±5 lines on any cited symbol from §6b.6. The integration tip advanced from `3147c02f` (plan-authoring HEAD) to `f52aa4de` (5g-supplement-complete) over 8 commits, but those 8 commits did not modify any of the cited verter_session anchors — they affected component_meta backfill tests in 5e/5f/5g-supplement.

---

## 6. Hard-stop constraints (§6b.8.3) compliance

| # | Constraint | Status |
|---|---|---|
| 1 | DO NOT spawn `phase-06b-followup` owner_phase | ✓ none introduced |
| 2 | DO NOT introduce 6b1/6b2 split | ✓ none introduced |
| 3 | DELETE `external_type_analysis_cache` and `route_owned_shallow_cache` mutex fields | ✓ both deleted in c6e7fbeb |
| 4 | DO NOT land partial marker | ✓ marker is `status: "success"` |
| 5 | Replacement marker has `status: "success"`, `deferred: []`, no follow-up reference, all 13 tests pass | ✓ verified |
| 6 | Tests discriminating per §6b.0.2 | ✓ all enumerated; T6/T11/T12/T13 REGRESSION |
| 7 | Re-anchor gate per commit | ✓ no drift |
| 8 | Pre-publish fence (STEP 7) mandatory | ✓ landed in `host_resolve.rs::ensure_route_owned_shallow_entry` |
| 9 | Tier-3 `project_generation` gate | ✓ landed in `route_owned_entry_is_fresh` |
| 10 | `set_exact_resolutions` wrapper bumps `bump_project_generation_and_evict()` AND `clear_all()` | ✓ landed in `lib.rs::set_exact_resolutions` |
| 11 | `notify_upsert` wrapper takes TWO parameters | ✓ `(canonical_id: &str, source: Arc<str>)` |
| 12 | `SingleflightGroup<Arc<str>, Arc<RouteOwnedShallowEntry>, ()>`; no `ResolverError` type | ✓ uses `()` error type per existing pattern |
| 13 | Same-hash reuse passes full freshness gate | ✓ STEP 5 in materializer applies tiered gate |
| 14 | CountingWorkspace fixture for T5 | ✓ inline `CountingWs` mirroring `host_manage_tests.rs:118` pattern |
| 15 | Inner DB method is `remove`, store-level wrapper is `evict_canonical` | ✓ confirmed |

---

## 7. Deferrals

**EMPTY.** Per §6b.8.3 hard-stop constraint #5, the replacement marker has `deferred: []`.

---

## 8. MSRV bump

Applied in workspace `Cargo.toml`: `rust-version = "1.86"`. Required for `Arc<dyn WorkspaceAccess>` → `Arc<dyn WorkspaceRead>` trait upcasting (Rust 1.86 / March 2025). Authorized in §6b.5 entry conditions. Verified by clean build under stable rustc 1.92.

---

End of replacement report.
