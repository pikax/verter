# Component-meta batch performance — validation snapshot, fixed batch view, Arc-share, fingerprint memo

Final-state description of the four mechanisms that turned the warm
component-meta batch from a full per-query re-resolve into an O(N)
cache-validation pass, with the numbers measured on this tree.

## Mechanisms

1. **Validation snapshot foundation** (`resolver_store.rs`).
   `VerterHost::store_view_manager()` hands out one immutable
   `Arc<StoreViewSnapshot>` per `StoreViewValidationToken` generation —
   built once under a no-torn singleflighted build, shared by cheap Arc
   clone. The token folds `store_view_epoch` + `project_generation` +
   `FileArtifactStore.artifact_generation` + `load_generation` + the
   workspace `content_generation` + the R21 env-hash fold + project
   identity + frozen overlay identity. `resolver_store_view()` returns a
   typed `StoreViewRead { Current | ReturnOnly }`: a non-current view
   never validates a warm entry, never joins a retained singleflight
   flight, and never promotes — warm validators require
   `CurrentHostStoreView` by type, cold seeds carry currentness
   intrinsically through `ColdSeedHostStoreView`. The
   `content_generation` dimension keeps the cached snapshot honest
   against file-set mutations that move no host-side epoch (watcher
   recovery / `DirectoryTreeDirty`, a wildcard dependency appearing):
   the snapshot's edge-currency gates evaluate against it at build time,
   so the manager must miss once it advances; a cold compute's own loads
   never advance it, so it cannot self-fence promotion.

2. **One fixed store view per component-meta batch** (`meta.rs`,
   `component_meta_entry.rs`, `component_meta_request.rs`). The batch
   coordinator captures ONE fixed view (overlay applied once via a
   single copy-on-write — `BatchFixedView`) and threads it through every
   per-job closure, payload and analysis, batch and scalar N=1: O(N) →
   O(1) `from_host` calls per batch. The fixed view is an optimization,
   never a validation bypass: it carries its captured
   external-supersession fingerprint + currentness, and promotion
   requires captured-current AND fingerprint-still-live AND a complete
   result (a budget-fail-closed / carrier-stopped partial is returned to
   its slot but never admitted, while a complete sibling in the same
   batch admits and stays warm — per-result completeness, no cross-job
   poisoning). Warm payload/analysis probes validate against the
   overlay-applied view, so an overlay session never serves a stale base
   surface.

3. **`Arc<ScriptAnalysisSnapshot>` on the read path** (`parse.rs`,
   `host_views.rs`, `types.rs`). The dominant warm AND cold cost was a
   deep clone + drop of the ~18-Vec snapshot per dependency × per
   component × per query on `effective_file_state`. The snapshot is
   frozen into an `Arc` at parse time; reads are refcount bumps. Pure
   copy→share, byte-identical results — pinned by an `Arc::ptr_eq`
   sharing guard and a full-surface byte-equality test.

4. **Lazy overlay-set fingerprint memo** (`session_view.rs`).
   `OverlaidView`/`OverlaidViewRef` memoize the overlay-set fingerprint
   in a request-view-scoped `OnceLock<u64>` (drift-impossible: the view
   is rebuilt per `with_overlay_view` over immutable refs). The
   fingerprint stays a session/view identity — it never enters
   base/global query-identity keys.

## Measured on this tree (Apple M3, 8 cores)

`bench:meta:ui:warm -- --exclude=chat,table` (nuxt-ui corpus, 168
components, ChatMessages.vue + Table.vue excluded — both resolve
degraded independent of cache state; `getComponentMetaBatch`
cold→warm→warm2 on one session; warm CPU is the load-insensitive
authoritative metric):

| | before (no mechanisms) | after | delta |
|---|---|---|---|
| warm batch CPU | 53.9 s | 8.3 s | **−84.6%** |
| warm wall | 12.9 s | 2.2 s | −82.6% |
| warm/cold CPU ratio | 1.02 | 0.42 | — |
| cold batch CPU | 53.0 s | 19.9 s | −62% (load-contaminated, indicative) |

The residual warm cost is genuine resolver re-walk
(`project_semantic_dispatch` / path-walking / dependency resolution) —
the type resolution the engine exists to perform. The next candidate
lead, if warm cost needs to shrink further, is whether that re-walk is
provably cacheable behind the fact rails; refuted leads (measured on the
original investigation): Mutex sharding (lock-slow 0.8%),
fact-validation hotspots (none), cold prefetch (cold-only).
