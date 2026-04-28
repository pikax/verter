# Phase 01 — Path-prefix reuse + dispatch traffic counter

**Branch:** `wt/phase-01-perf-pathprefix`
**Base commit (spawn):** `6b1cd1e967bca2d0993c67bae220608e4429fecd`
**Status:** success

## Summary

Phase 1 covered two independent sub-sections of the cutover plan:

- **§1.B Path-prefix subgraph reuse** — `build_project_path` now peeks
  the longest warm `(base, path[..k], Navigate)` prefix before
  constructing the walker, and after the walk completes it backfills
  every linear-member-step intermediate hop into the same warm map via
  the shared publish path that `execute_cooperative` uses. The
  `warm_publish_one` extraction satisfied all r4 invariants
  (no new public API, `pub(crate)` helpers, reverse-index / TOCTOU
  / inflight semantics preserved, targeted unit test landed).
- **§1.C Per-key dispatch traffic counter** — diagnostic-only
  `(variant_discriminant, content_hash)` digest + thread-local
  `DISPATCH_KEY_COUNTS` counter wired into `execute_read`. Zero
  footprint outside test builds.
- **§1.A SolverResultDb** — DROPPED in r2 per the brief; no work
  performed.

## Sub-sections completed

| Sub-section | Status |
|---|---|
| §1.B Path-prefix subgraph reuse | done |
| §1.C Per-key dispatch traffic counter | done |
| §1.C.3 Top-N dump test | DEFERRED — see "Deferred" section |

## Commits

| SHA | Message |
|---|---|
| `53bf3734` | `test(meta): TDD failing test for path-prefix peek + backfill` |
| `fbe13669` | `feat(meta): peek warm path prefixes in build_project_path` |
| `182bcc11` | `feat(meta): backfill intermediate path prefixes after walk` |
| `c86cddcd` | `test(meta): per-key dispatch traffic counter (phase-1c scaffold)` |
| `d5e30df9` | `style(meta): rustfmt phase-1b helpers and tests` |

## Files modified

| File | Lines added | Lines removed |
|---|---|---|
| `crates/verter_session/src/project_semantic_dispatch/build.rs` | 139 | 0 |
| `crates/verter_session/src/project_semantic_dispatch/raise.rs` | 56 | 0 |
| `crates/verter_session/src/project_semantic_dispatch/tests.rs` | 188 | 0 |
| `crates/verter_session/src/project_semantic_dispatch/walk.rs` | 24 | 0 |
| `crates/verter_session/src/semantic_query_memo.rs` | 339 | 63 |
| **Total** | **746** | **63** |

## Tests added

| Test name | File | Purpose |
|---|---|---|
| `project_path_prefix_peek_short_circuits_sibling_walk` | `project_semantic_dispatch/tests.rs` | Discriminating §1.B contract: prefix backfill + sibling-walk peek (delta = 1 on `PREFIX_PEEK_HITS`) |
| `warm_publish_one_inserts_warm_map_and_registers_reverse_index` | `semantic_query_memo.rs` (inline `mod tests`) | §1.B.4 invariant: extracted helper publishes into warm map AND registers Γ.B reverse index |

### Per-test pre-fix-fail / post-fix-pass confirmation

- `project_path_prefix_peek_short_circuits_sibling_walk`:
  - Pre-fix (after commit 4 only): FAILED at "prefix key must be warm
    after first dispatch — Phase 1B backfill should have published it"
    — backfill helper not yet wired.
  - Post-fix (after commit 6): PASSED. Prefix becomes warm after first
    dispatch, second dispatch increments `PREFIX_PEEK_HITS` by exactly 1.
- `warm_publish_one_inserts_warm_map_and_registers_reverse_index`:
  - Added in commit 6 alongside the helper. Verified by direct
    invocation that warm map AND reverse index are both populated.

## Verification

| Check | Result |
|---|---|
| `cargo test --workspace --tests` | 10118 passed, 0 failed |
| `cargo test -p verter_session --test correctness` | 11 passed, 0 failed, 1 ignored — ZERO snapshot drift |
| `cargo clippy --workspace -- -D warnings` | exit 0 (only the pre-existing serde-attribute parse warning, owned by Phase 11a per brief) |
| `cargo fmt --all --check` | clean (after the `style(meta)` commit) |
| `pnpm install --frozen-lockfile` | clean |

## Deferred

