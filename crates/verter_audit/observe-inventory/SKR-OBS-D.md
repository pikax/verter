# Seeded observation inventory

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover existing measurement feature groups and production occupancy
accessors only. They are not an exhaustive bookkeeping sweep.

The coordinated gate enables the listed legacy gates. Independent legacy
opt-ins remain usable until consolidation replaces their source `cfg` sites.

## verter_audit

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `attribution`: `SiteCell` counter table, scope/allocator attribution hooks and report readers | none; benchmark/report readers only | OPTIONAL | Process-global cells; explicit `reset()` between measurement windows; scope guards retire on drop | `src/attribution/{table,scope,alloc,report}.rs` | `cfg(feature = "semantic-observe")` (legacy `attribution`) |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `attribution`: forwarding for work counters and CSS-domain chargeability readers | none; own tests and measurement harness only | OPTIONAL | Uses the audit crate's process-global cells and measurement reset window | `Cargo.toml` feature; producers call `verter_audit::attribution` | `cfg(feature = "semantic-observe")` (legacy `attribution`, enabled by benchmark opt-in) |
| `currency_probe`: currency chokepoint `probe_scope!` hooks | none; measurement harness only | OPTIONAL | Scope guard per invocation; totals retained/reset by workspace probe storage | Currency call sites using `verter_workspace::probe_scope!`; `Cargo.toml` forwarding | `cfg(feature = "semantic-observe")` (legacy `currency_probe`, enabled by benchmark opt-in) |
| `VerterHost::retention_snapshot()` current occupancy, retained-owner and charge fields | Host retention/resource inspection; cache and pin policy accounting | REQUIRED-lifetime | Derived snapshot of host-owned resident state; snapshot value lives with caller, underlying owners retire on close/eviction/drop | `src/host_lifecycle.rs`, `src/types.rs`; collection owners in `src/project_type_store.rs` | always |
| Typed store `len()` accessors (`AnalysisReadyDb`, `CompileCacheDb`, `DerivedRawCacheDb`, `DependencyCacheDb`) | Current host store occupancy and retention accounting | REQUIRED-lifetime | Derived from live collection occupancy; no separate visit history | `src/project_type_store.rs` | always |

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `currency_probe`: per-site calls/nanoseconds atomics, scope timers and probe readers | none; measurement harness only | OPTIONAL | Process-global site totals with explicit resets; each timer retires on scope drop | `src/currency_probe.rs` | `cfg(feature = "semantic-observe")` (legacy `currency_probe`) |
| `WorkspaceRead::resource_snapshot()` / `WorkspaceResourceSnapshot` current entries, retained bytes and resolution occupancy | Host retention snapshot and resource accounting; workspace construction's published-project check | REQUIRED-lifetime | Derived from collection lengths/occupancy under the owning read locks; returned snapshot lives with caller | `src/traits.rs`, `src/engine.rs`; exposed by `src/{filesystem,memory}.rs` | always |
| `MemorySnapshot::len()`, `OverlayStore::len()`, `EdgeStore::{file_count, reverse_dep_bucket_count}`, `PackageIndex::found_count()` | Workspace resource snapshot and resident-table accounting | REQUIRED-lifetime | Live collection occupancy, updated by collection insert/remove/bulk transitions | `src/{memory,overlay,exact_resolution,package_index}.rs` | always |

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `hotpath`: instrumented central DAG mutex and profiling hooks | none; profiling harness only | OPTIONAL | Profiler-owned capture window; mutex guards retire on unlock/drop | `src/scheduler.rs` (`new_dag_mutex` and profiling sites) | `cfg(feature = "semantic-observe")` (legacy `hotpath`) |

Only current occupancy/charge fields are classified above. Historical stale
sweep totals or high-water counters sharing an existing snapshot are not made
REQUIRED by this inventory. Required collection capacity/backing charges must
remain available as their owners extend the inventory; this seed adds no
duplicate histories, global counters, or new accounting implementation.
