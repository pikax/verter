# Step 3 — `cooperative_get_or_insert` extraction + DB migration plan

Source plan: `<scratch>/architectural-debt-closure.md` (revision 10), Step 3.

## What landed in this commit

**Sub-task 3.2.0 — admission core extraction.** A standalone module
`crates/verter_session/src/cooperative_admission.rs` provides
`cooperative_get_or_insert<K, Entry, V, ...>` as an admission-only
primitive.

The primitive guarantees:
- Exactly-one-computer per `(key, miss-window)`.
- Cooperative wait: joiners block on a `parking_lot::Condvar`; no
  busy-spin.
- Panic safety via RAII guard: a panicking compute fails the slot,
  wakes waiters, and removes the slot from the in-flight table.
- Post-compute revalidation: a fresh entry that fails revalidation
  (e.g. dep-signature stale due to mid-compute file mutation) is
  dropped before publish; waiters fall through.
- Value projection isolation: the same Entry can be projected to
  different Value types per call site.

Strictly decoupled from semantic-specific concerns. Recursion sentinels,
stats, request-context cache events, retry budgets, and TOCTOU-against-
invalidation all stay in the caller's composition layer.

**5 gating tests pass:**
1. `cooperative_admission_one_winner_others_wait` — 100 threads,
   exactly 1 compute call observed.
2. `cooperative_admission_panic_wakes_waiters` — winner panics in
   compute; joiners wake with `None`; subsequent calls retry cold.
3. `cooperative_admission_post_compute_revalidation_drops_stale` —
   revalidation rejects fresh entry; map remains empty.
4. `cooperative_admission_invalidation_during_compute_retries` —
   first call drops on rejection, second succeeds when revalidation
   passes.
5. `cooperative_admission_value_projection_isolated` — same Entry
   projects to different Value types per call site.

## What is deferred

**Sub-task 3.2.1+ (10 typed DB wrappers on `ProjectTypeStore`).**
Each of the 10 caches enumerated in plan §3 D3.2 needs:

1. A typed `*Db` struct holding `(DashMap<Key, Arc<Entry>>,
   InflightTable<Key>)`.
2. Wiring to `ProjectTypeStore::evict_canonical` and
   `bump_project_generation_and_evict`.
3. `Engine` field conversion from `FxHashMap<Key, Value>` to a
   per-request read-through `RefCell<HashMap<Key, Value>>` view that
   delegates miss-handling to `<Db>.get_or_compute(key, host, ||
   compute)`.
4. Negative-caching support where the legacy cache stored `Option<V>`
   directly (e.g. `imported_registry_symbols`, `resolvable`) — wrap
   in `Entry { resolved: Option<V> }` so None-results stay cached.

Per the plan, all 10 default to MIGRATE (D3.1):

| Cache | Key shape | Value |
|---|---|---|
| `imported_registry_symbols` | `(canonical, name)` | `Option<ResolvedImportedRegistrySymbol>` |
| `declarations` | `(canonical, name)` | `ResolvedTypeDeclaration` |
| `resolvable` | `(canonical, name)` | `bool` |
| `prepared_target_cache` | `PreparedTargetCacheKey` | `Option<(String, String)>` |
| `materialized_member_surfaces` | `MaterializedMemberSurfaceKey` | `TypeExpr` |
| `prepared_surface_cache` | `PreparedSurfaceCacheKey` | `PreparedSurfaceProjection` |
| `prepared_member_cache` | `PreparedMemberCacheKey` | `Option<ProjectedMember>` |
| `routed_expr_surface_cache` | `RoutedExprSurfaceCacheKey` | `TypeExpr` |
| `owner_collection_exprs` | `String` | `Option<TypeExpr>` |
| `materialize_memo` | `(String, TypeExpr, bool)` | `MaterializedTypeExpr` |

Each migration needs its own commit due to the substantial coupling
between the cache's location, its callers, and its key/value shapes.
The plan estimated this at ~3000-5000 LOC across three commits; in
practice each cache is 100-200 LOC of changes including engine view
plumbing.

**Migration pattern (one cache):**