- **§1.C.3 Top-N dump test (`dispatch_traffic_top20_input_menu`)** —
  no InputMenu corpus fixture present at plan-write time and
  confirmed absent at spawn-time pre-flight (`ls
  crates/verter_session/tests/component_meta_audit/corpus_representatives/
  | grep -iE "input_menu|inputmenu"` returned no matches). Per
  §1.C.3 brief: "If the grep returns 0 matches, the worker SKIPS C.3
  and notes in `phase-01-report.md`: '§1.C.3 deferred — no InputMenu
  corpus fixture present at plan-write time. Defer to a follow-up
  that creates the fixture.'" — the diagnostic counter from §1.C.2
  still landed; a future fixture can use the existing harness in
  `tests/component_meta_audit/harness.rs` plus `DISPATCH_KEY_COUNTS`
  from `raise.rs` to author the dump test.

## §1.B implementation notes

### Walker `intermediate_nodes` contract refinement

The brief's intermediate_nodes contract (`intermediates[i] == node
reached after consuming path[..i+1]`) holds for the LINEAR prefix of
the walk before any arm-split. Once an arm-split happens, the
iterative-worklist drives per-arm `advance_step` calls that share the
walker's `intermediate_nodes`, breaking the alignment between
intermediate index and path index.

`backfill_prefixes` clamps iteration to:
- `intermediates.len() - 1` (skip terminal — owned by `execute_cooperative`)
- `path.len() - 1` (sibling-sharable prefixes only; `path[..i+1]` stays in range)
- breaks at the first `None` (post-arm-split entries no longer line up
  with `path[..k]`)

Without this clamp, an arm-split mid-walk causes a slice-range panic
when per-arm sub-walks push more intermediates than the trunk path has
segments. Caught and fixed during the post-implementation full lib
test sweep — 7 tests were affected.

### `warm_publish_one` extraction (r4 invariants)

| Invariant | Result |
|---|---|
| Public API of `SemanticGraphStore` is unchanged (no new `pub fn`) | OK — `warm_publish_one`, `warm_publish_one_if_absent`, `register_reverse_index` are all `pub(crate)` or private; `publish_warm_if_absent` is `pub(crate)` |
| Helper visibility is `pub(crate)` (not `pub`) | OK |
| Reverse-index registration / TOCTOU recheck / in-flight retirement / joiner-wakeup semantics preserved | OK — `warm_publish_one` keeps the entries-lock-first TOCTOU re-check + `cold_aborts_swept` accounting; in-flight retirement and joiner wakeup remain in `execute_cooperative`'s steps 6 and 7 (unchanged) |
| Targeted unit test for warm_publish_one | `warm_publish_one_inserts_warm_map_and_registers_reverse_index` |

### Codex-2 r3 fix — Navigate-only prefix keys

Per the path-precise rule (CLAUDE.md "Macro Type Traversal Rule":
intermediate hops at Navigate, terminal hop at caller's mode), all
prefix keys peeked by `find_longest_warm_prefix` and published by
`backfill_prefixes` use `mode: ProjectionMode::Navigate` regardless of
the caller's mode. The terminal full-path key keeps the caller's mode
and is published by `execute_cooperative`'s admission flow (unchanged).
A `debug_assert!` on `publish_warm_if_absent` enforces this invariant
at callsite.

## Stack-depth discipline (§0.6.5)

`PathWalker::walk` is the iterative worklist driver — graph
traversal happens via the explicit `WalkFrame` stack, not stack
recursion. Phase 1B's additions:
- `find_longest_warm_prefix` is a flat `for k in (1..path.len()).rev()`
  loop — bounded by `path.len()`.
- `backfill_prefixes` is a flat `for i in 0..max_i` loop — bounded by
  `min(intermediates.len(), path.len())`.

No new stack-depth risk introduced.

## Cache invariants (§0.6.6)

`backfill_prefixes` publishes every prefix entry through
`publish_warm_if_absent`, which delegates to
`warm_publish_one_if_absent`, which:
1. Skips when result is not `QueryResult::Value` (errors / sentinels
   never warm-publish).
2. Skips `FamilyKey::ResolvedNamedType` (per §7.16 — bypasses family memo).
3. Skips when slot is already warm.
4. Skips when key is currently in-flight.
5. Otherwise locks `entries`, publishes via `FamilySlots::publish`,
   drops the lock, then registers reverse-index entries per
   canonical-in-dep-signature (`entries → canonical_to_entries`
   shards lock order preserved).

Concurrent races between cold-winner publish and backfill are benign
(both publish the same canonical prefix node so values agree;
`FamilySlots::publish` resolves order). The targeted unit test
`warm_publish_one_inserts_warm_map_and_registers_reverse_index`
discriminates against a hypothetical refactor that drops the
reverse-index registration.

## Type-system enforcement (§0.6.8)

Not applicable to Phase 1.
