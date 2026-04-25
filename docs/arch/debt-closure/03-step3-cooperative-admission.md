# Step 3 — `cooperative_get_or_insert` extraction + DB migration plan

Source plan: `D:/tmp/architectural-debt-closure.md` (revision 10), Step 3.

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
    pub dep_signature: DepSignature,
}

impl DeclarationLookupDb {
    pub fn get_or_compute<F>(
        &self,
        key: &DeclarationLookupKey,
        host: &VerterHost,
        compute: F,
    ) -> Option<Arc<ResolvedTypeDeclaration>>
    where F: FnOnce() -> Option<(Arc<ResolvedTypeDeclaration>, DepSignature)>,
    {
        cooperative_get_or_insert(
            &self.entries,
            &self.inflight,
            key.clone(),
            // validate
            |entry: &DeclarationLookupEntry| {
                if HostFenceValidator::is_valid(&entry.dep_signature, host) {
                    Some(entry.value.clone())
                } else {
                    None
                }
            },
            // compute
            || compute().map(|(value, dep_signature)| DeclarationLookupEntry {
                value, dep_signature,
            }),
            // project
            |entry: &DeclarationLookupEntry| entry.value.clone(),
            // revalidate after compute
            |entry| HostFenceValidator::is_valid(&entry.dep_signature, host),
        )
    }
    // ... invalidate_canonical(), clear_all(), live_count()
}
```

**Sub-task 3.0 perf probes** (sequential + concurrent + thundering-herd):
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
