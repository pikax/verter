# Scheduler dependency-edge inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Dependency-edge store: `DepKey` → waiters keyed by edge sequence | Dependency gating, completion/cancellation fan-out, terminal-failure fan-out, Analysis demand | REQUIRED-lifetime | One entry per gated edge; linked at admission or pre-dispatch merge, unlinked with the waiter's `deps_remaining` entry, taken when the producer completes, cancels, fails or retires; reset drops every entry and releases the backing table | `src/dag/dep_edges.rs`, `src/dag.rs`, `src/dag/terminal_failures.rs` | always |
| Dependency-edge `(canonical, generation)` file index | Generation retirement of waiters, including producers that were never admitted | REQUIRED-lifetime | One entry per gated file dependency; enters with its first waiter, leaves with its last; reset releases the backing table | `src/dag/dep_edges.rs` | always |
| `DependencyOccupancy` (`SchedulerDag::dependency_occupancy`, `Scheduler::dependency_occupancy`): edges, gated dependencies, file-index canonicals/generations/dependencies, blocker-reference canonicals/entries, and the backing capacity of every resident hash table: the top-level tables plus the summed per-generation dependency sets and per-canonical blocker-reference entry tables, which keep their capacity while any member survives | Production occupancy count accessor | REQUIRED-lifetime | Derived on read from the resident tables under the DAG lock; no stored counter | `src/dag.rs`, `src/dag/dep_edges.rs`, `src/scheduler.rs` | always |
| `DagNode::deps_remaining` edge sequence | Direct edge removal on `complete`/`cancel` | REQUIRED-lifetime | One per outstanding edge of a live node | `src/dag.rs` | always |
| Edge sequence allocator | Edge-admission release order | REQUIRED-lifetime | One scalar per DAG; exhaustion panics before reuse | `src/dag/dep_edges.rs` | always |
| Blocker reference index: referenced canonical → `(owner, generation)` entry reference counts | File-removal scrub of Artifact blocker sets | REQUIRED-lifetime | Mirrors the live blocker registry; changes with each stored or taken entry; reset releases the backing table | `src/dag.rs`, `src/dag/blocker_registry.rs` | always |
| Per-canonical node, blocker-owner and terminal-failure indices as removal drivers | File removal cancellation and record scrubs | REQUIRED-lifetime | Existing indices; no new population | `src/dag.rs`, `src/dag/{blocker_registry,terminal_failures}.rs`, `src/scheduler/lifecycle.rs` | always |
| Per-dependency waiter vector, DAG-wide dependency-key scan and predicate node scan on removal | None; replaced by the edge store and the per-canonical indices | REQUIRED-lifetime, retired | No population or backing allocation | `src/dag.rs`, `src/scheduler/lifecycle.rs` | absent |
| `DepEdgeObservations::unlink_keys_compared`: edge-sequence comparisons made while removing single edges | none | OPTIONAL | Cumulative per DAG instance | `src/dag/dep_edges.rs` | `cfg(any(test, feature = "semantic-observe"))` |
| Per-thread edge-sequence comparison count behind `unlink_keys_compared` | none | OPTIONAL | Cumulative per thread | `src/dag/dep_edges.rs` | `cfg(any(test, feature = "semantic-observe"))` |
| `DepEdgeObservations::retire_entries_visited`: canonical buckets, generation buckets and dependency keys generation retirement traversed | none | OPTIONAL | Cumulative per DAG instance | `src/dag/dep_edges.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Task registry producer → waiters reverse wait index | Task retirement removes edges into the retired task | REQUIRED-lifetime | Mirrors the wait-for graph of the one shared registry; changes with each registered, dropped or retired edge | `src/tasks.rs` | always |
| `WaitGraphOccupancy` (`TaskRegistry::wait_graph_occupancy`): edges, producers with waiters, reverse-index entries, and the backing capacity of every resident hash table: both top-level tables plus the summed per-producer waiter sets, which keep their capacity while any waiter survives | Production occupancy count accessor | REQUIRED-lifetime | Derived on read from the resident tables under the registry lock; no stored counter | `src/tasks.rs` | always |
| Aggregate cancellation owner list with dead-owner popping and registration-time compaction | Aggregate job liveness and late-owner refusal | REQUIRED-lifetime | One weak entry per registration; the liveness probe pops dead owners off the end, and a registration that finds the list at its compaction length keeps only live owners, so the list never exceeds twice the owners live at the last compaction (at least 8); dropped with the aggregate token | `src/cancellation.rs` | always |
| Aggregate owner list compaction length | Registration-time compaction trigger | REQUIRED-lifetime | One scalar per aggregate token, reset by each compaction | `src/cancellation.rs` | always |
| Aggregate owner probe and compaction examination counter | none | OPTIONAL | Per aggregate token | `src/cancellation.rs` | `cfg(test)` |

The `semantic-observe` counters compile out of default builds and add no
per-operation enabled check. The edge sequence is ownership state, not
history: it identifies a live edge and orders its release.
