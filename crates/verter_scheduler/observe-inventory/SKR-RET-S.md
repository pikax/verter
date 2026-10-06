# Scheduler removal inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
This inventory covers the removal transition in `src/scheduler/lifecycle.rs`.

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| DAG mutex hold across removal sweep, node unpublication, and source-root publication | Admission, completion, dispatch and source-root coherence | REQUIRED-lifetime | One removal transition; released before stranded-waiter wakes | `src/scheduler/lifecycle.rs` | always |
| Live `FileNode` incarnation and generation reads | Removal sweep, source-root `Absent` publication, admission crossing gate | REQUIRED-lifetime | Current node incarnation; captured values live only through the transition | `src/scheduler/lifecycle.rs` | always |
| Tombstone/removal epoch writes | Queued-request rejection and suppression of removed dependencies | REQUIRED-lifetime | Existing canonical deletion state until explicit source re-add/reset; not yet reclaimable under the current suppression contract | `src/scheduler/lifecycle.rs`; storage in `src/scheduler.rs` | always |
| Generation floor writes and DAG retirement floor updates | Restart generation selection and refusal of retired work | REQUIRED-lifetime | Existing canonical history until identity replacement permits reclamation | `src/scheduler/lifecycle.rs`, `src/dag.rs`; scheduler storage in `src/scheduler.rs` | always |
| Blocker, terminal-failure, deferred-blocker, auto-ingest, and edge cleanup | Dependency gating and removal liveness | REQUIRED-lifetime | Current work/owner records; removed within the lifecycle hold | `src/scheduler/lifecycle.rs` | always |
| `after_sweep` admission rendezvous | No production consumer; barrier-controlled regression test only | OPTIONAL | One test removal call; parameter and invocation absent in production | `src/scheduler/lifecycle.rs` | `cfg(test)` |

The change adds no event histories, attribution counters, or production trace
storage. Existing history stores above remain correctness-bearing until the
replacement identity and deletion-authority contracts are settled; this patch
does not claim their retirement or zero backing capacity.
