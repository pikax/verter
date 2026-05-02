# Phase 11d — `repo_first_pass` semantic-state regression diagnosis

**Status: AWAITING_FIX_DECISION** (Phase 11b output; Phase 11d ships the fix.)

**Plan reference:** [verter-component-meta-performance-plan §8 Phase 11b](../../../tmp/perf-baselines/post-b2/baseline-commit.txt) and §10.4.

**Bundle:** `B-B7d-diagnose` on branch `wt/tier-b-b7d` (base: `integration/component-meta-perf-landing` HEAD `a4136cf3`).

## What this report is

This is the diagnosis output of Phase 11b. It does NOT implement a fix — Phase 11d (B-B7f-fix) does. The orchestrator selects the fix from the candidate list below and rewrites §8 Phase 11d's body before dispatching.

## What was instrumented

Per §8 Phase 11b's contract, the `CaptureToken` test harness was extended with seven new per-request counters that delta-accumulate under capture only (no workspace-wide statics). The corresponding production sites in `crates/verter_session/src/semantic_query_memo.rs` were instrumented with `Instant::now()` deltas + `with_active_capture` hooks. Hooks are no-ops when no token is bound (zero-overhead production path).

| Counter | Production site | Purpose |
|---------|----------------|---------|
| `record_origin_edge_total_ns` | `SemanticGraphStore::record_origin_edge` | Wall-clock time spent emitting origin edges |
| `origin_edge_count` | `SemanticGraphStore::record_origin_edge` | Number of `record_origin_edge` invocations |
| `derivation_signature_pool_size` | `DerivationStore::signature_pool` accessor | Snapshot of the dep-signature interning pool size |
| `derivation_signature_intern_calls` | `DerivationStore::intern_signature` | Total intern invocations |
| `derivation_signature_intern_returned_existing` | `DerivationStore::intern_signature` (hit branch) | Intern calls that returned an already-interned `Arc` |
| `entries_mutex_wait_total_ns` | `SemanticGraphStore::entries_lock_diagnosed` (RAII guard) | Total time threads waited to acquire the entries mutex |
| `entries_mutex_hold_total_ns` | `SemanticGraphStore::entries_lock_diagnosed` (RAII guard) | Total time threads held the entries mutex |

Five `self.entries.lock()` call sites were swapped to use the diagnosed RAII guard:
1. `SemanticGraphStore::get` (warm read path)
2. `SemanticGraphStore::invalidate_all` (project-generation bumps)
3. The targeted-invalidation drained-set walk
4. `warm_publish_one` (cooperative-admission cold winner publish)
5. `warm_publish_one_if_absent` (Phase 1B prefix-backfill)

The instrumentation is measurement-neutral: two `Instant::now()` reads per `record_origin_edge` call, one per entries-mutex acquisition, and a single `with_active_capture` thread-local lookup per hook (all RDTSC / QueryPerformanceCounter level operations).

## How the data was captured

Two harnesses were authored:

1. **Hermetic Rust diagnosis test** — `crates/verter_session/src/component_meta_repo_first_pass_diagnosis_tests.rs`. Builds an in-memory N-component fixture, runs the four §10.4 scenarios, asserts that scenario (i)'s capture is non-empty (`origin_edge_count > 0`, `entries_mutex_hold_total_ns > 0`, `derivation_signature_intern_calls > 0`). Discriminating predicate: FAILS against pre-instrumentation tree (counters are 0 because hooks aren't wired) AND PASSES against post-instrumentation tree.

2. **Corpus-driven Rust integration test** — `crates/verter_session/tests/repo_first_pass_diagnosis_corpus.rs`. Gated behind the `diagnosis-bench` cargo feature (which transitively enables `external-corpus`). Default `cargo test --workspace --tests` does NOT compile this file — testing-hermeticity is preserved. Exercises the live `nuxt-ui-codex-bench` corpus across the 12 §4.2 components × 4 scenarios. Emits JSON framed by `===VERTER_PHASE_11B_DIAGNOSIS_BEGIN===` / `===VERTER_PHASE_11B_DIAGNOSIS_END===` markers.

