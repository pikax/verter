## Cache Population Target Contract

Architectural target for the project-global cache cutover:

- Cache ownership is split into three reusable layers:
  - file artifact caches (`IndexedReady`, prepared declarations, route surfaces, owner import surfaces, optional analysis)
  - semantic query caches (resolved declaration identity, instantiated meaning, indexed access, projected members, mapped or conditional results, normalized reusable intermediates)
  - final result caches (e.g. final component-meta payloads)
- Final payload caches should hand out immutable `Arc` values. Cache backend choice is an implementation detail as long as concurrency, size bounds, and validation rules are preserved.
- Reusable semantic cache population must be path-independent. If the same semantic result is reached through different entry paths, a successful computation must populate the same shared cache entry.
- Broader successful results may backfill narrower reusable entries they actually satisfied.
- Narrower successful results must not claim broader work is cached.
- An `Expanded` result may satisfy and backfill `Shallow` or `Identity` for the same semantic key.
- A `Shallow` result may satisfy and backfill `Identity` for the same semantic key.
- A whole-surface projection may backfill per-member or per-indexed-access caches for the members or accesses it actually materialized.
- A narrow member or indexed-access result must not pretend sibling members or whole-surface projection are cached.
- Cancelled, superseded, interrupted, budget-exceeded, or partial results must not be promoted as warm shared cache entries.
- Versioned semantic nodes and final-result entries must also be sweepable. Project-global caching may be aggressive, but old identities must not accumulate forever across long editing sessions.
- Top-level live-host results must publish through a completion fence: record the touched dependency signature, revalidate before publish, retry at most 3 times on mid-flight changes, never warm shared caches with torn provisional or unstable results.

Concrete expectation:

- If `ProjectSurface(C, Expanded)` materializes member `"foo"`, a later `ProjectMember(C, "foo", Expanded)` should reuse that work.
- If `ProjectMember(C, "foo", Expanded)` ran first, that must not imply `ProjectSurface(C, Expanded)` is cached.

