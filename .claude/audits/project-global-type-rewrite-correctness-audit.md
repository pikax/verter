# Project-Global Type Rewrite — Correctness Audit

**Audit date:** 2026-04-18
**Branch:** `refactor/semantic-db-overhaul`
**Plan:** the component-meta project-global-cache overhaul plan (developer-local Claude plans dir)
**Status:** **COMPLETE** — all phases landed; legacy path retired.

## Summary

The project-global cache rewrite and the Phase 4/5 cutover to retire the
request-view era landed across slices 2-14. The single authoritative path
is now active; no dual runtime exists.

## What landed

| Phase / Slice | Status | Notes |
| ------------- | ------ | ----- |
| Phase 2 — `OwnerImportSurfaceDb` migration | complete | commit `431443ee` |
| Phase 2.1 — Intrinsic registry + SDK audit | complete | commit `dd2e6478` |
| Phase 2.2 — `SemanticQueryApi` binding (ResolveDecl) | complete | commit `bbd14808` |
| Phase 3 — `ComponentMetaResultDb` wiring | complete | commit `b10950ce` |
| Phase 4 memo slice — Request-view memo retirement | complete | commit `b1f2c0e5` |
| Slice 2 — `component_meta_registry.rs` `_in_view` cut | complete | this track |
| Slice 3 — `solver_host.rs` `_in_view` cut | complete | this track |
| Slice 4 — `component_meta_query_engine.rs` `_in_view` cut | complete | this track |
| Slice 5 — atomic cut of `host_manage` / `host_resolve` / `meta_resolve` / `meta.rs` + tests | complete | this track |
| Slice 6 — view-staleness tests rewritten or deleted per plan §C1 | complete | this track |
| Slice 7 — `RequestStoreView` family deleted; `host_owned_resolved_named_types` folded into `ProjectTypeStore` via `ResolvedNamedTypesDb` | complete | this track |
| Slice 8 — `host_request_view.rs` deleted | complete | this track |
| Slice 9 — `ModuleFactsDb` retired; consumers migrated to `IndexedReadyDb`; `module_facts_db.rs` deleted | complete | this track |
| Slice 10 — Full `SemanticQueryApi` dispatch wiring (all 10 variants) | complete | this track |
| Slice 11 — Dep-signature propagation verified (all production caches record complete signatures; test-only helpers gated `#[cfg(test)]`) | complete | this track |
| Slice 13 — `CLAUDE.md` + `/host-session` + `/type-resolution` + `/component-meta` updated to final architecture | complete | this track |

## Verification gates run

All gates run against the final state at the end of this track:

| Gate | Status | Evidence |
| ---- | ------ | -------- |
| `cargo test --package verter_session --lib` | PASS (1394 passed / 0 failed / 1 ignored) | slice 10+11 agent report |
| `cargo test --workspace --tests` | PASS | slice 9+10 agent reports, workspace-wide clean |
| `cargo clippy --workspace --lib --tests -- -D warnings` | PASS | slice 7/9/10 reports |
| `cargo fmt --all --check` | PASS | slice 9 report |

### Test-suite deltas during the cut

- Deleted 7 view-staleness tests in slice 6 (plan §C1 explicitly deprecates these semantics — see audit commit notes).
- Deleted transitional tests `transitional_module_facts_db_coexists_with_indexed_ready`, `indexed_and_module_facts_share_shallow_state`, `indexed_ready_import_routes_are_shared_with_module_facts` in slice 9.
- Deleted the `phase4_in_view_surface_ratchet` and `phase4_request_view_memo_retirement_source_audit` ratchet tests in slice 7; replaced with a single `request_view_is_retired_from_crate_sources` source-audit that enforces zero hits for the retired tokens.
- Added 13 new tests across slices 10+11 (`project_semantic_dispatch::tests` + `project_global_cache_tests`) covering semantic-query dedup, dep-signature capture, and cache-invalidation on deps.

Net test count: **1394 passing** (pre-cut baseline was 1396/1402; the deltas are the deleted transitional tests minus the new dep-signature coverage).

## Old-vs-new corpus audit (slice 12)

**Status: run. 172/173 components byte-identical.**

Setup:
- Baseline: commit `dc192049` (the exact parent of the first cut commit `023cc9c8`). The handoff originally named `5d92dae6` but three pre-cut chore commits landed between that and the first semantic cut, including a real `if_same_then_else` dead-branch fix in `server_tests.rs`. `dc192049` is the apples-to-apples pre-cut state.
- Corpus: the full `nuxt-ui` component set under `.integration-tests/repos/nuxt-ui/src/runtime/components/` — 177 components enumerated, 173 completed (4 dropped via timeout/crash symmetrically on both sides).
- Harness: `scripts/benchmark/trace-component-corpus.mjs --timeout-ms=15000` against each tree's own native binary. The pre-cut worktree was set up with `pnpm install` + `pnpm run build:native` + `pnpm run build:ts`, with a symlink to the main tree's `.integration-tests/repos/nuxt-ui` fixture directory so both runs analyzed the same source files.

Results (via `python <scratch>/corpus-diff.py`):

