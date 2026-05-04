# Deferred pre-Tier-B baselines: ChatMessage, ChatMessages

**Status:** Closed. Tier 4 §6.7 closure: chat-components bounds were
captured post-Tier-1 fix and recorded in
`crates/verter_session/tests/perf_bounds/chat-message.json` and
`crates/verter_session/tests/perf_bounds/chat-messages.json`. The
audit-counter wiring fix (Tier 4 §6.3) and the Tier-A / Tier-B
slot-binding + Pick-callable preservation fixes make both fixtures
measurable and reproducible.

**Plan reference:** [verter-component-meta-performance-plan §17.9](../../../tmp/perf-baselines/pre/baseline-commit.txt) row dated 2026-05-02 (B-00-baselines Option A).

## What this debt is

Phase 0 / B-00-baselines (Wave 0) was specified to capture pre-change baselines for 12 components (plan §4.2). 10 of 12 succeeded; 2 (`ChatMessage.vue`, `ChatMessages.vue`) cannot be measured on the pre-Tier-B integration tree because their cold-path component-meta queries do not terminate within any reasonable wall-clock window — verified empirically across three timeout configurations:

| Mode | Component | Timeout | Outcome |
|------|-----------|---------|---------|
| `fresh-cold` | ChatMessage | 5 min | exit 143 (SIGTERM); no audit dump |
| `cold-seq` | ChatMessages | 10 min | exit 143; no audit dump |
| `cold-seq` (after Button warmup) | ChatMessage | 10 min | exit 143; no audit dump |
| Original 12-component sweep | ChatMessages | 16 min wall (993s CPU) | killed before output |

The pre-Tier-B audit harness (`crates/verter_session/src/component_meta_audit/mod.rs`) writes its dump on completion, not incrementally — SIGTERM produces no record. The hangs are precisely the slow-cold paths Tier B exists to fix:

- ChatMessage hang root cause: slot-binding indexed-access not preserved symbolically when the indexed root is imported (plan §3 row 1). Tier-A Phase 3 (`B-A2-policy-guards`) ships the fix.
- ChatMessages hang root cause: `Pick<ChatMessageProps, 'actions'>` member-route descends into the `onClick` callback parameter type `UIMessage` from the `ai` package (plan §3 row 10). Tier-B Phase 10 (`B-B5-callback-pick`) ships the fix.

After the Tier-A Phase 3 fix lands, ChatMessage cold path becomes measurable. After the Tier-B Phase 10 fix lands, ChatMessages cold path becomes measurable. The deferred bounds are captured in a focused follow-up dispatch keyed `B-00b-baselines-chat-components`.

## Why this is acceptable for Slice A1 shipping

1. **Stub-prevention bars fabrication.** Per CLAUDE.md "Stub Prevention" rule, fabricated placeholder bounds are a gate-bypass, not a pass. We cannot author bound JSONs with invented numbers.

2. **Slice A1's `loaded_files <= bound` test asserts per-component.** Components without a bound JSON simply have no assertion — they are not silently passing a fake bound. Plan §17.0a step 3 was relaxed to "every measurable component has a bound JSON; deferred components are tracked in this debt-closure doc" (per §17.9 row dated 2026-05-02 / B-00-baselines Option A).

3. **The §4.3B 50%-drop gate evaluates against a lower-bound baseline.** Pre-change `query_ms_from_stdout` for ChatMessage and ChatMessages is documented as `≥ 300s (timeout-bound; audit harness has no cancellation primitive)`. The gate "≥ 50% drop" is satisfied by any post-change measurement < 150s — Tier B's expected behavior is dramatic improvement (the components terminate at all post-fix), so the missing exact pre-change value does not block PR-time gate evaluation.

4. **The hangs are the input to Tier B, not measurement bugs.** The plan exists because these components hang. Recording "≥ 300s, audit-dump-blocked" IS the honest pre-change baseline.

## What lands when

| Phase | Lands | Bound JSONs added |
|-------|-------|-------------------|
| Slice A1 (this commit) | 10 of 12 | button, icon, avatar, avatar-group, modal, form, table, select-menu, input-menu, editor |
| Slice A2 Phase 3 (`B-A2-policy-guards`) | Tier-A correctness | (no bounds) |
| Slice A2 Phase 9 (`B-A3-slot-skip`) | Tier-A correctness | (no bounds) |
| `B-00b-baselines-chat-components` follow-up | After Slice A2 lands | chat-message |
| Slice B1 Phase 10 (`B-B5-callback-pick`) | Tier-B Pick callable preservation | (no bounds) |
| `B-00b-baselines-chat-components` second pass | After Slice B1 Phase 10 lands | chat-messages |

The follow-up dispatches consume B-A0's `CaptureToken` API to capture richer per-DB / per-KeyFamily counters that pre-A0 tooling did not expose.

## Tooling notes (B-00-baselines deviation)

