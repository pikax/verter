# Pollable root computations and dependency handoffs

A host root computation (component metadata, compile) must be able to stop
when it needs work it does not own. A poll returns `Pending` with the
computation's state owned and suspended, and a race-safe wake subscription
registered. A later poll continues from that point, on any thread, without
redoing completed work. This page is the reference for the host roots and
the boundaries that already behave this way. It also lists, as an inventory
of the current tree, the boundaries a root can still block on.

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
- **Cancellation wakes too.** A pending poll that a cancellation must stop
  also registers its waker on that cancellation, so the cancellation itself
  wakes it and the next poll answers the cancellation. No boundary re-polls
  on a timer to notice a stop.

## Host roots

A host root is one public request as an owned computation. It is begun
without running any of it, and an external driver polls it to its answer.

| Entry | Root | Answer | Synchronous entry over it |
| --- | --- | --- | --- |
| `VerterHost::begin_component_meta_output(canonical_or_alias)` | `ComponentMetaOutputRoot<'h>` | `ComponentMetaOutputAnswer` | `get_component_meta_output` |
| `VerterHost::begin_compile_request(canonical_id, request)` | `CompileRequestRoot<'h>` | `CompileRequestAnswer` | `compile_request` |

All of these are in `crates/verter_session/src/root_continuation.rs` and
re-exported from the crate root with `RootWork`.

- **Ownership and lifetime.** A root borrows only the host (`'h`). It owns
  its request, every input it captures (the session view, the store-view
  capture, the compile input) and the suspended state of the wait it is in,
  so it outlives the call that began it. It is `Send`, and `poll` takes it
  exclusively, so a pending root may resume on any thread, one poller at a
  time. The session crate's tests assert the `Send` bound.
- **Ambient request state.** A root runs under the request context and the
  scheduler job its creator ran under, captured when it begins, and as one
  execution task of the host's `TaskRegistry`: the task its creator runs as,
  or one registered for it. Each poll installs the three on the polling
  thread and restores the thread's own afterwards; a root begun outside any
  request clears the poller's request slots for the poll instead. The task
  owns every producer the root claims and is the waiter of every flight it
  joins, so its same-path recursion and its wait cycles are decided by task
  across threads. Dropping a pending root releases its suspended state
  under the same ambient state.
- **Result and errors.** `Ready` carries exactly what the synchronous entry
  returns, including its typed failures. Polling after `Ready` panics.