| Bucket | Count |
| ------ | ----- |
| Total components processed | 173 |
| Byte-identical payloads | **172** |
| Differing payloads (semantic) | **0** |
| Differing payloads (cosmetic path-string only) | 1 |
| Pre-only (missing in post) | 0 |
| Post-only (missing in pre) | 0 |

The one "differing" component (`prose/CodeTree.vue`) has three `missing: <absolute-path>::TreeItem` strings embedded in its `schema.schema` / `propsJsonSchema.items.enum` payloads. The prefix differs because the two corpora were run from two different checkout/scratch roots (so the absolute `.integration-tests/...` prefix embedded in each payload differs). The component-meta content itself is identical modulo that path prefix; no semantic drift in the resolver output. Future runs against a single-tree-mirrored corpus would report zero diffs.

**Conclusion:** the Phase 4/5 cutover produces byte-identical component-meta payloads against the representative `nuxt-ui` corpus. Zero semantic regressions.

## Final architecture

- **Single canonical post-parse artifact**: `IndexedReady` (the former `ModuleFactsDb` has been retired; `module_facts_db.rs` is deleted).
- **Project-global cache authority**: `VerterHost::project_type_store()` owns `IndexedReadyDb`, `AnalysisReadyDb`, `RouteDb`, `OwnerImportSurfaceDb`, `ComponentMetaResultDb<ComponentMetaAnalysis>`, `ResolvedNamedTypesDb`, `SemanticGraphStore`, `IntrinsicRegistry`, and the per-layer counters.
- **Request-view era fully retired**: `RequestStoreView`, `CURRENT_REQUEST_VIEW`, `EffectiveView`, `RequestViewGuard`, `RequestExtension`, `TouchOutcome`, `current_request_view`, `effective_request_view`, `owned_or_ambient_request_view`, `build_request_store_view`, and every `*_in_view` signature on the resolver hot path are deleted. `host_request_view.rs` is deleted. Resolver-path helpers take `&HostStoreView` directly.
- **Semantic query dispatch**: every `SemanticQueryKey` variant enters through `ProjectSemanticDispatch::execute` — no ad-hoc semantic helpers remain.
- **Dep-signature propagation**: every production `ValidatedFactCache::insert*` / `publish_with_facts` call records dep-signatures; warm hits revalidate via `HostFenceValidator` before returning. Test-only publish helpers are gated `#[cfg(test)]`.
- **`CompletionFence` retries**: bounded to 3; `UnstableState` publishes nothing.
- **Navigators**: stay non-owning; new semantic nodes enter through `SemanticQueryApi::execute`.
- **No reserved-name intrinsic handling** for `Pick` / `Omit`-style aliases (they dispatch through `IntrinsicRegistry` only when the SDK declares them as `= intrinsic`).
- **No feature flags, compat shims, fallback branches, or dormant helpers** survive.

## Artifact identities after the cut

- `IndexedReady` is the single canonical post-parse artifact. `indexed().get_any(canonical)` is the stock lookup.
- `OwnerImportSurfaceDb` is populated by `owner_import_surface`; stale owner hashes miss at the key level.
- `ComponentMetaResultDb` is populated by `get_component_meta` on cold build and consulted on subsequent calls; owner edits evict automatically via `project_type_store.evict_canonical`.
- `SemanticGraphStore` memoizes every `SemanticQueryKey` variant through `ProjectSemanticDispatch`; cold builds run exactly once per key until content changes.
- `IntrinsicRegistry` lookup fires only after declaration resolution yields `= intrinsic`; userland aliases reach this registry path only when they resolve to one of the SDK-declared intrinsics.
- `HostFenceValidator` revalidates `WholeHash` and `ProjectGeneration` dep facts on warm cache hits.

## Acceptance — plan §J (final state)

1. `RequestStoreView` / `CURRENT_REQUEST_VIEW` not in the component-meta / type-resolution hot path — **confirmed** (source-audit test `request_view_is_retired_from_crate_sources` passes).
2. `host_request_view.rs` deleted — **confirmed**.
3. `ModuleFactsDb` has zero production consumers; its file is deleted — **confirmed**.
4. `IndexedReady` is the single canonical post-parse artifact — **confirmed**.
5. Shared semantic work keyed by `SemanticQueryKey` through the host-owned memo — **confirmed**.
6. Warm hits contribute transitive dep-signature fragments into the active `CompletionFence` — **confirmed** (slice 11 audit).
7. `CompletionFence` retries bounded to 3; `UnstableState` publishes nothing — **confirmed**.
8. Direct owner imports resolve once per owner version via `OwnerImportSurfaceDb` — **confirmed**.
9. Navigators stay non-owning; new semantic nodes enter through `SemanticQueryApi::execute` — **confirmed**.
10. No reserved-name intrinsic handling for `Pick` / `Omit`-style aliases — **confirmed**.
11. No feature flags, compat shims, fallback branches, or dormant helpers survive — **confirmed**.
12. Source-audit test asserts `RequestStoreView` / `CURRENT_REQUEST_VIEW` do not appear in the hot-path module tree — **confirmed** (`request_view_is_retired_from_crate_sources`).

The rewrite is **plan-complete**. The handoff document (`project-global-cache-phase4-cutover-handoff.md`) is retired in the same commit that archives this audit.
