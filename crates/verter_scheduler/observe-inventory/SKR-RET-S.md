# Scheduler incarnation inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| DAG lifecycle hold across admission, removal, reset, snapshot publication and failure | Atomic admission, capacity and source-root coherence | REQUIRED-lifetime | One transition; callbacks and inbox sends run after unlock | `src/scheduler/{admission,lifecycle,completion,driver}.rs`, `src/scheduler.rs` | always |
| Checked process-unique incarnation allocator | New FileNode and work identities | REQUIRED-lifetime | One scalar per process; exhaustion refuses allocation before reuse | `src/node.rs` | always |
| FileNode incarnation, live generation and retirement marker | File admission and worker dispatch/publication | REQUIRED-lifetime | One node object; delayed Arc owners may retain a retired object until work drains | `src/node.rs`, `src/dag.rs` | always |
| FileNode submission lifetime and queued request lifetime stamp | Preserve queued edits through language re-home; reject inbox work across removal/reset | REQUIRED-lifetime | One queued request; no per-path deletion record | `src/node.rs`, `src/driver.rs`, `src/scheduler/{admission,lifecycle}.rs` | always |
| WorkNodeIdentity and DepKey incarnation fields | Dedup, dependency gating, cycle traversal, completion and failure | REQUIRED-lifetime | Current DAG work or dependency edge | `src/dag.rs`, `src/dag/terminal_failures.rs`, `src/scheduler/{identity,dependencies,completion,driver}.rs` | always |
| FileNode current-generation Source admission marker | Distinguish a queued Source producer or reload from a terminal producer at blocker registration | REQUIRED-lifetime | One node object; reset on generation advance, set on successful Source admission | `src/node.rs`, `src/scheduler/{admission,completion}.rs`, `src/scheduler.rs` | always |
| Auto-ingest tracking incarnation and conditional clears | Distinguish queued producer from dead producer without touching successor tracking | REQUIRED-lifetime | Existing bounded active producer tracking lifetime | `src/scheduler/dependencies.rs`, `src/scheduler.rs` | always |
| Tombstones and DAG retirement floors | None; replaced by queued stamps and live object admission | REQUIRED-lifetime, retired | No population or backing allocation | `src/scheduler.rs`, `src/dag.rs` | absent |
| Generation floors | External artifact publication/eviction and host base source revision uniqueness | REQUIRED-lifetime | Removed known canonical history until those consumers carry captured incarnation/source witnesses; unknown removals allocate none | `src/scheduler.rs`, `src/scheduler/lifecycle.rs`; migration and retirement: SKR-RET-FLOORS | always |
| Source-root and edge/blocker/failure cleanup | Source visibility, dependency gating and capacity release | REQUIRED-lifetime | Current records and existing source-root retention leases | `src/scheduler/{lifecycle,completion}.rs`, `src/scheduler.rs` | always |
| Stale completion refusal counter | Attribution and tests only | OPTIONAL | Scheduler instance | `src/scheduler.rs`, `src/scheduler/completion.rs` | `cfg(any(test, feature = "semantic-observe"))` |
| after_sweep admission rendezvous | No production consumer; regression barrier only | OPTIONAL | One test removal call | `src/scheduler/lifecycle.rs` | `cfg(test)` |

Internal file work can reuse a generation without aliasing a removed object.
The external restart fence remains necessary, so this inventory does not claim
that all three history populations have drained. No event history or production
per-operation feature check is introduced.
