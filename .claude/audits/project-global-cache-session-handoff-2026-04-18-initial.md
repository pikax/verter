# Project-Global Cache Overhaul — Session Handoff

**Session date:** 2026-04-18
**Branch:** `refactor/semantic-db-overhaul`
**Plan:** the component-meta project-global-cache overhaul plan (developer-local Claude plans dir)

## What landed in this session

One `VerterHost`-owned cache root, eight new modules, and an IndexedReady
publication alongside the legacy `ModuleFactsDb` insertion site. **All
code compiles, zero clippy warnings on the new code, 1375 `verter_session`
unit tests and 9707 workspace tests pass.** No call sites have been
migrated onto the new types yet — the scaffolding exists and is observable
through the `VerterHost::project_type_store()` accessor, but the
component-meta / type-resolution hot path still routes through
`ModuleFactsDb`, `RequestStoreView`, and `CURRENT_REQUEST_VIEW`.

### Commits

1. `db55959b` feat(session): introduce project-global cache type foundation
2. `a71acc9c` feat(session): publish IndexedReady alongside ModuleFactsDb (Phase 0a+1)
3. `3278e7b9` feat(session): add Phase 2/2.1/2.2/3 scaffolding for project-global cache

### Modules added (all in `crates/verter_session/src/`)

| Module                      | Phase | Role                                                                      |
|-----------------------------|-------|---------------------------------------------------------------------------|
| `semantic_query.rs`         | 2.2   | `SemanticQueryKey` enum, `SemanticQueryApi`, `TypeNavigator`, supporting types |
| `semantic_query_memo.rs`    | 2.2   | Host-owned memo table with per-entry Mutex+Condvar cooperative wait       |
| `project_type_store.rs`     | 1     | `ProjectTypeStore`, `ArtifactRequirements`, `IndexedReady`, `AnalysisReady` |
| `completion_fence.rs`       | 1     | `CompletionFence` (3-attempt bound, dep-signature merge)                  |
| `owner_import_surface.rs`   | 2     | `OwnerImportSurfaceDb` + builder                                          |
| `component_meta_result_db.rs` | 3   | `ComponentMetaResultDb<P>` keyed by (owner, hash, kind, options)          |
| `intrinsic_registry.rs`     | 2.1   | `IntrinsicRegistry` + `lib*.d.ts` scanner                                  |
| `project_global_cache_tests.rs` | 0a | Contract tests (21 running + 3 phase-gated ignores)                       |

`VerterHost` now owns `Arc<ProjectTypeStore>` and exposes it via
`.project_type_store()`. `ProjectTypeStore` in turn owns:

- `IndexedReadyDb` (Phase 1) — populated by the same upsert path that
  populates `ModuleFactsDb` (shares the `shallow_state` and `import_routes`
  Arcs by construction, so both caches agree on the canonical post-parse
  artifact for the transitional coexistence window).
- `AnalysisReadyDb` (Phase 1) — scope-keyed with bitflag-based satisfaction
  lookup (`find_satisfying`).
- Rehomed `RouteDb` (Phase 1).
- Temporary `ImportedRootDb` (collapses to transitive-only use in Phase 2,
  folded into route/semantic-query layer in Phase 5).
- `SemanticGraphStore` (Phase 2.2) — node arena + memo table + in-flight
  admission with cooperative waits.
- `OwnerImportSurfaceDb` (Phase 2).
- `ComponentMetaResultDb<FinalComponentMetaPayloadPlaceholder>` (Phase 3).
- `IntrinsicRegistry::with_defaults()` (Phase 2.1).
- Per-layer debug counters (`ProjectTypeStoreCounters`).

### What *is* observable today

- Upsert-then-query populates both `ModuleFactsDb` and `IndexedReadyDb`
  with identical content.
- Unrelated files stay warm across an edit to one file.
- Stale `whole_hash` lookups miss at the key level without any
  request-view identity.
- `ComponentMetaResultDb` distinguishes `Native` vs `Compat` query kinds,
  different options fingerprints, and different owner hashes without
  aliasing.
- `IntrinsicRegistry` refuses to treat userland aliases as intrinsics —
  lookup only fires after a declaration resolves to `= intrinsic`.
  Scanner round-trip from `lib*.d.ts` samples works.
