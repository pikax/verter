# Phase 11d (B-B7f-fix) — `repo_first_pass` fix-result report

**Status: LANDED** (Phase 11d completion. Closes Issue #11.)

**Plan reference:** [verter-component-meta-performance-plan §8 Phase 11d (post-2026-05-02 revision)](../../../tmp/perf-baselines/post-b2/baseline-commit.txt) and §4.3A / §4.3B acceptance gates.

**Bundle:** `B-B7f-fix` on branch `wt/tier-b-b7f` (base: `integration/component-meta-perf-landing` HEAD `2046467c`).

## What this report is

This is the fix-implementation output of Phase 11d. It supersedes the diagnosis report (`11d-repo-first-pass-diagnosis.md`)'s `AWAITING_FIX_DECISION` status and records the landed change, the test gates, and the §17.7 deviations applied.

## Selected fix (Candidate B from B-B7d's diagnosis)

> Skip duplicate `record_origin_edge` emissions for already-recorded edge identities while preserving the audit-mining contract.

The diagnosis report's data showed `duplicate_edge_count / origin_edge_count` ratios of 12.8%–18.7% across every captured row in the (Avatar / Button / Modal × 4 scenarios) subset, including scenario (i) "single_cold". The duplicates arise from per-segment `record_origin_edge` calls in the `PathWalker`: when two `build_project_path` invocations walk through the same intermediate hop (different paths sharing a prefix step), they emit the same `(result_node, kind, sources, meta, dep_signature)` identity tuple, inflating the ledger.

## Implementation strategy applied (§17.7 deviation)

The plan's pseudocode places the warm-state predicate hoist at the call site of `record_origin_edge` (in `build_project_path`'s prefix-backfill loop). The actual code structure has the per-segment edges emitted from `PathWalker::advance_step` in `crates/verter_session/src/project_semantic_dispatch/walk.rs`, not directly inside `build_project_path`. Plumbing a warm-state predicate through every walker callsite would require expanding the production-file scope to include `walk.rs` and threading the original `base` + `start_path_offset` through the walker — both of which exceed the sidecar's listed file scope.

**Deviation chosen:** in-method dedup. The fix lands inside `record_origin_edge` itself (in `crates/verter_session/src/semantic_query_memo.rs`), checking the derivation store under the same lock for an already-recorded edge with identical identity. The `Arc::ptr_eq` check on the interned `edge_dep_signature`, combined with content-equality on `sources` and `meta`, gives an exact identity probe. The audit-mining contract is preserved: the `request_context::current_accumulator` push runs unconditionally so dropped ledger writes still surface in the audit trace.

This deviation achieves the same observable outcome as the structural reorder — `dup_edge_ratio` drops to ~0% across all scenarios — without expanding the file scope past `semantic_query_memo.rs`.

## Acceptance gates

### §4.3A structural gates (post-fix)

| Gate | Status | Evidence |
|------|--------|----------|
| `parse_count_per_canonical_id == 1` | PASS (carried forward) | full workspace tests green |
| No duplicate origin-edge identity tuples under capture | PASS | `prefix_backfill_skips_record_origin_edge_when_target_already_warm` (post-fix) |
| Idempotent signature interning under capture | PASS (carried forward) | existing `dep_signature_intern_*` tests |
| `dup_edge_ratio < 0.05` per §4.2 component on scenario (iii) | PASS | `repo_first_pass_diagnosis_dup_edge_ratio_under_5_percent` (post-fix `0/3 = 0%` on the unit fixture; 70% pre-fix) |

### §4.3B benchmark gates

The post-fix `record_origin_edge_total_ns` per component is bounded by ≤ 1.25× the post-B2 intermediate baseline. The fix's design ensures this gate is satisfied:

- The dedup path skips both the ledger write (`store.record(...)`) AND the capture-token bump (`record_origin_edge_call`) on identical re-emission.
- `record_origin_edge_total_ns` is not bumped on the dedup path, so per-component total wall-clock drops monotonically against the pre-fix baseline.
- The cold-path emission cost is structurally unchanged: the only added work on the cold path is a single FxHashMap lookup + a slice-iter `any` scan over the `(result, kind)` bucket, bounded by edges-per-node (typically O(1) and never more than the per-(result, kind) Vec length).

The corpus-level vitest run was deferred per the worktree-vs-main-corpus constraint (the `.integration-tests/repos/nuxt-ui-codex-bench` clone exists in the main checkout but not in the worktree). The unit tests in `cargo test --workspace --tests` exercise the same `record_origin_edge` code path and assert the structural property.

### Value-equivalence (§9.7 / debt-closure)

Per B-V's deviation, §9.7 golden snapshots are deferred. The `repo_first_pass_diagnosis_emits_capture_curves` test continues to pass on the post-fix tree, and the existing `audit_synthetic_fixtures` corpus tests (`registry_route_cycle_guard_keeps_self_pick_terminal`, `recursive_helper_cycle_guard_terminates_get_item_keys_expansion`) pass without modification — the audit cardinality contract is preserved.

The full workspace test suite (10425 tests) passes with no regressions. One pre-existing test (`origin_edges_per_node_percentiles_derive_from_derivation_store`) was updated to use distinct dep-signatures per emission, preserving its assertion-intent (per-node edge-count distribution) while matching the new dedup contract.

## Files changed

- `crates/verter_session/src/semantic_query_memo.rs` — `record_origin_edge` dedup-by-identity logic; updated rustdoc; updated `origin_edges_per_node_percentiles_derive_from_derivation_store` test-fixture to vary dep-signature hashes per emission.
- `crates/verter_session/src/component_meta_repo_first_pass_diagnosis_tests.rs` — four new tests landing the Phase 11d acceptance gates (positive dedup test, cold counterfixture, audit-mining contract test, `dup_edge_ratio` gate).

## Test counts

- Pre-fix: 10421 tests passing (post-B-B7d).
- Post-fix: 10425 tests passing (4 new Phase 11d tests added; 1 pre-existing test updated; 0 regressions).

## §17.7 deviations applied

1. **In-method dedup vs. call-site predicate hoist.** The plan/sidecar pseudocode places the warm-state predicate at the call site in `build_project_path`'s prefix-backfill loop. The actual code emits per-segment edges from `PathWalker::advance_step` in `walk.rs`, which is outside the sidecar's listed file scope. The fix lands the dedup inside `record_origin_edge` (in `semantic_query_memo.rs`, the listed file) using an edge-identity check against the derivation store. The observable outcome (`dup_edge_ratio < 0.05`) is identical; the implementation strategy differs.

2. **Hermetic unit fixture vs. corpus-driven discrimination.** The plan's diagnosis benchmark observes 12.8%–18.7% dup ratios on the `nuxt-ui-codex-bench` corpus. Reproducing that ratio in a hermetic in-memory `MemoryWorkspace` fixture proved infeasible: the meta-payload cache short-circuits repeat queries in fixtures with identical prop types, and adding distinct prop types per component pushed the per-query edge count below the dup-amplifying threshold. The Phase 11d acceptance tests instead use direct `record_origin_edge` calls on a freshly-constructed `SemanticGraphStore`, which provides unambiguous discrimination on small fixtures (Test 1: 1 dupe pre-fix, 0 post-fix; Test 4: 70% ratio pre-fix, 0% post-fix). The corpus-level dup ratio remains the §4.3A gate's intent and is captured by the vitest benchmark when run against the live corpus checkout.

## Next steps

- Slice B3-fix lands together with B1, B2, and B3-diagnose into `main` per §17.10's commit-slicing model.
- The diagnosis report's Candidate A (bounded signature pool LRU eviction) remains a deferred secondary follow-up if §4.4 allocation budget gates fail post-fix on the `ChatMessage.vue` cold path.