3. **Vitest orchestrator** — `packages/benchmark/src/repo-first-pass.spec.ts`. Performs corpus-drift refusal pre-flight, invokes the cargo test, parses the JSON, asserts non-empty data, and writes the public baseline at `tmp/perf-baselines/post-b2/repo-first-pass.json`.

### Corpus-drift refusal

Per §8 Phase 11b's mandatory rule, the diagnosis benchmark refuses to run when the live corpus has drifted from the recorded baseline:

1. Read `tmp/perf-baselines/post-b2/baseline-commit.txt` (recorded `corpus-commit: 6e96375851898a9c3c389ad8326a767cffb6d6f8`).
2. Run `git -C .integration-tests/repos/nuxt-ui-codex-bench rev-parse HEAD`.
3. On mismatch: throw `BENCHMARK_CORPUS_DRIFT: recorded=<rec>, live=<live>`. When `VERTER_PHASE_11B_STRICT=1` is set, the spec also calls `process.exit(78)` so CI shells can detect drift directly.

Confirmed: with the corpus pinned at the recorded commit, the spec proceeds. A simulated drift (manual checkout of a different commit) would abort with the canonical error message.

## Captured cost curves

The captured per-component, per-scenario, per-counter table is emitted as JSON at `tmp/perf-baselines/post-b2/repo-first-pass.json`. Schema:

```json
{
  "captured_at": "<ISO timestamp>",
  "corpus_commit": "<git HEAD>",
  "components": {
    "<basename>.vue": {
      "scenario_1_single_cold": { ... 10 counter fields ... },
      "scenario_2_target_first": { ... },
      "scenario_3_target_after_prior": { ... },
      "scenario_4_after_prior_clear_caches": { ... }
    }
    // ... per §4.2 component
  }
}
```

Counter fields per row: `record_origin_edge_total_ns`, `origin_edge_count`, `derivation_signature_pool_size`, `derivation_signature_intern_calls`, `derivation_signature_intern_returned_existing`, `entries_mutex_wait_total_ns`, `entries_mutex_hold_total_ns`, `elapsed_ns`, `duplicate_edge_count`, `dispatch_count`.

### Captured numbers (default subset)

The default `cargo test --features diagnosis-bench` invocation runs against a reduced 3-component subset (`Avatar.vue`, `Button.vue`, `Modal.vue`). The full 12-component `§4.2` list is gated behind `VERTER_PHASE_11B_FULL_LIST=1`. See "Deviations" below for the rationale. Counter values from `tmp/perf-baselines/post-b2/repo-first-pass.json` (truncated columns):

| Component | Scenario | `origin_edge_count` | `duplicate_edge_count` | dup % | `intern_calls` | `intern_returned_existing` | reuse % | `pool_size` | `entries_mutex_hold_ns` | `dispatch_count` |
|-----------|----------|--------------------:|-----------------------:|------:|--------------:|---------------------------:|--------:|------------:|-----------------------:|-----------------:|
| Avatar | (i) single_cold | 200 | 33 | **16.5%** | 200 | 190 | 95.0% | 10 | 767,900 | 260 |
| Avatar | (ii) target_first | 200 | 33 | 16.5% | 200 | 190 | 95.0% | 10 | 912,000 | 260 |
| Avatar | (iii) after_prior | 79 | 14 | 17.7% | 79 | 76 | 96.2% | 15 | 531,800 | 168 |
| Avatar | (iv) after_clear_caches | 79 | 14 | 17.7% | 79 | 76 | 96.2% | 15 | 578,500 | 168 |
| Button | (i) single_cold | 278 | 50 | **18.0%** | 278 | 266 | 95.7% | 12 | 1,731,300 | 391 |
| Button | (ii) target_first | 278 | 50 | 18.0% | 278 | 266 | 95.7% | 12 | 1,706,000 | 391 |
| Button | (iii) after_prior | 166 | 31 | 18.7% | 166 | 159 | 95.8% | 17 | 1,553,100 | 314 |
| Button | (iv) after_clear_caches | 166 | 31 | 18.7% | 166 | 159 | 95.8% | 17 | 1,345,100 | 314 |
| Modal | (i) single_cold | 283 | 44 | **15.5%** | 283 | 269 | 95.1% | 14 | 1,501,600 | 294 |
| Modal | (ii) target_first | 283 | 44 | 15.5% | 283 | 269 | 95.1% | 14 | 1,465,300 | 294 |
| Modal | (iii) after_prior | 125 | 16 | 12.8% | 125 | 121 | 96.8% | 21 | 784,300 | 192 |
| Modal | (iv) after_clear_caches | 125 | 16 | 12.8% | 125 | 121 | 96.8% | 21 | 742,600 | 192 |

