# Phase 8 — Cache-Shape Audit (Post-5l + Post-4b)

**Branch:** `wt/phase-08-cache-rehoming`
**Base commit at spawn:** `e2f41a0066c60b0e2015df004d8e65b9fdcc1105` (phase-04b-complete; Phase 04b is the topologically-final marker on `refactor/semantic-db-overhaul` — phases 5a–5m, 6, 6b, 6c, 7 already merged into the branch ancestry)
**Integration target:** `refactor/semantic-db-overhaul`
**HEAD when audit performed:** `e2f41a00` (no Phase 8 commits yet)

---

## §1 Purpose

Phase 8 verifies Phase 6b's per-field cache classification of `VerterHost`,
inventories any caches introduced by Phase 5l engine retirement, and ships
the static guard `no_off_store_host_caches` that mechanically enforces the
"no caches outside `ProjectTypeStore`" rule documented in CLAUDE.md.

Phase 6b's classifications are FINAL and BINDING. This audit does NOT
re-litigate them. Where 6b classified a field `legitimate-authority`, the
guard ALLOW-LISTS the field with the 6b citation. Where 6b classified a
field `mirror`, this audit verifies the field is DELETED at the integration
tip.

Source of truth for classifications:
- `<scratch>/verter-architecture-cutover-phase-06b.md` — the §6b sub-plan
  (1092 lines) authored against HEAD `3147c02f` and applied across
  9 commits landing the `wt/phase-06b-cache-mirror-inventory` branch.
- `phase-06b-report.md` at the integration tip — the worker report
  enumerating the 9 commits and 13 characterization tests.
- The annotations on `VerterHost` itself — every `legitimate-authority`
  field carries a `Phase 6b classification: ...` doc-line citing the
  per-row sub-plan section (added in commit `79fbad38`).

---

## §2 Per-field audit

### §2.1 `VerterHost` (crates/verter_session/src/lib.rs:261)

The struct definition spans lines 261–398. Every field is enumerated below
in declaration order with its post-Phase-6b/6c/7 classification verified
against the worktree HEAD (= integration tip).

| #   | Line | Field                          | Type / shape                                                                              | Cache shape? | 6b row | Classification                          |
| --- | ---- | ------------------------------ | ----------------------------------------------------------------------------------------- | ------------ | ------ | --------------------------------------- |
| 1   | 262  | `instance_id`                  | `u64`                                                                                     | No (scalar)  | n/a    | non-cache                               |
| 2   | 263  | `config`                       | `HostConfig`                                                                              | No (struct)  | n/a    | non-cache (config payload)              |
| 3   | 267  | `workspace`                    | `Arc<RwLock<Arc<dyn WorkspaceAccess>>>`                                                   | No (cell)    | n/a    | non-cache (single-cell handle)          |
| 4   | 276  | `alias_to_canonical`           | `Shared<FxHashMap<String, String>>`                                                       | YES          | F12    | **legitimate-authority**                |
| 5   | 277  | `tick`                         | `AtomicU64`                                                                               | No           | n/a    | non-cache (counter)                     |
| 6   | 282  | `store_view_epoch`             | `AtomicU64`                                                                               | No           | n/a    | non-cache (epoch counter)               |
| 7   | 289  | `last_const_prop_overrides`    | `Shared<FxHashMap<String, FxHashSet<String>>>`                                            | YES          | F13    | **legitimate-authority** (state-diff)   |
| 8   | 292  | `metrics` (cfg-gated)          | `HostMetrics` (under `feature = "session_metrics"`)                                       | varies       | n/a    | non-cache (telemetry struct)            |
| 9   | 298  | `scheduler`                    | `Arc<verter_scheduler::scheduler::Scheduler>`                                             | No           | n/a    | non-cache (Arc handle)                  |
| 10  | 308  | `compile_cache`                | `dashmap::DashMap<String, CompileCacheEntry>`                                             | YES          | F1     | **legitimate-authority**                |
| 11  | 311  | `provenance`                   | `Arc<MetaProvenance>`                                                                     | No           | n/a    | non-cache (Arc; counters internal)      |
| 12  | 326  | `resolved_type_cache`          | `Mutex<FxHashMap<ResolvedTypeCacheKey, ResolvedTypeCacheEntry>>`                          | YES          | F2     | **legitimate-authority**                |
| 13  | 330  | `resolver`                     | `HostResolverState` (Arc-shared `RouteDb`/`ImportedRootDb` with `ProjectTypeStore` per F3) | No (Arc)     | F3     | non-cache wrapper (post-6b.B2 cutover)  |
| 14  | 341  | `eval_env_cache`               | `Mutex<FxHashMap<String, (Hash16, Arc<EvalEnv>)>>`                                        | YES          | F4     | **legitimate-authority**                |
| 15  | 355  | `semantic_db`                  | `Mutex<verter_semantic::db::SemanticDb>`                                                  | YES (DB)     | F5     | **legitimate-authority**                |
| 16  | 362  | `query_profile`                | `Mutex<verter_semantic::profile::QueryProfile>`                                           | YES          | F10    | **legitimate-authority** (policy state) |
| 17  | 373  | `project_type_store`           | `Arc<crate::project_type_store::ProjectTypeStore>`                                        | n/a (root)   | n/a    | the destination — not subject           |
| 18  | 377  | `request_id_counter`           | `AtomicU64`                                                                               | No           | n/a    | non-cache (counter)                     |
| 19  | 390  | `audit_records`                | `Arc<crate::component_meta_audit::AuditRecordsStore>`                                     | YES (inner Mutex<IndexMap>) | F11    | **legitimate-authority**                |
| 20  | 397  | `test_audit` (cfg-gated)       | `Arc<crate::host_test_audit::HostTestAuditState>` (under `cfg(test)`)                     | YES (cfg)    | n/a    | non-cache (test-only telemetry)         |