- `SemanticGraphStore::execute_cooperative` memoizes one cold build per
  key, handles same-path recursion with a sentinel, blocks cross-thread
  joiners on a condvar, and refuses to warm errors or recursion sentinels.
- `CompletionFence::run` retries at most 3 times and publishes
  `FenceOutcome::Unstable { attempts: 3 }` instead of looping forever.

### What is *not* migrated yet (remaining Phase 2/3/4/5 work)

The scaffolding is in place, but the hot path still routes through
`ModuleFactsDb`, `RequestStoreView`, and `CURRENT_REQUEST_VIEW`. The
following are the concrete remaining deliverables from the plan:

#### Phase 2 — migrate direct-owner-import callers

- `crates/verter_session/src/host_manage.rs` —
  `resolve_imported_type_root_in_view` callers switch to
  `OwnerImportSurfaceDb`.
- `crates/verter_session/src/host_resolve.rs` — direct-import path.
- `crates/verter_session/src/resolver_core/component_meta_registry.rs` —
  `owner_component_meta_registry_import_root` and adjacent stages.
- `crates/verter_session/src/resolver_core/solver_host.rs` — solver-host
  binding resolution.
- `crates/verter_session/src/resolver_core/external_type_body.rs` —
  delete the `resolved_roots` field.
- Reduce `ImportedRootDb` to transitive-only consumers.

Expected trace envelope after Phase 2 (per plan § H):
- Accordion cold: `owner_import_surface_build == 1`,
  `canonical_bundle_build ≈ 10`, `resolve_imported_type_root == 0`.

#### Phase 2.1 — intrinsic audit wiring

- Wire the repo-SDK audit into `cargo test` as a dedicated `#[test]`
  that scans the active TypeScript SDK's `lib.es5.d.ts` /
  `lib.es2015.d.ts` / `lib.es2022.d.ts` files via
  `extract_intrinsics_from_lib_source` and fails with
  `audit_unsupported`.
- Add a maintenance `#[test]` (`#[ignore]` by default) that does the same
  against `typescript@latest` node module — opt-in via `cargo test --
  --ignored intrinsic_latest_audit`.
- The resolver's existing string-case-intrinsic handling in
  `verter_semantic::analysis::type_solver` needs to dispatch through
  `IntrinsicRegistry::lookup` instead of name-matching.

#### Phase 2.2 — bind the memo to real solver work

- Implement a `SemanticQueryApi` impl on `ProjectTypeStore` that routes:
  - `ResolveDecl` → existing `prepare_exported_type_decl` / route layer
  - `Instantiate` → existing solver instantiation path
  - `ProjectMember` / `IndexedAccess` — use `TypeNavigator` for
    intermediate hops, enter the query API only for new semantic nodes
  - `Expand` → existing `ExternalTypeFrontier` expansion behind the memo
- Retire the per-request `external_inputs_memo` and `eval_state_memo` on
  `RequestStoreView` in favour of the shared memo.
- Populate the memo's dep-signature from the sites currently writing to
  `ValidatedFactCache` fact lists.

#### Phase 3 — wire the final-result cache

- `crates/verter_session/src/host_manage.rs` `get_component_meta()`:
  1. Create a `CompletionFence`.
  2. Compute `options_fingerprint` from a manually-versioned
     `ComponentMetaOptions` struct via `xxhash_rust::xxh3::xxh3_128`.
  3. Check `ComponentMetaResultDb`; if present, revalidate its
     `DepSignature` via a new `FenceValidator` impl over the live host
     whole-hashes / route generations / project generation.
  4. On miss, run the existing resolver under the fence, observing the
     dep signature as it reads caches, then publish atomically.
  5. On `FenceOutcome::Unstable { attempts: 3 }`, surface as a structured
     `QueryError::UnstableState` to the caller.
- Replace the `FinalComponentMetaPayloadPlaceholder` with the real
  native payload type — today that lives in
  `component_meta::ResolvedComponentMetaParts`.
- Add a warm-rerun test: call `get_component_meta(owner)` twice against
  one `VerterHost`, assert the second call hits the cache (counter delta
  == 0 for `canonical_bundle_build` / `owner_import_surface_build`).

#### Phase 4 — cut the query engine to scratch-only

- `crates/verter_session/src/resolver_core/component_meta_query_engine.rs`
  (8891 lines) — delete every `Option<&RequestStoreView>` / `*_in_view`
  signature in the component-meta/resolver hot path as one atomic
  compile-green signature rewrite. Keep only the query-local mutable
  scratch documented in plan § F.