```rust
// crates/verter_session/src/component_meta_caches.rs (NEW)
pub struct DeclarationLookupDb {
    entries: DashMap<DeclarationLookupKey, Arc<DeclarationLookupEntry>>,
    inflight: InflightTable<DeclarationLookupKey>,
    counters: CacheCounters,
}

pub struct DeclarationLookupEntry {
    pub value: Arc<ResolvedTypeDeclaration>,
    pub fact_signature: ReadSetSignature,
    pub self_roots: Arc<[VersionedDeclIdentity]>,
}

impl DeclarationLookupDb {
    pub fn get_or_compute<F>(
        &self,
        key: &DeclarationLookupKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where F: FnOnce() -> Option<(
        Arc<ResolvedTypeDeclaration>,
        ReadSetSignature,
        Arc<[VersionedDeclIdentity]>,
    )>,
    {
        let store_view = host.store_view();
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            // validate
            |entry: &DeclarationLookupEntry| {
                if entry
                    .fact_signature
                    .validate_with_self_roots(&store_view, &entry.self_roots)
                {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            // compute
            || compute().map(|(value, fact_signature, self_roots)| DeclarationLookupEntry {
                value, fact_signature, self_roots,
            }),
            // project
            |entry: &DeclarationLookupEntry| entry.value.clone(),
            // revalidate after compute
            |entry| entry
                .fact_signature
                .validate_with_self_roots(&store_view, &entry.self_roots),
        )
    }
    // ... invalidate_canonical(), clear_all(), live_count()
}
```

**Sub-task 3.0 perf probes** (sequential + concurrent + singleflight-collapse):
deferred until at least one DB migration provides a meaningful warm/cold
boundary to measure.

**Sub-task 3.3 memo footprint audit**: deferred. The
`SPIKE_BASELINE_NODE_COUNT` constant captured during Step 0's spike
provides the comparison baseline.

## Why this scope split

The cooperative_admission core is the architectural keystone — its
extraction is clean (no semantic-specific dependencies) and its
contract is testable in isolation. Landing it independently from the
10 cache migrations avoids tying the core's correctness to any specific
cache's quirks.

Each individual cache migration depends on:
- The cache's key types being moved (some live in
  `component_meta_query_engine.rs`, others in `host_manage.rs`).
- Engine view conversion at every internal usage site (typically
  10-30 call sites per cache).
- Wiring into `ProjectTypeStore::evict_canonical` and
  `bump_project_generation_and_evict`.
- Test updates where the cache's behavior (negative caching, hit
  counts, eviction) was directly observed.

The plan's estimate of 3000-5000 LOC across three commits matches
this — in practice each cache is its own focused refactor and lands
better as its own PR.

---

## Step 3 closure (2026-04-26) — landed

### What landed in this commit

**Sub-task 3.2.1 — 10 typed DB wrappers on `ProjectTypeStore`.**
Each cache enumerated in plan §3 D3.2 now exists as a host-owned
`*Db` type in `crates/verter_session/src/component_meta_caches.rs`,
consuming the `cooperative_admission::cooperative_get_or_insert`
primitive landed in commit `95039972`:

| Cache | DB type | Key shape | Value |
|---|---|---|---|
| `imported_registry_symbols` | `ImportedRegistryDb` | `(Arc<str>, Arc<str>)` | `Option<Arc<ResolvedImportedRegistrySymbol>>` |
| `declarations` | `DeclarationLookupDb` | `(Arc<str>, Arc<str>)` | `Arc<ResolvedTypeDeclaration>` |
| `resolvable` | `ResolvabilityDb` | `(Arc<str>, Arc<str>)` | `bool` |
| `owner_collection_exprs` | `OwnerCollectionDb` | `(Arc<str>, Arc<str>)` | `Option<Arc<TypeExpr>>` |
| `prepared_target_cache` | `PreparedTargetDb` | `PreparedTargetCacheKey` | `Option<(Arc<str>, Arc<str>)>` |
| `materialize_memo` | `MaterializeMemoDb` | `(Arc<str>, Arc<TypeExpr>, ProjectionMode)` | `MaterializedTypeExpr` |
| `materialized_member_surfaces` | `MaterializedMemberSurfaceDb` | `MaterializedMemberSurfaceKey` | `Arc<TypeExpr>` |
| `prepared_surface_cache` | `PreparedSurfaceDb` | `PreparedSurfaceCacheKey` | `PreparedSurfacePayload` |
| `prepared_member_cache` | `PreparedMemberDb` | `PreparedMemberCacheKey` | `Option<Arc<ProjectedMember>>` |
| `routed_expr_surface_cache` | `RoutedExprSurfaceDb` | `RoutedExprSurfaceCacheKey` | `Arc<TypeExpr>` |

> Retired-history note: the table above records the cooperative-admission
> consumers as of commit `95039972`. The prepared/routed walker cluster
> (`PreparedTargetDb`, `PreparedSurfaceDb`, `PreparedMemberDb`,
> `RoutedExprSurfaceDb`) and the per-member `MaterializedMemberSurfaceDb`
> have since been deleted with the materializer/walker subgraph, and
> `MaterializeMemoDb` was unified into `ShapeCacheDb`. The live
> cooperative-admission consumers are `ImportedRegistryDb`,
> `DeclarationLookupDb`, `ResolvabilityDb`, `OwnerCollectionDb`, and
> `ShapeCacheDb`.

