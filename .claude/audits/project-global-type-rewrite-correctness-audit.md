# Project-Global Type Rewrite — Correctness Audit

**Audit date:** 2026-04-18
**Branch:** `refactor/semantic-db-overhaul`
**Plan:** `C:\Users\david\.claude\plans\component-meta-project-global-cache-overhaul.md`
**Handoff it supersedes:** `.claude/audits/project-global-cache-session-handoff.md`

## Purpose

Per plan § J, the rewrite ships one authoritative path and no permanent
dual runtime. This audit records the correctness checks run before the
legacy path is retired. Because full Phase 4 / Phase 5 deletion is still
outstanding (see § Remaining Work below), the current audit documents the
state reached at the end of the multi-phase work track, together with the
verification gates that passed and any intentional deltas observed.

## What landed in this track (Phases 2 → 4 slice)

| Phase | Status | Commits |
| ----- | ------ | ------- |
| 2. `OwnerImportSurfaceDb` migration | complete | `431443ee` |
| 2.1. Intrinsic registry + SDK audit | complete | `dd2e6478` |
| 2.2. `SemanticQueryApi` binding on `ProjectTypeStore` | complete (ResolveDecl wired) | `bbd14808` |
| 3. `ComponentMetaResultDb` wiring | complete | `b10950ce` |
| 4. Request-view memo retirement | complete (memo slice) | `b1f2c0e5` |
| 5. Docs + audit archive | complete | this commit |

## Verification gates run

All commits below were run against the commit listed plus the ones
preceding it (each phase-final commit is green against every gate).

| Gate | Status | Evidence |
| ---- | ------ | -------- |
| `cargo test --package verter_session frontier_tests` | PASS (30 tests) | `/tmp/lib-tests.txt` (post Phase 2) |
| `cargo test --package verter_session host_manage` | PASS | subset of lib tests |
| `cargo test --package verter_session --lib` | PASS (1399 tests, 1 ignored) | `/tmp/p4-final.txt` |
| `cargo test --workspace --tests` | PASS (9731 tests, 0 failures) | `/tmp/p4-workspace.txt` |
| `cargo clippy --workspace --lib -- -D warnings` | PASS (zero warnings) | `/tmp/clippy-workspace-lib.txt` |
| `active_ts_sdk_intrinsic_audit_matches_default_registry` | PASS | intrinsic audit test run against `typescript@5.8.2` |
| `phase4_request_view_memo_retirement_source_audit` | PASS | retired memos no longer appear in source |
| `owner_direct_imports_resolve_once_per_owner_version_phase2` | PASS | surface cached + reused across stages |
| `owner_import_surface_rebuilds_after_owner_edit_phase2` | PASS | owner edit rebuilds under new hash |
| `semantic_subqueries_dedup_across_request_boundaries_phase22` | PASS | memo dedup confirmed |
| `component_meta_warm_rerun_hits_final_result_cache_phase3` | PASS | warm rerun returns cached payload, live counter unchanged |
| `component_meta_cache_invalidates_on_owner_edit_phase3` | PASS | owner edit evicts entry, triggers cold build |

## Old-vs-new corpus audit

The plan's mandatory old-vs-new corpus diff was **not** run in this
track: the legacy path is still compiled alongside the new cache layers,
so the diff would show identical output (the cache is a write-through
fast path). The meaningful correctness audit lands with the large Phase 4
signature cut, at which point the cold resolver runs through the new API
and an A/B diff against the legacy `_in_view` path becomes well-defined.

Intentional deltas for the observable behaviour that landed in this track:

1. **Warm reruns of `get_component_meta`** now skip resolver work entirely
   on unchanged owners (Phase 3). The analysis payload is `Arc`-cloned
   from the cache entry, so repeated callers see the same pointer.
2. **Direct owner imports** resolve exactly once per `(owner, whole_hash)`
   via `OwnerImportSurfaceDb`. Downstream consumers (`component_meta_registry`,
   `meta_resolve`) read from the same cached surface.
3. **Per-request `external_inputs_memo` / `eval_state_memo` on
   `RequestStoreView`** are retired — repeated in-request probes now
   collapse onto the project-global `ModuleFactsDb` / `IndexedReadyDb`.
4. **Intrinsic dispatch** routes through `IntrinsicRegistry::lookup` at
   the solver-host boundary; the active SDK audit test asserts the
   registry stays in sync with `lib*.d.ts`.

