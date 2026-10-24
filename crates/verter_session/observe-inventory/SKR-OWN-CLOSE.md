# Reusable host close and parse-release completion

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Rows cover the state, hooks and counts a host close and its release boundary
add or change. Every row is current ownership or accounting and stays on with
capture off; no coordination-only hook or optional measurement state was
added, so nothing new sits behind `test-support`, `soak-support` or
`semantic-observe`.

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `VerterHost::close` reclamation step (`reclaim_released_storage`: retired-version sweep, drained semantic storage, pool-only identity shed) | Host close and a completed release boundary; frees only storage whose last referent is gone | REQUIRED-lifetime | Runs once per close and per completed boundary; holds nothing | `src/host_lifecycle.rs` | always |
| `VerterHost::release_completion` / `HostReleaseCompletion` | Callers that need queued parse releases applied before reading the account (soak runner, retention measurements, public-host tests) | REQUIRED-lifetime | One boundary per call; borrows the host, owns one marker answer per worker shard | `src/host_lifecycle.rs` | always |
| `HostRetentionSnapshot.identity_pool_entries` | Retention snapshot consumers comparing a closed host with its pre-workload baseline | REQUIRED | Read live from the store's identity pool per snapshot | `src/types.rs`, `src/host_lifecycle.rs` | always |
| `VerterHost::close` flow-slice clear (`FlowSliceStores::clear_all`) | Host close: the content-addressed graph bundles, slice plans and lowered slices no cascade or scheduler reset reaches | REQUIRED-lifetime | Runs once per close; a reader holding a bundle keeps exactly what it holds | `src/host_lifecycle.rs` | always |
| `VerterHost::close` vocabulary release (`SemanticGraphStore::release_vocabulary`) | Host close, after the caches release their owners | REQUIRED-lifetime | Runs once per close; holds nothing | `src/host_lifecycle.rs` | always |
| Production semantic context minted at `ProjectTypeStore` construction | Every engine request that names the production context; makes the process-permanent record part of construction instead of the first workload | REQUIRED-lifetime | Process-permanent record, created once | `src/project_type_store.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclLoweringService::release_completion` marker job per spawned worker | `HostReleaseCompletion::wait` and owner-level `try_complete` | REQUIRED-lifetime | One marker per worker per boundary; answered after the worker drains its recorded releases | `src/decl_lowering.rs` | always |
| `ParseReleaseCompletion` / `ParseReleaseError` | The host release boundary | REQUIRED-lifetime | One boundary; owns per-shard answer and liveness receivers | `src/decl_lowering.rs` | always |
| `DeclWorker.alive` liveness channel | A boundary fails with `WorkerStopped` instead of waiting on a marker a stopped worker can never run | REQUIRED-lifetime | One per worker thread; disconnects when the thread exits | `src/decl_lowering.rs` | always |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `IdentityInterner::shed_pool_only` | Host close and completed release boundary | REQUIRED-lifetime | Drops pool-only entries and their charges; held identities stay pooled | `src/identity_interner.rs` | always |
| `SemanticGraphStore::release_drained_storage` / `DrainedStorageReport` | Host close and completed release boundary | REQUIRED-lifetime | Drains destroyed-node sidecars, sweeps the dependency-rail interner, releases idle dedup tables | `src/semantic_query_memo/release.rs` | always |
| `FlowSliceStores::clear_all` (graph, hash and lowered stores) | Host close | REQUIRED-lifetime | Drops every published entry and its charge; in-flight tables untouched | `src/cache_runtime/flow_slice_node.rs` | always |
| `SemanticGraphStore::release_vocabulary` / `NodeArenaCore::release_vocabulary`, and permanence re-established on a vocabulary intern hit | Host close | REQUIRED-lifetime | Unpins every permanent slot under one arena hold, then releases it through the ordinary last-owner path; a held node is never revoked | `src/semantic_query_memo/release.rs`, `src/semantic_query_memo/arena.rs` | always |
| `NodeArenaCore::release_idle_dedup_tables` | `release_drained_storage` | REQUIRED-lifetime | Releases a shard's table and granule charge only while it indexes no node | `src/semantic_query_memo/arena.rs` | always |