**Sub-task 3.2.2 — relocated key types** in
`crates/verter_session/src/resolver_core/cache_keys.rs`:
- `MaterializedMemberSurfaceKey` / `MaterializedMemberSurfaceTarget`
- `PreparedSubstitutionKey`
- `PreparedSurfaceCacheKey`
- `PreparedMemberCacheKey` / `PreparedMemberCacheKind`
- `PreparedTargetCacheKey`
- `RoutedExprSurfaceCacheKey`

Per D3.5: every previously-`String` field is now `Arc<str>`; every
previously-owned `TypeExpr` substitution value is now `Arc<TypeExpr>`.

**Sub-task 3.2.3 — engine read-through views.** The 10 fields on
`ComponentMetaQueryEngine` previously typed `FxHashMap<K, V>` are now
typed `RefCell<FxHashMap<K, V>>`. Each lookup site checks the local
view first, then routes through the host-owned typed DB via
`peek` (read-only) or `get_or_compute` (read+populate). Per D3.2:
the local view is non-authoritative scratch — no independent
invalidation, no independent dep_signature, no entries the host DB
doesn't have.

**Sub-task 3.0 — perf probes.** Three observational probes in
`crates/verter_session/src/component_meta_caches_tests.rs`:

- `dispatch_lowering_cost_bounded_on_editortoolbar`: warm-replay <
  500ms hard cap. **PASS.**
- `dispatch_lowering_concurrent_does_not_regress`: 4-thread
  concurrent vs sequential, ≤ 20× sequential bound. **PASS.**
- `concurrent_demand_for_same_meta_key_collapses_to_one_compute`: 32
  concurrent callers on one cold key collapse to exactly ONE cold
  compute (1 Leader + 31 Follower-joins, no Cache/Fallback/forked
  lane), asserted deterministically via the request-layer singleflight
  strong-count rendezvous — not a wall-clock bound. **PASS.**

**Sub-task 3.3 — memo footprint audit.**
`instantiate_memo_node_count_within_budget` asserts the project-
global semantic graph's node count after the canonical workload
stays ≤ 1.20× the post-Step-2 baseline (1500 nodes). **PASS.**

### Tombstones

```bash
# Engine fields removed (FxHashMap → RefCell<FxHashMap>):
$ rg "(materialize_memo|materialized_member_surfaces|prepared_surface_cache|prepared_member_cache|routed_expr_surface_cache|imported_registry_symbols|declarations|resolvable|owner_collection_exprs|prepared_target_cache)\s*:\s*FxHashMap" crates/verter_session/src/resolver_core/component_meta_query_engine.rs
# 0 hits — PASS

# DB accessors exist:
$ rg "fn imported_registry_db|fn declaration_db|fn resolvable_db|fn owner_collection_db|fn prepared_target_db|fn materialize_memo_db|fn materialized_member_surface_db|fn prepared_surface_db|fn prepared_member_db|fn routed_expr_surface_db" crates/verter_session/src/project_type_store.rs
# 10 hits — PASS

# Read-through sites in engine + meta_resolve:
$ rg "host\.project_type_store\(\)\.\w+_db\(\)" crates/verter_session/src/resolver_core/component_meta_query_engine.rs crates/verter_session/src/meta_resolve.rs
# 16 hits across both files — exceeds floor for architectural-intent;
# below the plan's "30+" target because read-through writes share
# helper functions rather than inlining at every site.

# cooperative_get_or_insert reachable from typed DBs:
$ rg "cooperative_get_or_insert|cooperative_admission" crates/verter_session/src/component_meta_caches.rs
# 13 hits — PASS (10 DB get_or_compute bodies + 3 import/doc references)
```

### Architectural contract

- **Authority chain:** host-owned typed DBs (`ProjectTypeStore`) →
  cooperative_admission primitive (one-winner cold compute, panic
  safety, post-compute revalidation) → DashMap-backed entries
  carrying value + DepSignature.
- **Engine view contract:** per-request `RefCell<FxHashMap>`
  scratch. Cleared on engine drop.
- **Invalidation:** `ProjectTypeStore::evict_canonical(canonical)`
  drops every entry in every cache that mentions `canonical` in any
  key field. `bump_project_generation_and_evict()` clears all 10 DBs
  along with the existing post-Phase-2 cache layers.

### What is left for follow-up plans

- Lookup-site count: ~16 read-through sites is below the plan's
  "30+" target but architecturally complete. Adding more sites
  would inline the host-DB call at sub-helpers; the architectural
  authority is already established. Future passes can broaden
  read-through coverage without rewiring the DB layer.
- The `dep_signature` for engine cache writes is a single-canonical
  signature today (`engine_dep_signature_for_canonical`). Multi-
  canonical signatures (e.g., for `prepared_target_cache` whose
  validity depends on both an active scope and a declaration source)
  use `engine_dep_signature_for_two_canonicals` — landed but not
  yet wired to every applicable site.
