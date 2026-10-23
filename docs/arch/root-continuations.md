# Pollable root computations and dependency handoffs

A host root computation (component metadata, compile) must be able to stop
when it needs work it does not own. A poll returns `Pending` with the
computation's state owned and suspended, and a race-safe wake subscription
registered. A later poll continues from that point, on any thread, without
redoing completed work. This page is the reference for the boundaries that
already behave this way. It also lists, as an inventory of the current
tree, the boundaries a root can still block on.

## Protocol shared by every pollable boundary

Every boundary listed under [Delivered boundaries](#delivered-boundaries)
follows the same rules:

- **Check and register atomically.** A poll checks readiness and, while the
  work is not ready, registers the poll's `Waker` inside the same critical
  section the producer takes to publish. A completion that lands between the
  check and the registration therefore still wakes the poller.
- **Wake after unlock.** A producer takes the registered wakers under its
  lock and wakes them only after releasing it. No wake runs under a
  producer, queue, flight or publication lock.
- **One registration per subscriber.** Re-polling with a waker that wakes the
  same task keeps the registration. Any other waker replaces it. Dropping
  the subscriber withdraws exactly its own registration and leaves a
  sibling's alone, even when the sibling registered the same executor waker.
- **A wake only schedules.** A wake means a later poll will make progress.
  It never runs the waiting computation, so a readiness callback cannot
  re-enter a driver.
- **Owned, `Send`, one poller.** Subscribers own their state and hold no
  borrow of the producer. They are `Send` whenever their result is, so a
  suspended subscriber may resume on another thread. One poller drives a
  subscriber at a time (`poll` takes `&mut self`).

## The external-boundary driver

`verter_execution::blocking::block_on(slice, poll)` is the only place a
pollable computation is driven to completion by blocking. It polls with a
waker that unparks the calling thread, and parks while the poll is pending.
With a `slice`, each park is bounded, so a computation whose stop is
observed only at its next poll (a cancelled request) is re-polled at least
that often.

The driver is reached only from synchronous public entries outside the
computation. No poll-facing value hands a computation a way to call it.
Each synchronous entry listed below is the driver applied to the pollable
boundary beside it, so there is one implementation of each handoff, not a
blocking copy next to a polling one. The semantic runtime's blocking drive
(`SemanticExecution::drive`, `SemanticTask::drive`) uses the same driver,
with a 2 ms slice so a stop stays observable.

## Delivered boundaries

### Scheduler completions

| Boundary | Owner |
| --- | --- |
| `CompletionHandle::subscribe() -> CompletionSubscription<T>` | `crates/verter_scheduler/src/job.rs` |
| `CompletionSubscription::poll(&mut self, &Waker) -> Poll<CompletionState<T>>` | same |

- **Result:** the handle's terminal `CompletionState`, which is `Ready`,
  `Failed`, `Superseded` or `Shutdown`. Every poll that observes it gets a
  clone, and re-polling after the answer keeps returning the same state.
- **Errors and cancellation:** a scheduler error, a supersession or a
  shutdown arrives as the terminal state itself. Dropping a pending
  subscription withdraws its registration and cancels nothing: sibling
  subscribers and blocking waiters on the same handle are unaffected.
- **Lifetime:** the subscription holds an `Arc` of the handle's slot, so the
  completion outlives the request that issued it. `CompletionSender::send`
  stays first-writer-wins. It wakes every registered subscription once,
  after releasing the slot's lock.
- **Scheduler lifecycle locks:** admission, stage completion, failure and
  retirement publish their terminal state while holding the DAG lock, and
  retain the taken wakers on that guard. The guard releases the DAG lock
  before delivering them. Reset additionally defers delivery until its
  serialization hold and driver join finish. State publication order and
  first-writer semantics therefore stay intact without invoking a completion
  callback inside those critical sections. Exclusively owned standalone DAGs
  deliver immediately. If this scheduler's own driver re-enters an already
  active reset, that reset retains the quiesce obligation and the driver
  returns without waiting on its joiner. The exemption uses the exact driver
  thread identity; another scheduler's driver still waits for the reset
  barrier.
- **Own-driver reset and restart:** when no external reset owns the barrier,
  a reset from a completion wake clears state and leaves shutdown set. The
  current driver stops after that callback returns; its identity and join
  handle remain owned by the scheduler. External restart joins it before
  reopening work. A reset followed by restart on that same driver reopens
  its existing loop, without spawning another driver. Native pump teardown
  rejects queued requests and dispatches no new stages while shutdown is
  set. A reset racing a healthy lifecycle hold waits in bounded parked
  slices; it returns early only once shutdown proves the holder may be
  joining this callback. Startup gates all work until identity and handle
  publication and lifecycle unlock. Joins hold no driver-handle mutex.
  `scheduler::reset_tests` exercises these paths using real completion wakes
  and retains cleanup guards that release dependencies and join test threads.
- **Backpressure:** none at completion; admission has its own handoff below.

### Scheduler request admission

`Scheduler::begin_submit_request` returns an owned
`SchedulerSubmission<CompletionSubscription<RequestResult>>`.
`begin_submit_batch_atomic` returns the same handoff over a vector of
subscriptions in input order. Neither the handoff nor its output exposes a
blocking completion driver. Its only advancing operation is `poll(&mut self,
&Waker)`.

- **Backpressure:** a full inbox retains the whole submission (one item for
  an atomic batch) and returns `Pending`, without draining the inbox or
  executing a stage. The offer and registration share the capacity lock;
  consumers take the registrations after dequeue and wake after unlock.
- **Result and errors:** `Ready` transfers the completion subscription(s).
  Inbox admission is distinct from stage completion; poll the subscriptions
  separately. If shutdown rejects admission, those subscriptions are already
  terminal with `Shutdown`. An empty batch returns an empty vector without
  queue or submission accounting. Re-polling a completed admission panics.
- **Lifetime and cancellation:** construction captures each request's file
  lifetime using the same preparation as synchronous submission. The handoff
  retains the scheduler and all unsubmitted inputs. Dropping it withdraws
  only its own capacity registration and discards unsubmitted work. Once
  admitted, dropping a completion subscription leaves sibling consumers and
  the producer intact; request-context cancellation keeps its existing
  scheduler semantics. Reset invalidates the captured file lifetime, so a
  retained request cannot republish a retired incarnation after reset.
- **External drivers:** `submit_request`, `submit_batch_atomic` and the
  scheduler's internal synchronous sends drive the same admission state.
  Only these compatibility drivers may drain an older inbox item to avoid
  parking the sole consumer behind its own full queue. Stage execution stays
  with the existing scheduler driver or an explicitly driven `drive_one`.
  This handoff introduces no executor or pool.

### Scoped cache-node admission and producer transfer

`Scheduler::begin_scoped_cache_node<T>` returns `ScopedCacheNodeHandoff<T>`.
Its `poll` yields while the bounded inbox is full, until DAG dispatch, or
while another caller produces the shared value. It never executes a stage,
waits, enters a pool or runs a producer. The synchronous
`execute_scoped_cache_node` drives this same handoff and runs its transferred
producer on the existing CPU pool.

- **Producer ownership:** the first eligible poll after dispatch receives
  `ScopedCacheNodeStep::Produce`. That intermediate result transfers one
  `ScopedCacheNodeProducer<T>` to the external driver. Its `run` installs the
  request context, aggregate cancellation and active-path identity, executes
  the supplied closure, contains panic and publishes once. Dropping it
  without running settles the flight as cancelled. A later poll of the
  handoff receives the terminal `Value(Arc<T>)`; every surviving follower
  receives that same value. Producers can borrow the external caller's data.
- **Task ownership and cycles:** the host injects its single `TaskRegistry`
  into both the scheduler and semantic store. The explicit scheduler
  constructors are `with_executor_and_task_registry` (threaded native) and
  `new_sync_with_executor_and_task_registry` (inline native/WASM); standalone
  constructors create a registry for their scheduler. A handoff captures the
  installed task at construction, or registers a new external-root task. Its
  producer retains and installs that task wherever `run` executes, restoring
  the destination thread's prior task afterwards. A follower holds one RAII
  edge to the elected producer in that same registry. Same-task recursion or
  a cross-task cycle returns the existing `Reentrant` refusal to that follower;
  surviving owners keep the producer. Observing completion/refusal or dropping
  the handoff releases its edge. Election wakes earlier subscribers to register their
  edges now that the producer is known. The handoff that transferred the
  producer may await its external execution without a self-edge. Wakes still
  schedule later polls and run beyond the flight/lifecycle holds.
- **Backpressure and lifetime:** admission retains the unsubmitted item and
  separate capacity and flight registrations. A sibling's settlement
  can bypass that item's capacity wait; a retired flight resolves
  even if the successor inbox is full. The handoff and producer retain the
  scheduler independently and can move between threads with exclusive
  polling. Dropping a handoff withdraws its registrations and detaches only
  its owner; a surviving sibling keeps shared work live.
- **Errors:** cancellation, shutdown/reset, panic, mismatched result type and
  same-path re-entry retain the existing typed outcomes. Cancellation is
  checked at each poll; cancellation-only wake delivery remains unfinished.
  Executors must re-poll or drop on request
  cancellation. Polling after a terminal result is a contract violation.
- **Wake ordering:** dispatch and settlement take registrations under the
  flight lock. Delivery uses the scheduler's deferral rail, after flight,
  scoped-incarnation and DAG locks release, including reset's outer hold.
  Wakes request a later poll and do not execute the producer.
- **Evidence:** `scheduler::scoped_handoff_tests` exercises yielding before
  queue/result release, cross-thread producer transfer, follower isolation,
  panic/drop, re-entry, reset with a full successor inbox, and unlocked wake
  delivery. The session compile contract
  `scoped_handoff_no_blocking_scheduler` denies recovering the blocking
  scheduler from the handoff. These are boundary tests; complete production
  component-meta/compile roots are still synchronous.
  `scoped_cache_handoffs_share_the_semantic_wait_graph` holds real semantic
  and scoped producers, closes a cycle in both dependency orders, and runs
  the surviving producer on another thread under its original task.

### Declaration-lowering owner-shard handoffs

Retained parses live on dedicated worker shards and never leave them. A job
carries owned inputs to the shard and brings back owned results.

| Pollable boundary | Synchronous entry over it | Answer |
| --- | --- | --- |
| `DeclLoweringService::begin_acquire_lease -> LeaseHandoff<()>` | `acquire_lease` | `(LeaseOutcome, None)` |
| `DeclLoweringService::begin_acquire_lease_and_run -> LeaseHandoff<R>` | `acquire_lease_and_run` | `(LeaseOutcome, Option<R>)` |
| `DeclLoweringService::begin_run_leased -> LeasedRunHandoff<R>` | `run_leased` | `Option<R>`, where `None` is a lease miss |
| `DeclLoweringService::begin_run_leased_batch -> LeasedBatchHandoff<R>` | `run_leased_batch` | `Vec<Option<R>>` in input order |

All of these are in `crates/verter_semantic_source/src/decl_lowering.rs`.

- **Backpressure yields.** A poll first offers the job to its worker's
  bounded queue with a non-blocking send. If the queue is full, the poll
  keeps the job, registers for the space the worker frees when it dequeues
  a job, and returns `Pending`. The offer and the registration happen under
  the queue's waiter lock, and the worker takes that lock only after a
  dequeue, so space freed in between still wakes the submitter. No send
  ever blocks behind a poll.
- **Result:** once the queue admits the job, a poll takes the worker's answer
  or registers for it in the reply's lock. The worker's answer takes that
  same lock. A job that panicked on the worker re-panics on the polling
  thread. For a lease acquisition, it re-panics only after the lease was
  handed back, so the pin is released as the panic unwinds. If a worker
  stops without answering, the reply is closed and the poll panics, as the
  blocking rendezvous it replaced did.
- **Cancellation and lifetime:** dropping a handoff withdraws whichever
  registration it holds and abandons its reply. If the answer has already
  arrived, it is dropped. If it arrives later, the worker drops it on its
  own thread. Either way, a lease acquired for an abandoned handoff is
  released on the shard that pinned it, so no live lease and no retained
  parse outlives its handoff. While a handoff is outstanding it holds the
  service, and so its workers, alive. Polling after the answer is a
  contract violation.
- **Batches:** each poll advances every share still outstanding, so every
  shard's share is offered on the first poll and the shards lower in
  parallel. The batch answers once every share has answered, and the first
  job panic re-panics then.
- **wasm32:** the shard is the calling thread's own, so a handoff answers at
  submission and its first poll is `Ready`.

## Boundaries that still block

These are the production waits a component-meta or compile root can reach
in the current tree. Each one parks the polling thread inside whatever poll
reaches it, so a root that reaches one is not yet pollable. Semantic memo
flights and scoped cache-node followers share `TaskRegistry` wait edges;
the remaining families do not yet register their waits there.

| Wait | Owner | Reached from | Pollable form today |
| --- | --- | --- | --- |
| Semantic memo flight join (`SemanticGraphStore::acquire_query` → `Subscription::wait`) | `verter_type_engine/src/semantic_query_memo/producer.rs` | every synchronous `ProjectSemanticDispatch` query a step evaluates in place | `Subscription::poll`, used only by `SemanticTask` frames |
| Semantic task drive (`SemanticTask::drive`) | `verter_type_engine/src/project_semantic_dispatch/query_frames.rs` | `drive_query` for instantiations, `typeof` and callback-typing flow returns | `SemanticTask::poll`, test drivers only |
| Artifact cache singleflight joiner (`cooperative_admit_impl`) | `verter_type_engine/src/cache_runtime/singleflight.rs` | flow-slice lookups; declaration-lookup, resolvability, owner-collection and shape-cache memo DBs | private `SlotJoin::poll`; synchronous callers still drive it to completion |
| Query-identity candidate singleflight joiner (`cooperative_admit_with_lookup_publish`) | `verter_type_engine/src/cache_runtime/lookup_publish.rs` | imported-registry symbol resolution | same `SlotJoin::poll`; callers still synchronous |
| Resolver request singleflight follower (`SingleflightGroup::run_retaining`; thread-identity owner) | `verter_session/src/resolver_core/mod.rs` | `run_stable_request` ← the cold component-meta resolve; fallthrough requests; module exports, overlays, prepared declarations, source requests | private owned `FlightJoin::poll`; callers still synchronous |
| Store-view build park (`StoreViewManager::park_for_build`) | `verter_session/src/resolver_store.rs` | `capture_batch_fixed_view` at every component-meta entry | private `BuildRelease::poll`; callers still synchronous |
| Carrier publication lanes (`publish_or_get`, `await_stable_parse`) | `verter_session/src/carrier_publication_store/mod.rs` | the scheduler's source stage; session overlay structure | private `LaneWait::poll` and stable-parse poll; callers still synchronous |
| Host load (`CooperativeSchedulerAdapter::drive` → `Scheduler::wait_or_drive`) | `verter_session/src/cooperative_scheduler.rs`, `verter_scheduler/src/scheduler.rs` | `VerterHost::ensure_loaded` from component-meta registry/scope producers, prepared declarations, eval programs/environments, the frontier engine and compile entries | `CompletionSubscription::poll` on the handle; the adapter still parks |
| Batch upsert completion (`Scheduler::wait_batch`) | `verter_scheduler/src/scheduler.rs` | `PreparedUpsertBatch::finish` ← `compile_many` | as above |
| Scoped cache-node producer drive (`execute_scoped_cache_node`; external polling plus a rayon `install`) | `verter_scheduler/src/scheduler/scoped_handoff.rs` | Vue macro codegen in compile and the Vue API projector | `ScopedCacheNodeHandoff::poll` transfers a producer to the external driver; semantic callers still use the synchronous wrapper |
| Scheduler inbox submission (`send_submission`) | `verter_scheduler/src/scheduler/submission.rs` | `submit_request`, `submit_batch_atomic`, scoped cache nodes | owned `SchedulerSubmission::poll` for requests/batches; existing root callers still use synchronous wrappers |
| Workspace import-resolution flight (`FlightSubscription::wait`) | `verter_workspace/src/resolution_flights.rs` | `resolve_import_outcome*` from host loading, the analysis stage and compile hydration | `FlightSubscription::poll` registers a waker; callers still synchronous |
| Host CPU pool batch (`HostBatchCoordinator::run_batch` → `HostCpuPool::install`) | `verter_session/src/host_batch_coordinator.rs` | `compile_many`, `get_component_meta_batch` | none; the batch topology itself is the executor's to replace |

The declaration-lowering waits that used to be on this list are now the
pollable handoffs above. Their synchronous entries, which the declaration
body memo calls from inside semantic evaluation, still block through the
external driver. What remains is to make the computations that call them
suspend instead.

## What a pollable root still needs

The cold component-meta root (`VerterHost::component_meta_via_view_cold`)
is one synchronous computation. It resolves macros through
`ProjectSemanticDispatch`, extracts the analysis (which recurses into
imported components' fallthrough), and publishes through
`ComponentMetaResultPublish`. Compile is synchronous in the same way. Every
wait in the table above can be reached from inside a nested synchronous
call. A root becomes pollable only when each such call either answers
without waiting or suspends as owned state. The suspended state includes
the frames of the semantic runtime (`SemanticExecution`), and resumption
continues the suspended computation; it does not re-run it from the root.
Converting the low-level waits alone does not achieve this. The callers
that reach them have to become resumable steps too.
