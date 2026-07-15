# T4 — Canonical→key index for `FileArtifactStore::get_any` / `get_artifacts_for_content`

**Level:** micro/structural (O(store) → O(1) reads). **Risk:** medium — concurrent index coherence; fully test-pinned.
**Reference implementation:** branch `perf/t4-artifact-store-fastpaths`, commit `ef7a5c9b4` (measurement machine).

## Problem (profiler evidence)

`crates/verter_session/src/file_artifact_store.rs::get_any` (~line 1084) iterates the ENTIRE artifacts
DashMap per call to find the base entry for a canonical; `get_artifacts_for_content` (~line 1726) scans
similarly. `get_any` is called from `resolve_eval_dependency_canonical`'s existence probes tens of
thousands of times per request → **DashMap whole-store iteration alone is ~13 % of pass CPU** (14.8 %
of the eval-dependency chain), growing with store size (late-pass queries scan thousands of entries).

## Invariant discovered (matters for the index shape)

One-base-entry-per-canonical does NOT hold store-wide: the legacy `insert` drains prior same-canonical
keys (~lines 1579/1655), but `insert_artifacts` (~line 1921) explicitly allows multiple coexisting
versions (see `tests/cases/g_misc0/eviction_policy.rs` — 5 base keys for one canonical — and
`enforce_per_canonical_retention`). The sole production `insert_artifacts` caller
(`host_manage/overlay_materialize.rs:1089`) publishes overlay-scoped keys only, but the API contract
allows multi-base. **The index must therefore map to a key SET, not a single key.**

## Change

- Secondary index `canonical_keys: DashMap<Arc<str>, SmallVec<[FileArtifactKey; 2]>>` — canonical →
  full live key set. Reads: slot lookup → filter (`is_base()` for `get_any`; `content_hash ==` for
  `get_artifacts_for_content`, which is overlay-inclusive) → exact `artifacts.get(key)`.
- Coherence is guaranteed at mutation sites, not by fallback scans:
  - ALL inserts route through one `insert_artifact_entry` combinator; ALL removals through the
    documented "sole artifact-removal chokepoint" (`evict_artifact_keys`) — note the
    `artifact_removal_routes_through_single_chokepoint` guard pins `self.artifacts.remove(` textually
    inside that fn, so index maintenance is inlined there.
  - Both hold the canonical's index-slot entry guard across the paired map+index mutation
    (lock order: index slot → artifacts shard; deadlock-free because no path takes them in reverse).
  - Schema reset clears index BEFORE map, so races leave only benign dangling index keys, which
    readers skip via the exact `artifacts.get` recheck. No scan fallback, no debug_assert on dangling
    (it would false-fire on legitimate concurrent-removal windows).

## Test contract (characterization written pre-change, green through the change)

`get_any_follows_legacy_insert_content_drain`, `get_any_is_none_after_every_removal_shape`,
`base_reads_follow_eviction_sweeps`, `get_any_skips_overlay_key_inserted_before_base`,
`get_artifacts_for_content_selects_by_hash_across_key_shapes`,
`noop_legacy_reinsert_keeps_base_reads_warm` (+ the T7 pair below). Also keep green: the eviction-policy
integration suite, `artifact_removal_routes_through_single_chokepoint`,
`host_upsert_performs_no_reverse_dependent_eviction`,
`host_upsert_reverse_dep_eviction_scanner_discriminates`, and the get_any allowlist/currency-oracle guards.

## Measured result (this machine)

Full pass (post-fix protocol, T4+T7 together, median of 3 interleaved runs): steady 20 480 →
**16 039 ms (−21.7 %)**; p50 42.7 → 33.9 ms; p95 345 → 289 ms; max 1985 → 1760 ms; peak RSS 720 → 685 MB.