- Delete `host_owned_resolved_named_types`, `external_inputs_memo`,
  `eval_state_memo`, `host_request_view::extension_store`,
  `host_request_view::RequestViewGuard`,
  `host_request_view::current_request_view`,
  `host_request_view::effective_request_view`,
  `host_request_view::EffectiveView`.
- Add the source-audit test required by plan § Phase 4:
  ```rust
  #[test]
  fn component_meta_hot_path_no_longer_references_request_store_view() {
      let hot_path = [
          include_str!("resolver_core/component_meta_query_engine.rs"),
          include_str!("resolver_core/component_meta_registry.rs"),
          include_str!("resolver_core/solver_host.rs"),
          include_str!("meta.rs"),
          include_str!("host_manage.rs"),
      ];
      for source in hot_path {
          assert!(!source.contains("RequestStoreView"));
          assert!(!source.contains("CURRENT_REQUEST_VIEW"));
      }
  }
  ```

#### Phase 5 — delete legacy + update docs + archive correctness audit

- Delete `crates/verter_session/src/host_request_view.rs`.
- Delete `crates/verter_session/src/resolver_core/module_facts_db.rs`
  once the last consumer is migrated (confirm with
  `grep -r 'ModuleFacts\b' crates/verter_session/src` returning zero).
- Delete the Phase-0a `transitional_module_facts_db_coexists_with_indexed_ready`
  test.
- Update `CLAUDE.md` (§ Canonical Dependency Cache Rule,
  § Shallow File Processing Core Invariant), `/type-resolution`,
  `/component-meta`, `/host-session` skills with the new authority chain.
- Run the mandatory old-vs-new correctness audit per plan § J:
  replay the component-meta trace corpus through both paths (keep the
  old path compiled behind a transient test-only feature if needed),
  diff native payloads byte-for-byte, archive the result to
  `.claude/audits/project-global-type-rewrite-correctness-audit.md`.
- Delete the transient test-only feature flag in the same commit.

### Verification gates — status

| Gate                                                            | Status                          |
|-----------------------------------------------------------------|---------------------------------|
| `cargo test --package verter_session frontier_tests`            | PASS (30 tests)                 |
| `cargo test --package verter_session host_manage`               | PASS (211 tests)                |
| `cargo test --workspace --tests`                                | PASS (9707 tests, 0 failures)   |
| `cargo clippy -p verter_session` (lib only)                     | PASS (zero warnings)            |
| `cargo clippy --workspace`                                      | PASS (zero new warnings)        |
| `pnpm test`                                                     | not run this session            |
| `pnpm integration-test --skip-baseline --no-clone ...`          | not run this session            |
| Component-meta trace corpus                                     | not run this session            |
| Warm-rerun verification                                         | pending Phase 3                 |
| Correctness audit                                               | pending Phase 5                 |

### Why the full cutover is still outstanding

The plan explicitly budgets Phases 2.1, 2.2, 4, and 5 as major
subprojects — Phase 4 alone is "the large mechanical signature-cut phase"
across a ~9k-line engine module plus every `_in_view` helper. A single
Claude session with 1M context and tight formatting + test turnaround
produced the type surface + memo + Phase-0 contract tests + cache
publication wiring, which is the correct first slice: it locks the
API shape and contract while the migration work proceeds.

The scaffolding intentionally *preserves* `ModuleFactsDb`,
`RequestStoreView`, and the `*_in_view` signatures — removing them before
the migration completes would leave the hot path broken and no tests
could be run. They get deleted in Phase 5 after the correctness audit.

### Concrete entry points for the next session

1. **Start with Phase 2** — `OwnerImportSurfaceDb` migration. Lower-risk
   than Phase 4 because each consumer site can be migrated independently
   and test-verified. See target files listed under Phase 2 above.
2. **Then Phase 3** — `ComponentMetaResultDb` wiring. Small surface (one
   top-level entry point: `get_component_meta`) but highest leverage for
   warm-rerun perf.
3. **Phase 2.1** — intrinsic audit. Fastest to implement; isolated.
4. **Phase 2.2** — semantic query binding. Requires understanding the
   existing solver paths before routing them through the memo.
5. **Phase 4** — atomic signature cut. Largest surgical change.
6. **Phase 5** — deletions + docs + audit.
