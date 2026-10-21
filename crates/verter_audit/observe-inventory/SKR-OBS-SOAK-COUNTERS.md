# Narrow capture-off work observations

## verter_audit

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `observe::WorkObservedU64` | none; tooling work readers; no semantic decision | OPTIONAL | one relaxed atomic per selected owner field; zero-size without either feature | `src/observe/mod.rs` | `soak-support` or `semantic-observe`; neither enables the other |

## verter_type_engine

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| graph `misses`, `joined_waits`; `GraphWorkSnapshot` | none; tooling memo-claim and actual-join discrimination | OPTIONAL | two counters per graph, retired on graph drop; existing claim/join sites, one authority | `src/semantic_query_memo/{stats,producer}.rs` | `soak-support` adds these two counters and `work_snapshot`; broader observation/test builds retain their existing tallies |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `resolver_node_cache_misses`, `component_meta_resolved_state_recomputes`; `HostWorkSnapshot` | none; tooling host work discrimination | OPTIONAL | two counters per host, reset with existing provenance reset | `src/meta_provenance.rs`, `src/host_manage/component_meta_methods.rs` | `soak-support` or `semantic-observe`; full provenance remains observation-only |

The standalone session-soak compile-contract lane executes the public runtime
proof without dev-feature unification. No graph metric is a component-meta
producer proxy. The host resolver metric counts request results; recomputes
count producer entry, including early returns. Graph misses count memo claim
misses, which may include recursive requests or subscribers. Joined waits count
actual subscriptions at the existing increment boundary. Counters do not affect
admission, budgets, cache validity, cancellation, scheduling or lifetime.