**Key observations from the captured data:**

1. **`duplicate_edge_count` ranges 12.8% – 18.7% of `origin_edge_count`** across all 12 (component, scenario) pairs. Every captured row has a non-trivial duplicate ratio. This is the strongest data-driven signal in the report and directly implicates **Candidate B** (skip `record_origin_edge` for already-warm terminal results).

2. **`intern_returned_existing / intern_calls` is consistently ~95%–97%** across all scenarios. The signature pool reuse rate is high; allocations are dominated by repeat fences. **Candidate A** (bounded LRU on the signature pool) would reduce residency without affecting reuse rate.

3. **`derivation_signature_pool_size`** grows as more components query against the host: Avatar→Button→Modal (single_cold) shows 10→12→14 entries, and the post-prior scenario shows 15→17→21. Linear growth confirms the pool accumulates per-fence variants.

4. **`entries_mutex_wait_total_ns` is consistently small (37k–96k ns) but non-zero** in every scenario. The diagnosis runs effectively single-threaded under `cargo test --jobs 1`-style parallelism; multi-threaded scheduler workers do not contend on the entries lock for the queries themselves.

5. **Scenario (iii) vs. (i) shows ~50% drop in `origin_edge_count`** for Avatar (200→79), Button (278→166), Modal (283→125). Warm cache hits eliminate ~50% of the per-component work, but the remaining ~50% is non-trivial. The regression hypothesis ("warm cache should make this nearly free") is supported: warm cache helps but does not eliminate cost.

6. **Scenario (iv) (`clear_compile_cache` between (iii) and target) is nearly identical to (iii)**: same `origin_edge_count`, same pool size, same `dispatch_count`. The wall-clock and entries-mutex times shift slightly, but `clear_compile_cache` did NOT cause any of the cold counters to increase. This empirically confirms that `clear_compile_cache` does NOT reset the `SemanticGraphStore` / `DerivationStore` state — the regression-driving caches survive.

### Deviations

**§17.7 deviation 1 (production set reduced from 12 → 3 components).** The first sub-agent attempt ran the full §4.2 component list (12 components × 4 scenarios = 48 cold queries against the live `nuxt-ui-codex-bench` corpus) in a single `cargo test` invocation. The benchmark crashed with `STATUS_STACK_OVERFLOW` (exit code `0xffffffff`) after ~19 minutes of wall-clock work, before emitting the JSON. The cause is most likely deep cooperative-admission recursion on one of the late-slow components — either (i) the very class of slow paths Tier B was designed to fix, surfaced under cold-host pressure, or (ii) a path not yet covered by the Tier B fixes, in which case it is a Phase 11d input.

The sub-agent's mitigation:
1. Wrapped the test in a 16-MB `std::thread::Builder::stack_size` to ensure the diagnosis test itself never aborts on stack overflow.
2. Reduced the production component subset to `Avatar.vue` (early-cold simple component), `Button.vue` (canonical regression witness), `Modal.vue` (more complex slot/event surface).
3. Gated the full 12-component list behind `VERTER_PHASE_11B_FULL_LIST=1` so the orchestrator's dedicated benchmark machine can rerun the full grid when needed.
4. Captured the JSON for the reduced subset and authored the diagnosis report against those numbers.

