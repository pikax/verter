# Typeinfo: in-place package-edit published-`Surface` supersession gap

> **Status:** DEFERRED — tracked follow-up. Pre-existing gap, surfaced (not introduced) during the type-macro migration's typeinfo cache-invalidation hardening (2026-07-20). The five *in-scope* typeinfo invalidation holes were closed in that work (`OwnerImportSurfaceDb`/`RouteDb` warm-hit fact re-observation, commit `6234bbac9`); this one has a **distinct root** and is recorded here for a focused follow-up.

**Audit verdict (2026-07-22): OUT-OF-SCOPE.** This is an internal typeinfo/cache supersession defect in the semantic type engine.

## Symptom

An **in-place content edit** to a **scheduler-tracked (package-backed) leaf file** does not invalidate the warm published `Surface`. After the leaf's content hash is bumped V1 → V2, the owner still resolves the **stale V1** shape (e.g. `Selected.v == 1` instead of `2`).

Reproduction (un-ignore to observe the failure):

```
crates/verter_session/src/typeinfo/typeinfo_tests/cache_invalidation.rs
  ::cache_invalidation_in_place_package_edit_flips_published_surface
```

Shape of the repro: cold-resolve an owner whose published `Surface` reaches a **package-tracked** leaf under `ProjectionMode::Expanded`; `MemoryWorkspace::inject_file` the leaf to a V2 body; re-resolve the same owner query → the surface still reports V1.

## Root cause — a missing scheduler-lane content-supersession rail

This is **not** the `OwnerImportSurfaceDb` / `RouteDb` warm-hit observing-facts discipline (that path now re-observes the surface's recorded chain facts into active tracers — fixed in `6234bbac9`). Here the fact that *is* observed is **itself stale**, because the scheduler node was never reloaded:

- `MemoryWorkspace::inject_file` (the in-place edit path) advances the per-canonical **transition ledger** (`last_content_transition_generation`) **without reloading the scheduler node**.
- Therefore `HostStoreView::build` keeps snapshotting the **stale V1 whole-hash**.
- The warm memo's `FileWholeHash{V1}` fact keeps **validating** against that stale view (the view still tracks V1), so revalidation is a false pass.
- `ensure_loaded`'s fast path keeps serving the stale V1 artifact.

Empirically confirmed during diagnosis: the ledger generation had advanced (gen 6) past the artifact edge generation (edge_gen 5), yet the `StoreView` still tracked V1 — the supersession signal existed in the ledger but never propagated into the loaded scheduler node or the view snapshot.

The **artifact-only lane** already has the equivalent freshness rail (`artifact_only_candidate_is_fresh`). The **scheduler-tracked lane** lacks it.

## Proposed fix (multi-touch — `resolver_store` / host lifecycle)

A scheduler-tracked content-supersession rail, mirroring the artifact-only lane:

1. **Stamp a loaded-at generation** on the scheduler node at upsert / integrate time.
2. **Snapshot-build supersession check** in `HostStoreView::build`: if the canonical's transition-ledger generation is newer than the loaded node's loaded-at stamp, the node is superseded.
3. **Reject in `validates` / `validates_self_root_whole_hash`** when the loaded-at generation is superseded, so a warm `FileWholeHash` fact cannot falsely validate against a stale snapshot.
4. **`ensure_loaded` reload gate**: on a superseded node, reload before serving.

This is deliberately larger and riskier than the demand-time mirror fix that closed the other five holes (it touches `resolver_store` and the host lifecycle across several sites), which is why it was **not** attempted inline during the migration — the fix agent correctly stopped rather than land a multi-touch lifecycle change unbriefed.

## Why deferred (disposition rationale)

- **Distinct root** from the migration's demand-time fact-observation work — it is a scheduler/lifecycle supersession gap, not a resolver-path discipline gap.
- **Pre-existing** — the contract was already `#[ignore]`d before the migration; the migration neither introduced nor worsened it.
- **Out of the migration's ratified scope** (type_surface deletion → TypeInfo-owned demand-driven projection).
- **Bounded blast radius today**: it affects *in-place edits to package-tracked files* (the linked-package / in-place-`node_modules`-edit dev scenario), not the ordinary workspace-file edit path (which the migration's demand-time discipline covers and which is proven green).

## Acceptance gate for the follow-up

Un-ignore `cache_invalidation_in_place_package_edit_flips_published_surface` and make it green (leaf V1→V2 in-place edit flips the published `Surface` to V2), plus a companion negative assertion that an *unrelated* in-place edit keeps the warm surface (no over-invalidation), with the full canonical gate remaining green.