No behavioural regression was observed in the 9731-test workspace suite.

## Remaining work (tracked for the next session)

The plan's explicit deletion list from § G1 is partially complete:

| Item | Status | Notes |
| ---- | ------ | ----- |
| `RequestStoreView` deleted | **not done** | requires full Phase 4 signature cut |
| `CURRENT_REQUEST_VIEW` deleted | not done | same blocker |
| `RequestViewGuard` deleted | not done | same blocker |
| `effective_request_view` / `current_request_view` deleted from hot path | not done | same blocker |
| `EffectiveView` deleted | not done | same blocker |
| `extension_store` deleted | not done | still used by `ensure_loaded` |
| `external_inputs_memo` deleted | **done** | phase 4 slice, commit `b1f2c0e5` |
| `eval_state_memo` deleted | **done** | phase 4 slice, commit `b1f2c0e5` |
| `host_owned_resolved_named_types` folded | not done | requires Phase 2.2 expansion dispatch |
| `ImportedRootDb` direct-owner-import role | **done** | Phase 2, now transitive-only |
| `ExternalTypeBodyCache.resolved_roots` | **done** | commit `431443ee` |
| scheduler-freshness probes on `base_eval_env_arc` / `current_eval_state` | not done | same blocker |
| `*_in_view` signatures in hot path | not done | the big atomic cut |
| Source-audit test that hot-path modules are free of `RequestStoreView` | partial | Phase 4 memo-retirement audit landed; full hot-path audit blocks on the signature cut |

`ModuleFactsDb` still ships alongside `IndexedReadyDb` during this
transitional window — both publish under the same `whole_hash`, and
the Phase 0a `transitional_module_facts_db_coexists_with_indexed_ready`
test documents this explicitly. By plan § Phase 5, `ModuleFactsDb` must
have zero production consumers before its file is deleted. The existing
resolver still routes through `ModuleFactsDb`; migrating every consumer
to `IndexedReadyDb` + `ShallowFileState` lookups is a line-by-line
migration that pairs with the `_in_view` signature cut.

## Completion contract for the next session

The plan's acceptance criteria distinguish landable state from final
state. To be considered plan-complete, the following must hold:

1. `grep -n "RequestStoreView" crates/verter_session/src` returns no hits
   in `host_manage.rs`, `host_resolve.rs`,
   `resolver_core/component_meta_query_engine.rs`,
   `resolver_core/component_meta_registry.rs`,
   `resolver_core/solver_host.rs`, and `meta.rs`.
2. `grep -rn 'ModuleFacts\b' crates/verter_session/src` returns zero hits
   outside `module_facts_db.rs` itself, at which point the module
   deletes.
3. The old-vs-new corpus diff runs against a representative
   component-meta fixture set and records byte-identical native
   payloads modulo intentional deltas.
4. Integration tests for `nuxt-ui`, `element-plus`, `coreui`, `vuetify`
   pass or are annotated with documented regressions.
5. VS Code E2E + playground smoke run clean.
6. The three skill docs (`/type-resolution`, `/component-meta`,
   `/host-session`) reflect the final architecture.

Until those gates all pass, the rewrite is feature-landing-only; the
legacy path has **not** been deleted yet.

## Artifact identities after this track

- `IndexedReady` publishes alongside `ModuleFacts` (shared `shallow_state`
  and `import_routes` `Arc`s — identity check in `indexed_and_module_facts_share_shallow_state`).
- `OwnerImportSurfaceDb` is populated by `owner_import_surface_in_view`;
  stale owner hashes miss at the key level.
- `ComponentMetaResultDb` is populated by `get_component_meta` on cold
  build and consulted on subsequent calls; owner edits evict
  automatically via `project_type_store.evict_canonical`.
- `SemanticGraphStore` memoizes `ResolveDecl` keys; cold builds run
  exactly once per key until content changes.
- `IntrinsicRegistry` lookup fires only after declaration resolution
  yields `= intrinsic`; userland aliases reach this registry path only
  when they resolve to one of the six SDK-declared intrinsics.
- `HostFenceValidator` revalidates `WholeHash` and `ProjectGeneration`
  dep facts; route-generation facts are reserved for future emitters
  and validate permissively until the emission sites come online.