**Enumeration count:** 20 declarations (excluding `metrics` cfg-feature
gating doesn't change shape; `test_audit` cfg(test) gating is enforced by
the guard's `#[cfg(test)]` test-mode visibility).

**Cache-shape count:** 9 fields with cache shapes (rows 4, 7, 10, 12, 14,
15, 16, 19, plus 20 under cfg(test)).

**6b mirror count:** 0 mirror fields remain on `VerterHost`. F3 (resolver
inner Dbs) was Arc-share-rehomed in 6b.B2; F6 (`external_type_analysis_cache`)
and F7 (`route_owned_shallow_cache`) were deleted in 6b.D2a. Post-6b state
verified at lines 363–369 of `lib.rs` (comment block declaring F6/F7 deletion).

**6b legitimate-authority count on VerterHost:** 8 fields (rows 4, 7, 10,
12, 14, 15, 16, 19) — F12, F13, F1, F2, F4, F5, F10, F11.

### §2.2 Phase-6b deletion verification

Phase 6b's `mirror` classifications on `VerterHost`:

| 6b row | Field                            | Disposition          | Verification at integration tip                                                       |
| ------ | -------------------------------- | -------------------- | ------------------------------------------------------------------------------------- |
| F3     | `resolver.runtime.routes` / `.imported_roots` | Arc-share with ProjectTypeStore (Option (i)) | `lib.rs:373` `project_type_store: Arc<ProjectTypeStore>`; `routes_handle()` / `imported_roots_handle()` accessors verified `Arc::ptr_eq` in 6b.B2 commit `cb6f5bf1`. Resolver's inner Dbs are `Arc<RouteDb>` / `Arc<ImportedRootDb>` shared with the store. |
| F6     | `external_type_analysis_cache`   | DELETE — into `ProjectTypeStore.route_owned_shallow` | Field absent from `VerterHost` (verified by `grep`). Comment at `lib.rs:363` documents F6/F7 atomic deletion in 6b.D2a commit `c6e7fbeb`. |
| F7     | `route_owned_shallow_cache`      | DELETE — into `ProjectTypeStore.route_owned_shallow` | Field absent from `VerterHost`. Same comment block verifies. |

### §2.3 Out-of-`VerterHost` classifications from 6b

| 6b row | Owner struct                      | Field                       | Classification               | Verification                                                |
| ------ | --------------------------------- | --------------------------- | ---------------------------- | ----------------------------------------------------------- |
| F8     | `SessionRuntime`                  | `resolved_meta_cache`       | legitimate-authority         | Per-session overlay isolation; on `SessionRuntime` not `VerterHost`. Out of guard scope per `VerterHost`-only rule. |
| F9     | `HostFrontierAdapter`             | `route_shallow_cache`       | scratch (request-scoped)     | `RefCell`-shaped, request-lifetime. Out of guard scope per request-scratch rule (CLAUDE.md §0.6.6). |

These two are deliberately out of the static guard's scope: 6b classified
both as legitimate (F8 = session-overlay-isolation; F9 = request-scoped),
neither lives on a long-lived host struct that the project-global cache
rule covers. F8 lives on `SessionRuntime` (per-session value); F9 lives on
`HostFrontierAdapter` (per-request adapter). Neither is a `VerterHost`
field. The guard scope is `VerterHost` because that is the long-lived host
struct Phase 6b's rule targets and the brief's `no_off_store_host_caches`
guard names.

---

## §3 Phase 5l verification

`phase-05l-report.md` describes the atomic deletion of 13 deprecated
`ComponentMetaQueryEngine` resolver methods plus their 21 engine-internal
callsites. It is a `refactor/meta` deletion — no new caches were
introduced.

Spot check via `git log`:

```text
73f35740 style(meta): apply cargo fmt to 5l sources
67353fe2 test(meta): update phase_05m guard header marker for 5l rewrite
d9771acc test(meta): add Phase 5l engine-deletion regression guard
65e7c48d refactor(meta): atomic deletion of 13 deprecated engine resolver methods
76a5a759 refactor(meta): migrate engine-internal callers off deprecated methods (pre-deletion)
f32ed748 refactor(meta): rewrite bridge helpers to call dispatch + engine pub(crate) helpers directly
70abe885 refactor(meta): rewrite deep_resolve_type_refs to use direct dispatch
```

None of the commit subjects suggest a new cache field was introduced
between 5h and 5l. Phase 5l report's "Files touched" section names only
`meta_resolve.rs`, `component_meta_query_engine.rs`, and
`phase_05l_engine_deletion_tests.rs` (a new test module). No new struct
fields were declared on `VerterHost`. The post-5l cache-field set on
`VerterHost` is identical to the post-6b set.

---

## §4 Phase 6c verification

`phase-06c-report.md` describes deletion of `SchedulerBackedWorkspace`
shim. No new caches introduced. Phase 6c also un-ignored
`no_scheduler_backed_workspace_shim_in_session_src` — that guard now
runs un-ignored at the integration tip (verified by reading
`crates/verter_session/tests/architecture_guards.rs` end-to-end).

---

## §5 Phase 7 verification

`phase-07-report.md` describes consolidating LSP's local `vite_config`
module into `verter_workspace`. No `VerterHost` fields touched. No new
caches.

---

## §6 Static guard scope

The new `no_off_store_host_caches` guard targets:

- **`crates/verter_session/src/lib.rs`** — the `VerterHost` struct.

This is the scope §6b's classification covers. Other `*Host` and `*State`
structs in the workspace either:
- live on `ProjectTypeStore` already (e.g., `OwnerCollectionDb`,
  `MaterializeMemoDb`, the route/imported-root Dbs);
- are per-request value snapshots not subject to the rule (e.g.,
  `HostStoreView`);
- are wrappers without cache-shaped fields (e.g., `HostStageExecutor`,
  `ComponentMetaHost`/`ComponentMetaHostInner`);
- are explicitly classified `scratch` or `legitimate-authority` outside
  `VerterHost` by 6b (F8 `SessionRuntime`, F9 `HostFrontierAdapter`).

Extending the guard to additional host structs would re-litigate 6b's
scope: 6b's per-field algorithm explicitly stops at the named structs.
Per the brief's hard constraint — "Phase 6b's classifications are
BINDING. You do NOT re-litigate `legitimate-authority` decisions." —
the guard's scope is `VerterHost` exactly.

---

## §7 Allow-list construction

The guard's allow-list maps each `legitimate-authority` field name to a
phase-report citation:

| Field                       | Citation                                            |
| --------------------------- | --------------------------------------------------- |
| `alias_to_canonical`        | `phase-06b-report.md §F12: caller-supplied virtual-alias map populated at upsert time, disjoint from VFS overlay and from `ProjectResolver`. Host-scoped, no equivalent in `ProjectTypeStore`.` |
| `last_const_prop_overrides` | `phase-06b-report.md §F13: Phase-7 invalidation state-diff. NOT a cache of resolution results — a state-diff record. No equivalent in `ProjectTypeStore`.` |
| `compile_cache`             | `phase-06b-report.md §F1: per-profile compile state with sub-mirror lifecycle on `import_routes` (compile-event invalidation differs from file-content-event invalidation that drives `IndexedReady.import_routes`).` |
| `resolved_type_cache`       | `phase-06b-report.md §F2: shared external-type cache with profile-gated writes; bounded clear-all at `RESOLVED_TYPE_CACHE_CAP` (NOT LRU). Distinct from `SemanticGraphStore.HostResolvedNamedTypeKey` identity.` |
| `eval_env_cache`            | `phase-06b-report.md §F4: owned-data EvalEnv snapshots; consumers are host-local, no project-global sharing benefit. Migration to a hypothetical `ProjectTypeStore.EvalEnvDb` is unmotivated by current consumer patterns.` |
| `semantic_db`               | `phase-06b-report.md §F5: different crate, different artifact than `ProjectTypeStore.semantic_graph()`.` |
| `query_profile`             | `phase-06b-report.md §F10: execution-policy state, not a result memoiser. Different artifact type than anything in `ProjectTypeStore`.` |
| `audit_records`             | `phase-06b-report.md §F11: bounded insert-ordered store (`Mutex<IndexMap<u64, RustAuditRecord>>`, capacity 256, FIFO eviction via `shift_remove_index(0)`). Different artifact type than anything in `ProjectTypeStore`.` |

Allow-list size: **8 entries**.

---

## §8 Re-litigation check

Per the brief's STOP condition:

> A `legitimate-authority` field's 6b-cited rationale is no longer
> architecturally sound post-5l/4b → STOP, surface to user.

For each field in the allow-list, the 6b rationale was checked against
the post-5l + post-4b tree:

| Field                       | Post-5l/4b verification                                                                                                                                                                                |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `alias_to_canonical`        | Phase 5l deletes engine resolver methods; Phase 4b deletes `read_source` text-projection. Neither touches alias map machinery. Rationale stands.                                                       |
| `last_const_prop_overrides` | Phase-7 invalidation state-diff is unchanged — Phase 7 (vite-config consolidation) is unrelated; Phase 5l engine deletion does not touch the const-prop diff path. Rationale stands.                   |
| `compile_cache`             | Per-profile compile state is unchanged by 5l/4b. The `import_routes` sub-mirror lifecycle is unaffected by engine-method deletion. Rationale stands.                                                   |
| `resolved_type_cache`       | Phase 5l deletes `project_type_surface*`/`project_expr_surface*` engine trampolines but routes them through dispatch — `resolved_type_cache` is not on those code paths. Profile-gating discipline preserved. Rationale stands. |
| `eval_env_cache`            | Phase 5l/4b do not introduce new EvalEnv consumers. The "consumers are host-local" rationale is unchanged. Rationale stands.                                                                            |
| `semantic_db`               | The `verter_semantic::db::SemanticDb` artifact is unchanged by 5l/4b — those phases live in `verter_session` resolver-core, not `verter_semantic`. Rationale stands.                                    |
| `query_profile`             | Execution-policy state untouched. Rationale stands.                                                                                                                                                    |
| `audit_records`             | Audit subsystem untouched by 5l/4b. Rationale stands.                                                                                                                                                  |

**Conclusion:** Zero re-litigation triggered. No `legitimate-authority`
field's 6b rationale becomes unsound post-5l/4b. The audit's expected path
("ZERO fields needing rehome") holds.

---

## §9 New cache search

A targeted check for cache-shaped fields ADDED on `VerterHost` since 6b:

```bash
git log -p --all 3147c02f..HEAD -- crates/verter_session/src/lib.rs \
  | grep -E '^\+\s*pub\(crate\)\s+\w+:\s*(DashMap|RwLock|Mutex|FxHashMap|HashMap)'
```

Result: zero new cache-shaped fields added on `VerterHost` between Phase
6b's foundation HEAD `3147c02f` and the integration tip `e2f41a00`. The
intervening commits across 5h–5l, 6, 6b, 6c, 7, and 4b touch
`VerterHost` only for:
- The annotation pass in 6b.A (commit `79fbad38`) — pure doc-only.
- The 6b.D2a deletion of F6/F7 — net field removal (commit `c6e7fbeb`).
- 6b.D2b host wrappers (`notify_close`, `notify_upsert`,
  `set_exact_resolutions`) — added methods, no new fields (commit `5ced1e8f`).

---

## §10 Audit conclusion

- Phase 6b's classification of `VerterHost` is internally consistent
  with the post-5l + post-4b tree.
- The 8 `legitimate-authority` fields each carry a 6b-cited rationale
  that survives post-5l/4b.
- F3 / F6 / F7 mirror deletions are verified absent from the integration
  tip.
- No new cache-shaped fields were introduced on `VerterHost` since
  Phase 6b.
- Zero rehoming commits required. The expected path ("audit reveals
  zero fields needing rehome") is the actual path.

Phase 8 therefore proceeds directly to:
1. This audit document (commit 1).
2. The static guard `no_off_store_host_caches` with the 8-entry
   allow-list above (commit 2).
3. The marker (commit 3).

No rehoming commits are required. No deferrals. `deferred[]` is empty.

End of audit.