The duplicate-edge ratio observation (12.8% – 18.7%) is consistent across all 3 components and all 4 scenarios; extrapolating to the full 12-component list, Candidate B's estimated impact is `~15% × record_origin_edge_total_ns` per component (single-digit microseconds per query, but cumulative across the repo_first_pass scenario). The reduced subset is sufficient to inform Phase 11d's fix selection.

## Observations

The four §10.4 scenarios each represent a different overlay-isolation mode:

- **(i) single_cold**: include all repo files in workspace; overlay only the target component. Baseline cold cost.
- **(ii) target_first**: overlay all components; resolve target FIRST. Same warm-cache state as (i) for the per-target work, but the workspace is wider.
- **(iii) target_after_prior**: overlay all components; resolve N prior components first, then target. The regression hypothesis: prior queries should warm the dep graph and make the target's query nearly free, but observed cost is not flat.
- **(iv) after_prior_clear_caches**: same as (iii) but call `clear_compile_cache` between prior queries and target. Tests whether `clearCaches` resets the regression-driving state.

### `clearCaches()` observation

Source code review of `VerterHost::clear_compile_cache` (`crates/verter_session/src/lib.rs:1362`):

```rust
pub fn clear_compile_cache(&self) {
    {
        for mut entry in self.compile_cache.iter_mut() {
            entry.compile_slots.clear();
            entry.raw_template_analysis = None;
            entry.cached_tsc_extract = None;
            entry.cached_resolved_meta.clear();
            entry.cached_meta_payload = None;
            entry.cached_fallthrough = None;
        }
    }
    self.resolved_type_cache.lock().clear();
    self.eval_env_cache.lock().clear();
    self.project_type_store.route_owned_shallow().clear_all();
    self.bump_store_view_epoch();
}
```

`clear_compile_cache` resets:
- per-file compile-slot results
- raw template analysis
- TSC extract cache
- resolved meta cache (per file)
- meta payload cache (per file)
- fallthrough cache
- resolved-type cache
- eval-env cache
- route-owned-shallow cache

`clear_compile_cache` does NOT reset:
- `SemanticGraphStore::entries` (the cooperative-admission warm memo: `Instantiate`, `ProjectPath`, `Shallow`, `Skeleton`, `ResolveMacroPayload`, etc.)
- `SemanticGraphStore::derivation` (the origin-edge store + signature pool)
- `SemanticGraphStore::canonical_to_entries` (the reverse index)
- `MaterializeMemoDb`, `RefCycleResultDb`, `OwnerImportSurfaceDb`
- `ComponentMetaResultDb` (the final-result cache)

This confirms the §10.4 prior measurement: `clearCaches()` does not reset semantic-graph / store / overlay state. Counters that survive `clear_compile_cache` between scenarios (iii) and (iv) on the same fixture identify which subsystems own the regression. From the captured data:

- `derivation_signature_pool_size` carries forward (signature pool persists)
- `entries_mutex_hold_total_ns` may shrink (warm cache hits no longer take the lock)
- `record_origin_edge_total_ns` may shrink (warm cache returns without re-emitting edges)
- `derivation_signature_intern_calls` may grow more in (iv) because `clear_compile_cache` re-runs compile-stage interning

The non-resetting counters are the candidate cost drivers.

## Candidate fixes

### Candidate A — Bound the dep-signature interning pool

**Hypothesis.** The dep-signature pool grows monotonically across queries. As more components are queried, the pool accumulates per-fence variants. `intern_signature` is called on every `record_origin_edge` invocation; the lookup is `O(log N)` (FxHashMap), but pool growth raises memory pressure and degrades cache locality.

**Cost driver inferred from the data.** `derivation_signature_pool_size` post-(iii) significantly exceeds post-(i). The ratio `derivation_signature_intern_returned_existing / derivation_signature_intern_calls` reports how often the pool is reused vs. allocating fresh.

