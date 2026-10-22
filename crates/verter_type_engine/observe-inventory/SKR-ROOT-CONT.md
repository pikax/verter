# Pollable dependency handoff inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The interface itself is described in
[`docs/arch/root-continuations.md`](../../../docs/arch/root-continuations.md).

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `PARK_WAKER` (thread-local unparking waker) and `UnparkThread` | `blocking::block_on` polls with it and parks between polls; `verter_type_engine`'s blocking drive and every declaration-lowering synchronous entry go through it. This supersedes the copy the continuation runtime's inventory lists in `verter_type_engine/src/semantic_execution.rs`, which no longer exists. | REQUIRED | The thread | `src/blocking.rs` | always |

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CompletionSlot.wakers`, `CompletionSlot.next_registration` | `CompletionSender::send` takes every registered waker under the slot lock and wakes each after releasing it | REQUIRED | One completion handle, until it resolves | `src/job.rs` | always |
| `CompletionSubscription.registration` | `CompletionSubscription::poll` keeps or replaces it; its `Drop` withdraws exactly it | REQUIRED | One subscription | `src/job.rs` | always |
| `CompletionHandle::registered_wakers` | none; tests read it | OPTIONAL | Derived from live occupancy | `src/job.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclWorker.space` (`SpaceWaiters`: wakers with registration ids, next id) | `ShardHandoff::poll` registers on a full queue; the worker wakes every waiter after a dequeue, outside the lock; a handoff's `Drop` withdraws its own | REQUIRED | One worker; each entry until the worker takes it or its handoff withdraws it | `src/decl_lowering.rs` | always |
| `ShardReply` / `ReplyState` | The worker's `ReplySender::answer` publishes into it and wakes the registered poll; a closed reply fails the poll; an abandoned reply returns the answer to the worker to drop | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `ShardHandoff` (job until admitted, the worker's sender, space registration, reply, retained service) | The pollable entries and their synchronous drivers | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `LeaseHandoff`, `LeasedRunHandoff`, `LeasedBatchHandoff` (and the worker answers `LeaseAnswer`, `RunAnswer`, `ShardAnswer`) | `begin_acquire_lease*`, `begin_run_leased*` return them; `acquire_lease*` and `run_leased*` drive them | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `LeaseHandoff.profile` / `LeasedRunHandoff.profile` (sink and submission instant) and the answers' worker `timing` | The handoff profile records the queue, service and response split for the decl-handoff profile dump | OPTIONAL | One handoff | `src/decl_lowering.rs` | `cfg(feature = "semantic-observe")` |
| `ShardHandoff::admitted` | none; tests read it | OPTIONAL | Derived | `src/decl_lowering.rs` | `cfg(test)` |

No trace or history is added. The former uninhabited `HandoffStats`
stand-in for default builds is removed, because no default-build code
names a profile any more.