- **Dependency work.** A root never runs a dependency's producer inside its
  own poll and never drives the scheduler that dispatches its dependencies.
  When it needs either, the poll returns `Pending` and leaves one
  `RootWork` for the driver (`take_work`). The driver runs it wherever it
  chooses — `RootWork::is_producer` separates CPU work from a scheduler
  dispatch step — and polls again; `RootWork::run` answers whether the work
  made progress (a dispatch step that dispatched nothing returns `false`,
  and the dependency's own settlement then wakes the root). Nothing else
  wakes a root that handed out work. Dropping the work abandons the
  dependency it carried, and the next poll answers that abandonment.
- **Backpressure.** A root waits on admission like any other pending
  dependency; it adds no queue of its own.
- **External driving.** The synchronous entry drives the same root with
  `verter_execution::blocking::block_on(WaitFamily::Root, ..)` on its own
  thread, running handed-out producers where the scheduler runs producers
  (`Scheduler::run_transferred_producer`: its CPU pool natively, the caller
  on wasm32) and dispatch steps on its own thread
  (`Scheduler::dispatch_for_pending_dependency`). That is the only place the
  pool binding appears; a root's poll never enters it.

What each root suspends on today:

- **Component-metadata output.** The store-view capture it starts with
  suspends behind a build another thread holds (see
  [Store-view reads](#store-view-reads)). Its resolution, extraction and
  output materialization still enter their dependency waits synchronously
  inside the poll; see [Boundaries that still block](#boundaries-that-still-block).
- **Compile request.** The macro bundle it compiles with is produced on a
  pollable path: the store-view read suspends; the scoped cache node
  suspends while another root produces it and while it waits for dispatch
  (handing out a dispatch step); and when the node elects this root its
  producer, the root hands the producer out instead of running it. The
  backend and its compile arena are created after the bundle is produced,
  so neither is held across a suspension, and parse-stack refusals are
  recorded across polls (`refusals_within_pollable`). The macro traversal
  inside the handed-out producer runs synchronously wherever the driver
  runs it; its own dependency waits are the semantic engine's.

Evidence in `verter_session`'s `root_continuations_tests` and
`root_continuation::tests`: a root begun by a function that returns it is
polled to its answer on another thread and answers what the synchronous
entry answers; a root resumed on another thread runs under its own task and
request and leaves the polling thread's state as it was; a root begun inside
a task adopts it; the component-metadata root suspends behind a held
store-view build and resumes warm without entering a wait; dropping it there
withdraws its wait; the compile root's polls enter no wait under a
forbidding probe, cold and warm, handing out its producer once; and a second
compile root following the first's held producer answers `Pending` and
completes once that producer runs. One thread drives three compile roots
— two of one request and one of another — begun before driving starts:
every poll answers or suspends without entering a wait, each shared
bundle is produced once by the work its first root hands out while the
other waits, a follower's wake arrives while the driver runs that work and
only queues a later poll, and every answer is the synchronous entry's.
Cancelling a suspended follower root's request wakes it to its
cancellation while the producing root completes, and a scheduler reset
wakes it to the shutdown. Planted controls fail the same checks: the
synchronous capture behind a held build is refused, and running the macro
producer inside the compile root's poll fails the compile cases. The
compile contracts `root_external_consumer_positive` (a plain external
consumer drives both roots, with no support feature) and
`root_poll_no_blocking_driver` (the boundary driver is unreachable from a
root) pin the polling surface; making the driver public fails the second.
On `wasm32-unknown-unknown`, where the scheduler and the owner shards run
inline on the one thread, `verter_wasm`'s `host_root_wasm_tests` (run by
the wasm JS-boundary lane) drive a compile root by the dispatch steps and
the producer it hands out, and poll a component-metadata root to the
synchronous entry's answer.

## Execution tasks across suspension

Producers and waits are owned by execution tasks, never by threads, wherever
a computation can suspend while it holds them:

- **Semantic memo flights and scoped cache nodes** register task wait edges
  in the host's one `TaskRegistry` (below).
- **Resolver request flights** (`SingleflightGroup`, used by stable
  requests, fallthrough requests, imported roots and IndexedReady
  materialization) take the host registry at construction. A lane's leader
  is the task that claimed it (`FlightInner::Running { owner: TaskId }`).
  The flight protocol is one pollable computation
  (`run_retaining_pollable`), whose leader may suspend inside its compute;
  `run_retaining` drives it, as a task installed for its extent. A claim by
  the leading task — same-path re-entry — computes inline as a nested
  leader; any other task registers its `follower -> leader` edge first and
  follows, and a wait that would close a cycle computes inline instead.
  Dropping a suspended leader aborts the lane so followers re-elect;
  dropping a follower withdraws its registration and edge. Evidence:
  `resolver_core::singleflight_task_tests` (two tasks on one thread share
  one leader; re-entry and cycles compute inline; dropped leaders and
  followers release), each failed by an applied thread-style or edge-less
  mutation.
- **Workspace resolution flights** decide "this thread owes a flight, so it
  resolves directly" from the thread's lease count. A `FlightLease` is not
  `Send`, so a suspending computation cannot hold one across a suspension
  and the count is exactly the leases the polling computation owes; a
  compile-time assertion in `resolution_flights` fails once a lease is
  `Send`.
- **Store-view build claims** are taken, built and released within one poll;
  a reader never suspends while it holds a claim.

## Interior state a suspended computation shares

`ProjectSemanticDispatch` keeps its request state in
`verter_execution::sync_cell::{SyncRefCell, SyncCell}`, which keep the borrow
discipline of `RefCell` and `Cell` — a conflicting borrow panics, it never
waits — with an atomic flag, so the dispatcher is `Sync` and a computation
that suspends while it holds a shared reference to it is `Send`. Borrow
guards are not `Send`: a borrow held across a suspension point does not
compile. The session pins the dispatcher's `Sync` bound.

## Cancellation subscriptions

`CancellationToken::subscribe() -> CancellationSubscription`
(`crates/verter_execution/src/cancellation.rs`) is the cancellation's own
pollable boundary. `poll(&mut self, &Waker)` answers `Ready` once the token
is cancelled and otherwise registers the waker under the token's waker
lock; `cancel` sets the flag and then takes and wakes every registration
outside that lock, so a cancellation racing a registration is never lost.
Dropping a subscription withdraws exactly its own registration and cancels
nothing.

An aggregate job token wakes its subscribers when it becomes cancelled:
when the last live owner's request cancels (each owner's request token
carries a registration that re-evaluates the aggregate) or when the last
live owner detaches. A requester cancelling while a sibling keeps the job
live wakes nothing. The waker lock is never taken while an aggregate's
owner list is held.

Users: the semantic runtime registers on the token its stop selects
(`Program::wake_on_stop`, called whenever a poll returns `Pending`; for
`QueryProgram` the job token when it has owners, else the request's, else
the ownerless job's), so a pending `SemanticTask` is woken by the
cancellation that stops it. The scoped cache-node handoff registers on its
request's token while pending. The synchronous semantic flight wait, the
carrier publication and stable-parse lanes and the workspace flight wait
register on their cancellation and park untimed.

## The external-boundary driver

`verter_execution::blocking::block_on(family, slice, poll)` drives a
pollable computation to completion by blocking. It polls with a waker that
unparks the calling thread, and parks while the poll is pending. Every
driver over a cancellable wait passes no `slice`: the poll registers on its
cancellation, which wakes the thread. The `slice` parameter remains for a
caller that must bound its park for another reason.
`block_on_when_pending(family, poll)` is the same driver for a computation
that usually answers at once and waits only behind another caller's work
(the store-view read): it enters the `family` wait only when its first poll
is pending.

Each synchronous entry listed below is the driver applied to the pollable
boundary beside it, so there is one implementation of each handoff, not a
blocking copy next to a polling one. The synchronous host entries drive
their roots ([Host roots](#host-roots)) as `WaitFamily::Root`. The semantic
runtime's blocking drive (`SemanticExecution::drive`, `SemanticTask::drive`)
uses the same driver, untimed, and is woken by its stop's cancellation.

The target is that only a synchronous public entry outside the computation
reaches the driver. The current tree does not meet it: the component-
metadata root still reaches the driver, and the other waits in
[Boundaries that still block](#boundaries-that-still-block), from deep
inside its resolution, and so does the macro traversal a compile root hands
its driver. No poll-facing value hands a computation a way to call it.

## Measuring what a root still reaches

Every synchronous wait on a dependency announces itself on entry through
`verter_execution::blocking::enter_wait(WaitFamily)`, before it checks
readiness, locks or parks; `block_on` takes the family as its first
argument and announces it. `WaitProbe::record()` lists the families the
installing thread enters; `WaitProbe::forbid()` makes entering any of them
panic, naming the family, before the wait has any effect. Probes cover the
installing thread only and nest (an inner probe under a forbidding one
forbids too). The probe is test evidence: it compiles only under `test` or
`verter_execution`'s `test-support` feature, and a production build keeps
each announcement as an empty inline call.

`verter_session`'s `root_continuations_tests` group runs the public roots
under a recording probe over an owner component that imports a props
module and a child component. The component-metadata rows poll the root
directly, with a loop that enters no wait of its own:

| Path | Families reached by the polling thread |
| --- | --- |
| Host construction and source registration | `SchedulerCompletion`, `SchedulerAdmission` |
| Cold component-metadata root | `SemanticDrive`, `DeclLowering` |
| Warm component-metadata root | none, uncontended |
| Mixed: child warm, owner cold after a props-module edit | `SemanticDrive`, `DeclLowering` |
| Compile root polls (runtime client and IDE companion), cold or warm | none: the root hands its macro producer to its driver |
| Synchronous `compile_request`, cold or warm | `Root` (its driver), `CpuPool` (the handed-out producer, on the scheduler pool) |
| Batch compile (`compile_many`), cold or warm | `CpuPool`; each input runs on a pool thread outside the probe |
| Host drop | none |

A single-request run never reaches the contention-only families
(`SemanticFlight`, `CacheFlight`, `ResolverFlight`, `StoreViewBuild`):
another request's flight or store-view build has to be in progress. The
tests pin each path's reached set: a conversion that removes a family from
a path fails its pin until this table and the pin move together.

Beneath the cold component-metadata root's two families, a backtrace of
every wait it enters on that fixture shows fourteen synchronous waits,
all but two of them declaration lowering on the owner shards: the owner's
and its imports' IndexedReady builds (`ensure_indexed_ready_serve_located`
→ `acquire_lease_and_run`, reached from the request's input capture, the
local symbol metadata and the owner's direct-import indexing), whole-file
evaluation environments (`DeclBodyMemo::whole_env`, reached from local type
declarations, the fallthrough environment and value-export peeling), macro
type arguments (`transient_macro_type_argument`), and declaration bodies
demanded by instantiations and locator lowering inside the semantic engine
(`type_decl_outcome_in`, `transient_type_parts_in`); the remaining two are
`SemanticTask::drive` for instantiations reached from the shallow path
walker. They sit between 24 and 138 native frames below the root's poll.

The same group holds the Pending-before-release harness; its cases over a
held semantic flight (`root_continuations_harness_*`) live in
`semantic_task_tests`, beside the fixtures that hold the flight. It polls once on
its own thread under a forbidding probe and a bounded deadline while the
test holds a real dependency (another task's semantic flight). A pollable
subscriber answers `Pending`, is woken by the release and answers. Two
planted controls fail it: the synchronous `Subscription::wait` is refused
by the probe at once, and an unannounced park fails at an injected 50 ms
deadline. In both, the harness releases the hold and joins its thread
before reporting. Removing the `SemanticFlight` announcement turns the
first control into a deadline failure, so the announcement is what the
refusal rests on.

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
  checked at each poll, and a pending handoff is registered on its request's
  cancellation, so cancelling the request wakes it and the next poll answers
  `Cancelled` without disturbing the producer or sibling followers. Polling
  after a terminal result is a contract violation.
- **Wake ordering:** dispatch and settlement take registrations under the
  flight lock. Delivery uses the scheduler's deferral rail, after flight,
  scoped-incarnation and DAG locks release, including reset's outer hold.
  Wakes request a later poll and do not execute the producer.
- **Evidence:** `scheduler::scoped_handoff_tests` exercises yielding before
  queue/result release, cross-thread producer transfer, follower isolation,
  panic/drop, re-entry, reset with a full successor inbox, and unlocked wake
  delivery. The session compile contract
  `scoped_handoff_no_blocking_scheduler` denies recovering the blocking
  scheduler from the handoff.
  `scoped_cache_handoffs_share_the_semantic_wait_graph` holds real semantic
  and scoped producers, closes a cycle in both dependency orders, and runs
  the surviving producer on another thread under its original task.
- **Production use.** The compile root awaits the Vue macro bundle's node
  through this handoff and hands its producer and dispatch steps to its
  driver as `RootWork` ([Host roots](#host-roots)); the synchronous macro
  bundle entry and the Vue API projector still use
  `execute_scoped_cache_node`. Both drivers share one request, one producer
  body and one delivery (`vue_macro_codegen_request`,
  `build_vue_macro_codegen`, `finish_vue_macro_codegen`), and both place
  producers through `Scheduler::run_transferred_producer`.

### Store-view reads

`StoreViewManager::base_view_pollable` (`crates/verter_session/src/resolver_store.rs`)
is the store-view read: a warm hit answers at once; a reader that finds no
in-flight build claims one, builds and publishes within the same poll; a
reader that finds another thread's build in flight returns `Pending` with
its waker registered for the claim's release (`BuildRelease`, a future), and
holds no manager lock while it waits. `base_view` drives the same
computation with `block_on_when_pending(StoreViewBuild, ..)`, so a reader
announces the wait only when it would park. `HostStoreView::from_host_read_pollable`
and `VerterHost::capture_batch_fixed_view_pollable` are its pollable callers;
the component-metadata root captures its fixed view through them, and the
compile root's macro bundle reads its cold seed through them.

- **Lifetime and cancellation.** Dropping a pending read withdraws its
  registration and its test-only parked-reader count. A claim is never held
  across a suspension, so the self-await refusal keeps its thread identity.
- **Evidence:** `root_continuations_component_meta_root_suspends_behind_a_held_store_view_build`
  holds a real build at the test gate on another thread; the root's first
  poll answers `Pending` under a forbidding probe, the release wakes it, and
  it completes warm without entering a wait. Replacing the pollable wait
  with the synchronous park fails it with `EnteredWait(StoreViewBuild)`.

### Parse-stack refusal recording

`verter_parser::oxc_parse::refusals_within_pollable(operation)` records the
stack refusals an operation that may suspend makes on whichever thread
polls it, into the operation's own record, carries it across suspensions,
and answers it with the result; each poll also carries a refusal into the
recording that encloses that poll, as `refusals_within` does when it
returns. The compile root's entry records through it.

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
in the current tree, with the `WaitFamily` each announces. Each one parks
the polling thread inside whatever poll reaches it, so a root that reaches
one is not yet pollable at that point. Semantic memo flights, scoped
cache-node followers and resolver request followers share `TaskRegistry`
wait edges; the remaining families do not yet register their waits there.

| Wait | Family | Owner | Reached from | Pollable form today |
| --- | --- | --- | --- | --- |
| Semantic memo flight join (`SemanticGraphStore::acquire_query` → `Subscription::wait`) | `SemanticFlight` | `verter_type_engine/src/semantic_query_memo/producer.rs` | every synchronous `ProjectSemanticDispatch` query a step evaluates in place | `Subscription::poll`, used only by `SemanticTask` frames |
| Semantic task drive (`SemanticTask::drive`) | `SemanticDrive` | `verter_type_engine/src/project_semantic_dispatch/query_frames.rs` | `drive_query` for instantiations, `typeof` and callback-typing flow returns | `SemanticTask::poll`, test drivers only |
| Artifact cache singleflight joiner (`cooperative_admit_impl`) | `CacheFlight` | `verter_type_engine/src/cache_runtime/singleflight.rs` | flow-slice lookups; declaration-lookup, resolvability, owner-collection and shape-cache memo DBs | private `SlotJoin::poll`; synchronous callers still drive it to completion |
| Query-identity candidate singleflight joiner (`cooperative_admit_with_lookup_publish`) | `CacheFlight` | `verter_type_engine/src/cache_runtime/lookup_publish.rs` | imported-registry symbol resolution | same `SlotJoin::poll`; callers still synchronous |
| Resolver request singleflight follower (`SingleflightGroup::run_retaining`) | `ResolverFlight` | `verter_session/src/resolver_core/mod.rs` | `run_stable_request` ← the cold component-meta resolve; fallthrough requests; IndexedReady materialization; imported roots | `run_retaining_pollable`, led and followed by task; callers still synchronous |
| Store-view build park, synchronous reads | `StoreViewBuild` | `verter_session/src/resolver_store.rs` | `resolver_store_view_read` and the synchronous capture, from inside resolution | `base_view_pollable`; the roots' own captures already use it |
| Carrier publication lanes (`publish_or_get`, `await_stable_parse`) | `CarrierPublication` | `verter_session/src/carrier_publication_store/mod.rs` | the scheduler's source stage; session overlay structure | private `LaneWait::poll` and stable-parse poll; callers still synchronous |
| Host load (`CooperativeSchedulerAdapter::drive` → `Scheduler::wait_or_drive`), a completion handle's `wait` | `SchedulerCompletion` | `verter_session/src/cooperative_scheduler.rs`, `verter_scheduler/src/scheduler.rs`, `verter_scheduler/src/job.rs` | `VerterHost::ensure_loaded` from component-meta registry/scope producers, prepared declarations, eval programs/environments, the frontier engine and compile entries | `CompletionSubscription::poll` on the handle; the adapter still parks |
| Batch upsert completion (`Scheduler::wait_batch`) | `SchedulerCompletion` | `verter_scheduler/src/scheduler.rs` | `PreparedUpsertBatch::finish` ← `compile_many` | as above |
| Scoped cache-node producer drive (`execute_scoped_cache_node`; external polling plus a CPU-pool `install`) | `ScopedCacheNode`, then `CpuPool` | `verter_scheduler/src/scheduler/scoped_handoff.rs` | the synchronous macro bundle entry and the Vue API projector | the compile root awaits the handoff and hands out its producer |
| Scheduler inbox submission (`send_submission`) | `SchedulerAdmission` | `verter_scheduler/src/scheduler/submission.rs` | `submit_request`, `submit_batch_atomic`, scoped cache nodes | owned `SchedulerSubmission::poll` for requests/batches; existing root callers still use synchronous wrappers |
| Workspace import-resolution flight (`FlightSubscription::wait`) | `WorkspaceResolution` | `verter_workspace/src/resolution_flights.rs` | `resolve_import_outcome*` from host loading, the analysis stage and compile hydration | `FlightSubscription::poll` registers a waker; callers still synchronous |
| Declaration-lowering synchronous entries (`acquire_lease*`, `run_leased*`) | `DeclLowering` | `verter_semantic_source/src/decl_lowering.rs` | IndexedReady materialization and the declaration body memo, from inside resolution and semantic evaluation | the `begin_*` handoffs above; callers still synchronous |
| Host CPU pool batch (`HostBatchCoordinator::run_batch` → `HostCpuPool::install`) and the scheduler pool's `install` | `CpuPool` | `verter_session/src/host_batch_coordinator.rs`, `verter_scheduler/src/execution/` | `compile_many`, `get_component_meta_batch`, and the synchronous drivers' handed-out producers | none; the batch topology itself is the executor's to replace |

The declaration-lowering row's handoffs are pollable; what still blocks is
their synchronous entries, which IndexedReady materialization and the
declaration body memo call from inside resolution and semantic evaluation.
What remains is to make the computations that call them suspend instead.

## What a pollable root still needs

The cold component-metadata root resolves macros through
`ProjectSemanticDispatch`, extracts the analysis (which recurses into
imported components' fallthrough), and publishes through
`ComponentMetaResultPublish`, all synchronously inside the poll that follows
its capture; the compile root's handed-out macro producer runs the same
kind of traversal. Every wait in the table above can be reached from inside
that nested synchronous call graph — the measured fixture alone reaches
fourteen, up to 138 frames deep. A root becomes fully pollable only when
each such call either answers without waiting or suspends as owned state,
and resumption continues the suspended computation rather than re-running
it. Converting the low-level waits alone does not achieve this; the callers
that reach them have to become resumable too.

The conversion that remains runs from each wait upward to the root, one
caller at a time, as compiler-generated continuation state (`async` bodies
polled by the root), so a suspended call keeps its locals, its traversal
position and its completed results, and resuming it repeats none of them.
What is already in place for that:

- the roots themselves, their ambient installation, their task identity
  and the `RootWork` handoff;
- a `Sync` dispatcher, so continuation state that borrows it is `Send`;
- task-owned resolver flights whose leader may suspend inside its compute,
  and workspace leases that cannot cross a suspension;
- pollable forms of every leaf wait in the table.

Each scope a converted caller holds across a suspension point — a fact
tracer, a spend or span log, a refusal recording, a request budget, a
construction lane — needs the per-poll carry `SemanticTask` and
`refusals_within_pollable` use: the scope's state is owned by the suspended
computation and installed for each poll on the polling thread. Where a
scope's guard is thread-bound — the spend log, a workspace lease, a borrow of
the dispatcher's state — the compiler rejects holding it across a
suspension; a scope whose guard is `Send` has to be given its carry
explicitly before a caller that holds it is converted.