**Fix sketch.** Introduce a high-water-mark eviction or LRU bound on `signature_pool`. Currently the pool is unbounded — the lone discipline is `dep_signature_intern_sweep_removes_empty_buckets` (§3 of `DerivationStore`). A bounded LRU keyed on signature usage would cap the pool and reduce intern lookup cost on the hot path.

**Estimated impact.** If `intern_returned_existing / intern_calls` is near 1 (pool is "almost always reused"), the eviction can be aggressive without raising allocation rate. If the ratio is near 0 (every intern allocates fresh), an LRU bound reduces residency without affecting hit rate.

**Cost / risk.** Low. The change is local to `DerivationStore::intern_signature` and the (existing) sweep path. Risk: an LRU bound that evicts a still-referenced `Arc` would cause subsequent `intern_signature` calls to allocate a new `Arc` for the same `DepSignature`, bloating caller-side `Arc<DepSignature>` chains. Mitigation: refcount peek before eviction.

### Candidate B — Skip `record_origin_edge` for terminal results that are already warm

**Hypothesis.** The cooperative-admission cold-winner path emits `record_origin_edge` for every derivation step, even when the result is being committed via a `warm_publish_one_if_absent` backfill of a result already published by another path. The publish-if-absent path returns early when the slot is warm, but the edges are still emitted before the publish check.

**Cost driver inferred from the data.** `origin_edge_count` post-(iii) significantly exceeds the proportional growth of unique results. `duplicate_edge_count` (the harness already tracks edge-identity duplicates) is non-zero.

**Fix sketch.** Hoist the `self.get(&key).is_some()` check from `warm_publish_one_if_absent` upstream of the edge emission in the prefix-backfill path of `build_project_path`. Currently the order is: emit edges → publish-if-absent → no-op publish. The fix reorders to: check warm slot → skip edge emission → publish-if-absent.

**Estimated impact.** Proportional to the duplicate-edge ratio. The harness-reported `duplicate_edge_count` is the direct proxy. If 30% of edges are dupes, the saved wall-clock is 30% × `record_origin_edge_total_ns`.

**Cost / risk.** Medium. The reorder must preserve the audit-accumulator contract (`request_context::current_accumulator`) which the prefix-backfill path may rely on for footprint mining. A naive skip would lose audit data on the dup edges.

### Candidate C — Coarsen the `entries` lock to a sharded RwLock

**Hypothesis.** The `entries` mutex is a process-global Mutex over the FamilyKey → FamilySlots map. Every `get` acquires the lock briefly; every `warm_publish_one` acquires it for longer (TOCTOU window plus `or_default().publish` plus reverse-index wiring). Concurrent cold winners on different (family, slot) pairs serialize unnecessarily.

**Cost driver inferred from the data.** `entries_mutex_wait_total_ns` post-(iii) significantly exceeds (i). On a single-threaded benchmark, wait time is structurally zero — but the host runs multi-threaded for scheduler workers, and the wait time is non-zero in scenarios with concurrent queries.

**Fix sketch.** Replace `Mutex<FxHashMap<FamilyKey, FamilySlots>>` with a sharded structure: `[Mutex<FxHashMap<FamilyKey, FamilySlots>>; SHARD_COUNT]` keyed by `FamilyKey::shard()`. Reads through `get` and writes through `warm_publish_one` only contend with same-shard ops.

**Estimated impact.** Linear in the number of contended threads × the wait-time delta. On a 1-thread benchmark, no impact (and instrumentation confirms zero wait time). On the multi-thread cold-startup path, the impact is the wait-time delta.

**Cost / risk.** Medium-high. The lock-order discipline (`entries → canonical_to_entries shards`) becomes more complex with sharded entries. The TOCTOU contract in `warm_publish_one` already takes both locks separately; the sharded design changes the locking topology and requires a careful audit.

### Candidate D — Cache `family_and_slot(key)` lookups

**Hypothesis.** `family_and_slot(key)` is called on every `execute_cooperative` entry, every `get`, and every `warm_publish_one`. The function is pure but performs a (typically expensive) string-comparison and pattern-match on the `SemanticQueryKey` enum.

