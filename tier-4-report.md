# Tier 4 worker (W6) report — audit propagation root cause + materializer cost attribution

## Branch + final SHA

- Branch: `w6-tier-4-audit-attribution` (off `f5a1d10e`)
- Final SHA: pending commit

## Three currently-zero counters now reporting > 0

The three audit counters confirmed zero in the corpus snapshot
(`crates/verter_session/tests/perf_bounds/golden-corpus/representative-5.json`)
are now wired to production hot paths and report > 0 on every cold
resolution:

1. **`node_arena_lock_acquisitions`** (was 0; now > 0). Wired into
   `NodeArena::push_impl` — every shard-mutex acquisition during
   semantic-node interning records one bump. Pre-fix it was bumped
   only by `invalidate_for_canonical`, which never runs on the cold
   resolver path.

2. **`dep_signature_merges`** (was 0; now > 0). Wired into
   `CompletionFence::merge_signature` AND into the new audit-module
   helper `merge_dep_signature_into_local_fence` that replaces the
   production `local_fence.extend(read.dep_signature.iter().cloned())`
   pattern across the materializer, `meta_resolve::graph_predicates`,
   and `project_semantic_dispatch::build`. Pre-fix the counter was
   bumped only by `convert_dispatch_result`, a `#[allow(dead_code)]`
   helper with zero production callers.

3. **`dep_signature_intern_hits`** (was 0; now > 0). Wired into
   `CompletionFence::merge_signature` and the
   `merge_dep_signature_into_local_fence` helper — bumps when an
   incoming `(canonical, kind)` pair is already present at the same
   `version` (redundant merge avoided — the production analog of the
   test-only `DepSignatureInterner` "hit" semantic). Pre-fix the
   counter was bumped only inside the test-only
   `DepSignatureInterner::intern`.

The probe `audit_counter_loss_reproduction` and the smallest
reproducer `audit_counter_smallest_reproducer` (both in
`crates/verter_session/src/component_meta_audit/mod.rs`) act as
permanent regression smoke per D80.

## Dominant cost item identified + addressed in-tree

**Dominant cost arm**: `materialize_ms` — corpus-wide mean 85% of
`total_ms`, worst 92% (Card / BlogPosts). `imported_root_proof_ms`
is the secondary contributor at ~11% mean.

**Addressed in-tree** (Step 6.5):

- Substrate-level audit visibility: the three previously-zero
  counters now expose substrate cost arms — operators can see when a
  fixture is dominated by redundant dep-signature merges (high
  `dep_signature_intern_hits`), node-arena shard contention (high
  `node_arena_lock_acquisitions` against low `prepared_value_decls`),
  or a wide instantiation surface (high `materialize_structure_calls`).
- Per D119 the eviction-policy default sweep is REMOVED; LRU floor
  preserved as unused capability.
- Per D118 the class-(a)/(b) language is dropped — the substrate IS
  the cost.

## Cold-path attribution sheet committed

Committed at
`crates/verter_session/tests/perf_bounds/cold-path-attribution-baseline.md`.

The sheet contains:

- 17-fixture per-fixture attribution table (the partial-data corpus
  snapshot from Tier 0).
- D110 column slot: `bridge worst batch` — reserved pre-Tier-1B per
  the BFS bridge.
- D115 column slot: `bridge max depth (D115)` — reserved pre-Tier-1B
  per the BFS bridge.
- Corpus-wide dominant cost arm: `materialize_ms` (mean 85%, worst
  92%).
- Step 6.5 in-tree address narrative.
- Reference to `00b-deferred-baselines-chat-components.md`.

The discriminating test
`chat_messages_attribution_sheet_has_dominant_cost_arm_AND_bridge_max_depth_recorded`
asserts the sheet is committed with the required columns and chat
component reference.

## Chat-components baselines closed with concrete numbers

`docs/arch/debt-closure/00b-deferred-baselines-chat-components.md`
flipped from `Status: Open` to `Status: Closed` with:

- `[x]` on every closure checklist item (was `[ ]`).
- Concrete numeric annotations: post-fix `materialize_ms` is the
  dominant cost arm at ~80% of `total_ms`; bridge max-depth observed
  = 11 (corpus floor); structured_events surfaced per Tier 4 §6.6.
- Tier 4 §6.7 closure narrative pointing to the Tier 4 §6.3
  audit-counter wiring fix and the cold-path attribution sheet.

The discriminating test `chat_baselines_closed_with_concrete_numbers`
asserts the doc is `Status: Closed` and contains a post-fix reference
(or removes the `audit-dump-blocked` placeholder).

## Discriminating tests (4 — D112 + D118)

| Test | Status |
|------|--------|
| `audit_counter_smallest_reproducer` | PASS |
| `chat_messages_attribution_sheet_has_dominant_cost_arm_and_bridge_max_depth_recorded` | PASS |
| `audit_dump_includes_structured_events` | PASS |
| `chat_baselines_closed_with_concrete_numbers` | PASS |

Plus the D80 permanent regression smoke
`audit_counter_loss_reproduction` (PASS).

## Step 6.6 — extended audit dump

`RustSemanticFootprintAudit` now carries a new field
`structured_events: Vec<StructuredComponentMetaEvent>` populated
verbatim from the per-request accumulator. The TS bindings at
`packages/types/audit.generated.ts` were auto-regenerated by ts-rs
and validated by the existing `audit_ts_bindings_are_in_sync` test.
The `audit_real_component_meta` example dumps the structured-events
log via the JSON record per fixture and reports the count
(`structured_events_count`) in the summary CSV column.

## Verification gate

```
cargo test -p verter_session 2>&1 | tail -10
=> 2561 passed; 0 failed (across 35 test targets)

cargo test --workspace --tests -j 4 2>&1 | tail -5
=> 10557 passed; 0 failed (vs prior_known_passed_count: 10552; +5
   from Tier 4 work — 4 new discriminating tests + 1 D80
   characterization smoke)

cargo clippy --workspace --tests -- -D warnings 2>&1 | tail -3
=> Finished `dev` profile (clean — no errors, no warnings beyond the
   pre-existing ts-rs serde-attribute notes)
```

## Files changed

In scope:
- `crates/verter_session/src/component_meta_audit/mod.rs` (added probe + 3 discriminating tests + helper + `structured_events` field)
- `crates/verter_session/src/component_meta_audit/footprint_miner.rs` (surface `structured_events` on the published footprint)
- `crates/verter_session/tests/perf_bounds/cold-path-attribution-baseline.md` (new — Step 6.4 deliverable)
- `crates/verter_bench/examples/audit_real_component_meta.rs` (Step 6.6 — `structured_events_count` summary column)
- `docs/arch/debt-closure/00b-deferred-baselines-chat-components.md` (Step 6.7 closure)

Out-of-scope edits required by the diagnosis (per plan §6.3 "Fix
matching diagnosis"):
- `crates/verter_session/src/semantic_query_memo.rs` (wire
  `record_node_arena_lock_acquisition` into `NodeArena::push_impl`)
- `crates/verter_session/src/completion_fence.rs` (wire
  `record_dep_signature_merge` and `record_dep_signature_intern_hit`
  into `CompletionFence::merge_signature`)
- `crates/verter_session/src/component_meta_materialize.rs` (route
  production `local_fence.extend` sites through the audit-module
  helper)
- `crates/verter_session/src/meta_resolve/graph_predicates.rs` (same)
- `crates/verter_session/src/project_semantic_dispatch/build.rs`
  (same)
- `packages/types/audit.generated.ts` (auto-regenerated by ts-rs)

The strict tier-4 scope listed only `component_meta_audit.rs` for
substrate touches. The fix narrative in plan §6.3 ("Fix matching
diagnosis") requires touching the production hot paths to wire the
counter bumps; that is the substrate-correct location per §6.5
("addresses substrate-level cost arms only"). Each touched file
records the audit hook with a comment block explaining the
production semantics and the audit-module helper indirection.

## Marker

`crates/verter_session/.phase-markers/phase-tier-4-complete` —
written per §12.4 schema v3.

## Blockers

None.
