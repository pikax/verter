# Resolver-context dispatch inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Per-method resolver-context port call counters (`count_resolver_context_call!` sites, `MethodCounter`, registry, `snapshot`/`reset` readers) | none; the `resolver_dispatch_profile` measurement example only | OPTIONAL | Process-global per-call-site counters, registered on first hit; zeroed by an explicit `reset()` between measurement windows | `src/resolver_core/dispatch_profile.rs` | `cfg(feature = "semantic-observe")` |
| `RequestSnapshot` / `RequestFlags` (cancellation checkpoint, project-generation clock, live aggregate clock reader) | Cancellation checkpoints, memo generation gates and fact-tracer basis installation on every request | REQUIRED | One request: captured at request admission, dropped with the request context; holds handles only, never sampled state | `src/resolver_core/resolver_context.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Port-method counter call sites on the request-bound adapter and owned-lowering implementations | none; the `resolver_dispatch_profile` measurement example only | OPTIONAL | Expand to nothing without the feature; counts live in the engine's counters | `src/resolver_core/{request_bound,owned_lowering_port}.rs` | `cfg(feature = "semantic-observe")` |
| Request snapshot captured by each request lifecycle | The engine's per-node cancellation, generation and clock reads for that request | REQUIRED | One request lifecycle | `src/resolver_core/{host_resolver_context,session_resolver_context}.rs`, `src/resolver_store.rs` | always |
| Direct-host request snapshot | Test-only direct-host resolver seam | REQUIRED-lifetime | One host; captured on first direct read | `src/lib.rs`, `src/host_construction.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_bench

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `resolver_dispatch_profile` example and `semantic_perf::dispatch_profile` lanes | none; measurement only | OPTIONAL | One measurement process | `src/semantic_perf/dispatch_profile.rs`, `examples/resolver_dispatch_profile.rs` | `cfg(feature = "semantic-observe")` (`required-features`) |
