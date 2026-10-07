# Scheduler dependency-edge inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Dependency-edge store: `DepKey` → waiters keyed by edge sequence | Dependency gating, completion/cancellation fan-out, terminal-failure fan-out, Analysis demand | REQUIRED-lifetime | One entry per gated edge; linked at admission or pre-dispatch merge, unlinked with the waiter's `deps_remaining` entry, taken when the producer completes, cancels, fails or retires; cleared on reset | `src/dag/dep_edges.rs`, `src/dag.rs`, `src/dag/terminal_failures.rs` | always |
| Dependency-edge `(canonical, generation)` file index | Generation retirement of waiters, including producers that were never admitted | REQUIRED-lifetime | One entry per gated file dependency; enters with its first waiter, leaves with its last | `src/dag/dep_edges.rs` | always |
| `DagNode::deps_remaining` edge sequence | Direct edge removal on `complete`/`cancel` | REQUIRED-lifetime | One per outstanding edge of a live node | `src/dag.rs` | always |
| Edge sequence allocator | Edge-admission release order | REQUIRED-lifetime | One scalar per DAG; exhaustion panics before reuse | `src/dag/dep_edges.rs` | always |
| Blocker reference index: referenced canonical → `(owner, generation)` entry reference counts | File-removal scrub of Artifact blocker sets | REQUIRED-lifetime | Mirrors the live blocker registry; changes with each stored or taken entry; cleared on reset | `src/dag.rs`, `src/dag/blocker_registry.rs` | always |
| Per-canonical node, blocker-owner and terminal-failure indices as removal drivers | File removal cancellation and record scrubs | REQUIRED-lifetime | Existing indices; no new population | `src/dag.rs`, `src/dag/{blocker_registry,terminal_failures}.rs`, `src/scheduler/lifecycle.rs` | always |
| Per-dependency waiter vector, DAG-wide dependency-key scan and predicate node scan on removal | None; replaced by the edge store and the per-canonical indices | REQUIRED-lifetime, retired | No population or backing allocation | `src/dag.rs`, `src/scheduler/lifecycle.rs` | absent |
| `DepEdgeObservations::unlink_entries_visited` | none | OPTIONAL | Cumulative per DAG instance | `src/dag/dep_edges.rs` | `cfg(any(test, feature = "semantic-observe"))` |
| `DepEdgeObservations::retire_keys_visited` | none | OPTIONAL | Cumulative per DAG instance | `src/dag/dep_edges.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Task registry producer → waiters reverse wait index | Task retirement removes edges into the retired task | REQUIRED-lifetime | Mirrors the wait-for graph of the one shared registry; changes with each registered, dropped or retired edge | `src/tasks.rs` | always |
| Aggregate cancellation owner list with dead-owner popping | Aggregate job liveness and late-owner refusal | REQUIRED-lifetime | One weak entry per registration until the liveness probe meets it dead; dropped with the aggregate token | `src/cancellation.rs` | always |
| Aggregate owner probe examination counter | none | OPTIONAL | Per aggregate token | `src/cancellation.rs` | `cfg(test)` |

The `semantic-observe` counters compile out of default builds and add no
per-operation enabled check. The edge sequence is ownership state, not
history: it identifies a live edge and orders its release.
