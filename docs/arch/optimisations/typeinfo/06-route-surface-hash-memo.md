# T5 — Memoize `hash_route_surface` per `ShallowFileState`

**Level:** micro (recompute elimination). **Risk:** low — value-identical; guarded by an immutability audit.
**Reference implementation:** branch `perf/t5-route-surface-hash-memo`, commit `59ffd48d5` (measurement machine).

## Problem (profiler evidence)

`crates/verter_session/src/resolver_store.rs::hash_route_surface(state)` sorts the full export map and
SipHashes every export + routing target on EVERY call. It is a pure function of the (immutable once
published) `ShallowFileState`, called from three hot sites totalling **≈ 12 % of pass CPU**:
`frontier_engine::build_named_type_export_route_entry` (5.4 %),
`CanonicalCompletionOverlay::write_completion_entry` (4.8 %),
`HostStoreView::build_coherent` / `with_session_overlay` (2.0 %). Its internal `sort_unstable_by_key`
is the bulk of the profile's slice-sort traffic (~4.4 %) plus a large share of `memcmp` (7.3 % total).

## Change

- `crates/verter_session/src/resolver_core/shallow_file_state.rs` (struct at ~line 40, derives
  `Debug, Clone` only — no equality/serialization semantics to protect): add a private
  `route_surface_hash: RouteSurfaceHashMemo` field — a `pub(crate)` newtype over
  `std::sync::OnceLock<Hash16>` (the state is `Arc`-shared across threads).
- **`Clone` must be implemented manually to RESET the memo to an empty cell.** Rationale: a clone is
  exactly the shape that may still be mutated pre-publication (routing fields are `pub`;
  `insert_synthesised_value_default` takes `&mut self`), so an inherited digest could go stale.
- `hash_route_surface` becomes
  `state.route_surface_hash_memo().get_or_init(|| hash_route_surface_uncached(state))` with the former
  body moved verbatim into `hash_route_surface_uncached`.

## Immutability audit (MUST be re-verified before landing on a moved base)

- Single struct-literal construction site: `assemble_from_analysis_with_memo`.
- Only two post-assembly mutators of the hashed surface in the workspace:
  `routing_tables_only_for_test` (test-only, pre-return) and
  `inject_component_default_into_shallow_state` (sole `&mut ShallowFileState` reach; called at
  `prepared_decl.rs:~2034` and `overlay_materialize.rs:~927` on a `mut` local STRICTLY BEFORE
  `Arc::new` and before the producer's first `hash_route_surface` call).
- No `Arc::get_mut`/`make_mut` on the state anywhere. If any post-publication mutation of
  exports/wildcards appears in the future, the memo is unsound — re-run this audit.
- `hash_import_route_targets` is intentionally NOT memoized: its per-artifact memo already exists as
  `IndexedReady.import_route_hash` (`project_type_store.rs:~119`); the state-level memo covers the hot
  callers hashing overlay/donor states where the artifact field is unavailable.

## Test contract

- Characterization (pre-change): independent identical construction hashes equal; a reexport retarget
  and a participant whole-hash move each change the hash.
- Memo behavior (post-change): first call populates; second call identical; a clone starts EMPTY and
  recomputes to an equal value; clone-then-mutate hashes its OWN surface (discriminates against a
  memo-carrying Clone).
- Full `cargo test -p verter_session --lib` (4179 green on reference) + the in-process `--tests`
  surface (all architecture guards green).

## Measured result (this machine)

Full pass (post-fix protocol, median of 3 interleaved runs): steady 20 480 → **17 595 ms (−14.1 %)**;
p50 42.7 → 36.5 ms; p95 345 → 305 ms; max 1985 → 1765 ms; peak RSS 720 → 680 MB.