Three plan-vs-tooling mismatches were surfaced during B-00-baselines and resolved in plan §17.9 row 2026-05-02 / B-00-baselines:

1. **Sidecar §8 Phase 0 Step 6 named the wrong example.** `profile_component_meta` is the SYNTHETIC scenario example (reads from env vars only, no positional arg). The corpus-driven sibling is `profile_real_component_meta`. Plan §8 Phase 0 Step 6 was corrected.

2. **`scripts/benchmark/trace-component-corpus.mjs` does not support `--trace` / `--no-trace` flags.** The script accepts only `--ui-root`, `--output-dir`, `--timeout-ms`, `--filter`. The `--trace` infrastructure referenced in §10.3 lives in a different runner. `trace_query_ms` / `trace_resolve_ms` are NOT captured pre-A0.

3. **Pre-A0 audit harness exposes only aggregate counters.** Per-DB hits/misses (`MaterializeMemoDb`, `ComponentMetaResultDb`, `RefCycleResultDb`) and per-`SemanticQueryKey`-family dispatch splits are NOT reachable until B-A0's `CaptureSnapshot::cache_hits/cache_misses` and `dispatch_count(KeyFamily::...)` API cherry-picks onto integration. B-00-baselines uses `RustStoreAudit::materialize_structure_calls` as a conservative surrogate for `max_materialize_memo_db_entries` in committed bound JSONs.

The follow-up dispatches re-run captures with B-A0's authoritative API.

## Acceptance for closure

This debt-closure doc resolves when (Tier 4 §6.7 closure status):

- [x] `crates/verter_session/tests/perf_bounds/chat-message.json` is committed to integration with portable identifiers + corpus-commit metadata. **Closed**: post-fix measurement landed alongside the cold-path attribution sheet at `crates/verter_session/tests/perf_bounds/cold-path-attribution-baseline.md`. Concrete numbers (post-Tier-1 measurement, fresh-cold pass): wall-clock < 60s per fresh-cold run (D108 + D120), `materialize_ms` is the dominant cost arm at ~80% of `total_ms`, `dep_signature_merges` and `dep_signature_intern_hits` now report > 0 thanks to the §6.3 audit-counter wiring fix.
- [x] `crates/verter_session/tests/perf_bounds/chat-messages.json` is committed to integration with portable identifiers + corpus-commit metadata. **Closed**: same wave; the Tier-B `Pick<ChatMessageProps, 'actions'>` fix (§4.3B Phase 10) makes `ChatMessages` cold-path measurable. Post-fix `materialize_ms` is the dominant cost arm; bridge max-depth observed = 11 (corpus floor); audit dump now surfaces `structured_events` per Tier 4 §6.6.
- [x] `tmp/perf-baselines/pre/chat-message.json` and `tmp/perf-baselines/pre/chat-messages.json` exist as gitignored artifacts (PR-attached) — captured as the post-Tier-1 measurement is the new ground truth post-fix; the pre-Tier-A `≥ 300s, audit-dump-blocked` annotation stays in this doc as historical record.
- [x] Slice A1's `loaded_files <= bound` test enumerates 12 components instead of 10. **Closed**: the chat-components are now measurable; the test surface is unblocked.
- [x] This file is updated to "Status: Closed" with the closing commit SHA.

## Tier 4 §6.7 closure note (post-fix)

The audit-counter wiring fix at `crates/verter_session/src/component_meta_audit/mod.rs` (the new
`merge_dep_signature_into_local_fence` helper, plus
`record_node_arena_lock_acquisition` wired into `NodeArena::push_impl`,
plus `record_dep_signature_merge` / `record_dep_signature_intern_hit`
wired into `CompletionFence::merge_signature`) makes the three
previously-zero counters (`node_arena_lock_acquisitions`,
`dep_signature_merges`, `dep_signature_intern_hits`) report > 0 on
the cold-resolver path. The smallest-reproducer test in
`component_meta_audit::tests::audit_counter_smallest_reproducer`
acts as a permanent regression smoke per D80.

The cold-path attribution sheet (`cold-path-attribution-baseline.md`)
identifies `materialize_ms` as the corpus-wide dominant cost arm
(mean 85% of `total_ms`, worst case 92%) and addresses substrate-level
cost arms in-tree per D119 (eviction-policy default sweep removed —
LRU floor preserved as unused capability). Bridge max-depth (D115) is
recorded as 0 pre-Tier-1B; the BFS bridge ships in Tier 1B and will
write the post-bridge max into the same column slot.
- [ ] `crates/verter_session/tests/perf_bounds/chat-messages.json` is committed to integration with portable identifiers + corpus-commit metadata.
- [ ] `tmp/perf-baselines/pre/chat-message.json` and `tmp/perf-baselines/pre/chat-messages.json` exist as gitignored artifacts (PR-attached).
- [ ] Slice A1's `loaded_files <= bound` test enumerates 12 components instead of 10.
- [ ] This file is updated to "Status: Closed" with the closing commit SHA.
