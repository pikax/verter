# T7 — Entry-embedded hit counter + access tick in `FileArtifactStore`

**Level:** micro (per-hit overhead removal). **Risk:** low; two small documented policy shifts.
**Reference implementation:** branch `perf/t4-artifact-store-fastpaths`, commit `3be3dad43` (measurement machine).

## Problem (profiler evidence)

On EVERY warm artifact hit, `bump_hit_counter` (~line 1150) clones the full `FileArtifactKey` into a
separate `hit_counters` DashMap (SipHash of the whole key per bump — 1.2 % of pass CPU) and
`bump_access_tick` (~line 1141) allocates a fresh `Arc<str>` from `&str` into a `last_access` map.
Pure telemetry costing key-clone + hash + alloc on the hottest read path.

## Change

- Map value becomes `StoredArtifact { payload: Arc<FileArtifacts>, hits: AtomicU32 (saturating),
  last_access_tick: AtomicU64 (fetch_max) }`. Warm hits bump through the already-held entry reference —
  zero hashing, zero allocation. The global `access_tick: AtomicU64` remains the tick source.
- `hit_counters` and `last_access` side-maps are DELETED (no external consumers; the
  `last_access_tick` string occurrence in `architecture_guards.rs` is a scanner self-test fixture, not
  a consumer).
- `evict_lru_promoted` reads the embedded atomics during its existing iteration, and drops its
  per-removed-key whole-store `has_more` rescan.
- Two intentional, documented policy shifts:
  1. A REPLACED value starts cold (hits=0) — this matches the removal chokepoint's stated lifecycle
     rule that the old side-map silently violated (stale counters survived replacement).
  2. Recency is per-ENTRY, not per-canonical (the old `last_access` was canonical-keyed). Eviction
     ordering consumers were updated accordingly.

## Test contract

`warm_hit_counter_increments_and_dies_with_the_entry`,
`lru_promotion_sees_hits_recorded_through_base_reads`, plus the whole T4 suite and the eviction-policy
integration tests (6) staying green. `cfg(test)` accessor `hit_count()` reads the embedded atomic.

## Measured result (this machine)

Measured together with T4 (see `04-…`): combined T4+T7 = −21.7 % steady. Commits are separable
(`ef7a5c9b4` index, `3be3dad43` counters) if per-commit attribution is needed on the production machine.