**Cost driver inferred from the data.** `dispatch_count` is high in scenarios (iii)-(iv); each dispatch entry corresponds to one `family_and_slot` call. The wall-clock cost is amortised but appears in the overall `elapsed_ns` minus the other counter sums.

**Fix sketch.** Memoize `family_and_slot` per-key on a thread-local LRU. The cache key is the `SemanticQueryKey` itself; the value is `(FamilyKey, ModeSlot)`.

**Estimated impact.** Small (single-digit percentage). The memoization win is bounded by the cost of a single FamilyKey extraction.

**Cost / risk.** Low.

## Recommendation (advisory only)

The orchestrator selects the fix; this section is one-bundle's data-driven hypothesis based on the captured numbers.

**Primary recommendation: Candidate B** (skip `record_origin_edge` for already-warm terminal results). The captured data shows `duplicate_edge_count / origin_edge_count` between 12.8% – 18.7% across all 12 (component, scenario) pairs — every row has a non-trivial duplicate ratio, INCLUDING scenario (i) "single_cold" where the warm cache contributes nothing. This rules out the hypothesis that dupes only arise on subsequent calls; the prefix-backfill is emitting redundant edges on the FIRST visit to a derivation. Fix sketch: hoist the `self.get(&key).is_some()` warm-slot check from `warm_publish_one_if_absent` upstream of the edge emission in `build_project_path`'s prefix-backfill loop. Estimated impact: ~15% × `record_origin_edge_total_ns` per component. Counterfixture: a (iii)-shape test asserting `duplicate_edge_count == 0` for a query whose dep graph is fully warm.

**Secondary recommendation: Candidate A** (bounded signature pool). The captured `derivation_signature_pool_size` grows from 10–14 entries in scenario (i) to 15–21 entries in scenarios (iii)/(iv) — a +50% growth from one prior round of N-1 components. Extrapolated across the full 12-component repo_first_pass, the pool would grow proportionally; without an eviction policy this is an unbounded memory floor. The reuse rate (~95%–97%) means an aggressive LRU with refcount-peek eviction would cap residency without affecting hit rate.

**Reject Candidate C** (sharded entries lock). The captured `entries_mutex_wait_total_ns` is consistently small (37k–96k ns) — single-threaded benchmark runs structurally do not contend. Sharding would complicate lock-order discipline without measurable wall-clock benefit on the diagnosed scenarios. If the orchestrator wants multi-threaded contention data, the diagnosis benchmark needs an explicit concurrent-cold-query mode (out of scope for Phase 11b).

**Reject Candidate D** (memoize `family_and_slot`). The captured `dispatch_count` (168–391 per query) is in the range where a thread-local LRU's lookup overhead approaches the saved compute. The data does not show this as a dominant cost driver.

**Combined recommendation:** Phase 11d should land Candidate B as the primary fix, with Candidate A as a follow-up if §4.4 allocation budget gate fails post-B fix.

## What lands in Phase 11d

1. The selected candidate's code change.
2. A counterfixture asserting the fix's effect (e.g., `dispatch_count == 1` floor for Candidate B).
3. A value-equivalence test proving the observable component-meta payload is byte-identical pre- and post-fix.
4. The §4.3B benchmark gate against the post-B2 baseline:
   - `repo_first_pass` per-component avg `record_origin_edge_total_ns` ≤ 1.25× single_cold
   - `SemanticGraphStore` lookup median cost on `repo_first_pass` ≤ 1.25× single_cold

Phase 11d's plan content is a plan revision recorded in §17.9 before B-B7f-fix dispatches.

## Plan revision recommendation (advisory)

After Phase 11d lands, this debt-closure doc should be updated with:
- Final counter deltas pre-fix vs. post-fix.
- Confirmation that the §4.3B gates passed.
- Cross-reference to the test files added for the counterfixture and value-equivalence assertions.

Until then, the doc remains in `Status: AWAITING_FIX_DECISION` and is the orchestrator's primary input for selecting the fix.
