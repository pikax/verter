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
With a `slice`, each park is bounded, so a computation without its own
stop registration is re-polled at least that often. Semantic tasks register
their cancellation wakes directly and do not require timed re-polling.

The driver is reached only from synchronous public entries outside the
computation. No poll-facing value hands a computation a way to call it.
Each synchronous entry listed below is the driver applied to the pollable
boundary beside it, so there is one implementation of each handoff, not a
blocking copy next to a polling one. The semantic runtime's blocking drive
(`SemanticExecution::drive`, `SemanticTask::drive`) uses the same driver,
with a 2 ms slice so a stop stays observable.

## Delivered boundaries

### Task-owned cancellation

`verter_execution::dependency::OwnedDependency<H>` binds a poll-only
`DependencyPoll` to its actual producer and waiter tasks. Both must belong
to the same registry. Pending registers one wait edge; a cycle, an already
waiting task or an authority mismatch returns `DependencyStop::Cycle`.
An immediately ready answer needs no edge. `DependencyStop::Cancelled`
contains no partial answer. Terminal answers and drop withdraw the readiness
and cancellation registrations, edge and task pins. No method exposes the
underlying handoff or its synchronous driver.

`CancellationToken::subscribe` observes explicit stop without timer-based
re-polling. Registration and stop publication share the token's wake lock.
Aggregate tokens forward the final owner's cancellation or withdrawal;
surviving owners keep the shared computation live. Liveness probes remain
callback-free. Owners that publish cancellation under an outer lock use
`cancel_deferred`, `register_owner_deferred` or
`CancellationOwner::detach_deferred` and deliver the
returned wakes after unlocking. The scheduler uses its existing DAG/lifecycle
delivery rail for these wakes as well as completion wakes.

The host constructs one registry and injects it into its scheduler and
semantic store. A scoped cache flight owns a producer task in that registry;
its producer installs that task while executing, including when moved to
another thread. A scoped subscriber captures the current task or owns a new
task for an external caller. It retains its wait edge while the transferred
producer runs under its own task, so a demand back into the waiting parent
is refused. Terminalization withdraws that edge. Cancellation
wakes the subscriber directly, and `Reentrant` also represents refused
cross-family cycles.

Terminal scoped handoffs also release their scheduler, identity and request
context; keeping a completed handoff alive does not retain its scheduler tree.

The semantic task retains cancellation subscriptions beside its suspended
execution. A request stop or the final shared-job owner's cancellation or
withdrawal wakes its latest poller, including after a move to another thread.
A job that has accepted request owners decides cancellation for shared work;
otherwise the bound request decides, with an ownerless job as fallback. The
job subscription remains armed while it is ownerless, so accepting owners
during a suspension cannot lose a later stop. An unselected ownerless job
may cause a harmless extra wake. Every terminal poll or task drop withdraws
the task's stop registrations. This does not make the nested synchronous
semantic callers in the inventory below pollable.

Semantic memo subscriptions use one polling implementation for both task
polls and synchronous joins. `Subscription::wait` applies the external
driver to `Subscription::poll`; flight cells hold only readiness wakers,
with no separate condition-variable wait implementation. The synchronous
join retains its bounded park for callers without task-owned stop wakes.

Conditional frames retain completed operand preparation in
`ConditionalDecisionState`. The synchronous and frame entries use the same
preparation and selection stages. Each stage owns its speculative observation
scope: an unanswered read discards only that unfinished stage, while completed
preparation has already joined the frame's retained facts and taint. A later
relation dependency therefore resumes selection without preparing the operands
again. Incomplete preparation and selection still retry their own stage; this
does not make the remaining nested synchronous calls pollable.

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
- **Backpressure:** none at completion; admission has its own handoff below.

`CompletionSubscription::owned(waiter, producer, cancellation)` returns an
`OwnedDependency` with the same terminal states. The supplied producer must
be the execution that actually produces the completion. It stops only that
subscriber, preserving the handle's first-writer-wins result and siblings.

`CompletionSubscription::owned_producer(waiter, cancellation)` follows the
scheduler's actual producer instead of accepting one from the caller. Gated
stages follow their first admitted prerequisite recursively, in dependency-key
order. Analysis dependencies not yet admitted follow their Source producer.
Each prerequisite release, new stage admission or scoped producer replacement
updates affected ancestor completions through the reverse dependency indexes
and wakes their subscriptions after the DAG lock releases. Re-polling moves
the root's single wait edge to that producer; completion and cancellation
release its readiness registration, stop registration and task pins.

`Scheduler::begin_owned_request` combines pollable inbox admission with this
completion ownership. Cancellation before admission drops the unsubmitted
request; cancellation after admission detaches only the waiter. Shared stages
and sibling requests continue.

### Host load and external integration

`VerterHost::begin_load` returns `HostLoadHandoff`. Its poll advances owned
scheduler admission/completion and transfers `HostLoadCommit` once Analysis is
ready. It performs no host integration, stage execution or blocking submission.
`HostLoadCommit::apply` performs integration externally and refuses another
host, cancellation, changed source incarnation/generation or a changed canonical
eviction revision or project authority. This fence also applies to warm commits
and prevents an old commit reversing a later eviction. Unrelated-file evictions
and additive load-generation changes remain compatible. The same stop, source,
project and revision fence is rechecked under the existing derived-state row
lock immediately before clearing its eviction marker; no callback runs in that
critical section. The caller assignments above this handoff are DISPATCH for
shared resolver loading, META for metadata traversal and COMPILE for compilation.

The synchronous `ensure_loaded` entry drives this same handoff. Its external
driver rechecks cancellation after yielding, parks external native callers on
the driver, and lets scheduler workers use the existing cooperative pump and
bounded park cadence. Inline drivers retain typed `NoProgress` when no work
can advance. These driving operations are absent from the handoff capability.

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
- **Backpressure and lifetime:** admission retains the unsubmitted item and
  separate capacity and flight registrations. A sibling's settlement
  can bypass that item's capacity wait; a retired flight resolves
  even if the successor inbox is full. The handoff and producer retain the
  scheduler independently and can move between threads with exclusive
  polling. Dropping a handoff withdraws its registrations and detaches only
  its owner; a surviving sibling keeps shared work live.
- **Errors:** cancellation, shutdown/reset, panic, mismatched result type and
  same-path re-entry retain the existing typed outcomes. Cancellation is
  checked and registered at each poll. A stop wakes only its subscriber;
  shared-registry wait edges refuse cycles before suspension. Polling after
  a terminal result is a contract violation.
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

### Resolver request flights

`SingleflightGroup::begin_owned_retaining` returns `OwnedResolverFlight`.
Its only advancing operation, `poll`, returns a complete
`SingleflightRunResult` or transfers the sole `OwnedResolverProducer`.
Polling does not compute or run the retention policy. `run_retaining` and
`run` externally drive the same core with borrowed callbacks.

- **Ownership:** host composition binds every resolver flight family to the
  host's existing registry. Production compatibility entries install that
  authority; a transferred producer installs its actual distinct task while
  running. The waiting parent keeps its edge until terminalization or drop.
- **Cancellation and errors:** cancellation stops one subscriber without
  stopping its producer or siblings. `Cycle`, `ProducerUnowned`,
  `ProducerDropped`, `ProducerPanicked`, `Reset` and the original producer
  error are typed terminal refusals. The owned producer contains panic;
  compatibility producers preserve unwinding and their followers re-elect. Same-path
  owned recursion refuses a cycle; synchronous recursion retains its nested
  computation escape outside poll.
- **Publication:** `participate_owned` pins before a warm-cache probe and
  can survive suspension without borrowing the group. Participation pins retain a stable rendezvous for its
  concurrent burst. Non-retained results publish to existing followers and
  leave the table in one locked operation. Pointer-checked retirement cannot
  remove a successor flight. Reset wakes a held producer's subscribers and
  prevents its later execution or publication.
- **Lifetime and backpressure:** each subscriber owns one readiness and stop
  registration and exactly its own task edge. Terminalization withdraws them
  and drops retained callbacks, keys, group handles and task pins. Producer
  transfer has no queue or pool admission. Dropping an unrun producer wakes
  followers with a typed failure; dropping a follower leaves siblings live.

The metadata request and fallthrough flight callers belong to metadata
traversal conversion. Module exports, overlay materialization, prepared
and indexed declarations, imported sources, route names and barrel surfaces
belong to dispatch conversion; compilation reaches them through dispatch.
Their existing synchronous calls still need resumable traversal state.

| Higher caller | Conversion owner |
| --- | --- |
| `resolver_core/component_meta_request.rs::run_component_meta_request` → `run_stable_request` | META |
| `resolver_core/fallthrough_request.rs` → `run_stable_request` | META |
| `host_manage/module_exports.rs` → `module_export_singleflight` | DISPATCH |
| `host_manage/overlay_materialize.rs` → `indexed_singleflight` | DISPATCH |
| `host_manage/prepared_decl.rs` → prepared-bundle and indexed flights | DISPATCH |
| `host_manage/source_request.rs` → imported-root and route-name flights | DISPATCH |
| `resolver_core/route_db.rs` → barrel-surface flight | DISPATCH |
| Compile traversal and hydration above these shared dispatch calls | COMPILE |

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

Each handoff additionally offers `owned(waiter, cancellation)` before its
first poll. This returns `OwnedDependency` with typed `LoweringFailure`
(`WorkerStopped` or `Panicked`) inside its answer. The existing worker job
installs its producer task from the waiter's registry; no parser arena or
borrowed AST crosses a shard boundary. A batch's pure shard operations
jointly produce one ordered answer under its batch task identity. Cancellation
drops unsubmitted jobs or abandons admitted replies; an arriving lease is
released on its owner shard. Existing synchronous entries retain their panic
compatibility and drive the same queue/reply state.

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
- **wasm32:** the shard is the calling thread's own. Construction retains
  owned inputs; its first poll runs the pure shard operation and is `Ready`.
  Attaching ownership before that poll installs the task around the work and
  allows an already cancelled demand to stop without executing it. Panic
  containment requires an unwinding target; the default wasm panic-abort
  target is checked for compilation rather than panic recovery.

### Cache admission and publication

Both artifact and query-candidate admission use one owned protocol. Artifact
callers enter `singleflight::owned_admit_with_post_publish` or
`owned_admit_with_post_publish_by_flight_key`; query-candidate callers enter
`lookup_publish::owned_admit_with_lookup_publish` in the type engine.

- **Work transfer:** `OwnedAdmission::poll` answers a validated warm value,
  subscribes to a flight, or transfers the sole `OwnedAdmissionProducer`.
  Running that producer is the external owner's job. Polling neither computes
  cold work nor submits to a queue. The producer installs its actual task from
  the waiter's shared registry, and the parent edge remains live until the
  answer is observed or abandoned.
- **Result and failure:** `Value(Some(value))` is complete. `Value(None)` keeps
  the adapter's failure policy. `Cancelled`, `Cycle`, `ProducerUnowned`,
  `ProducerDropped` and `ProducerPanicked` are typed refusals without partial values. The artifact
  follower surfaces failed production; a query follower forks after producer
  panic or rejected admission. Return-only production is never shared across
  views. A cacheable follower revalidates and bubbles facts in its own view.
- **Lifetime:** each subscriber owns one replaceable readiness registration
  and its stop interest. Dropping or cancelling it leaves the producer and
  sibling subscriptions intact. Terminalization releases task pins, its table,
  key and callbacks while a completed handle may remain alive. Dropping an
  unrun producer fails and retires its exact flight and wakes the followers.
- **Publication:** the original validation, retention fences, pointer-checked
  retirement, displaced-entry cleanup and deferred eviction stay in the
  owning adapter. Wakes occur after publication locks release. The same-key
  and split-flight artifact entries preserve their different publish locking.
- **Compatibility:** synchronous admission/get-or-insert entries externally
  drive this same state machine with borrowed callbacks. Explicit owned APIs
  require owned transferable views/tracers; they refuse a pending producer
  whose shared task authority is unavailable with `ProducerUnowned`. A ready
  answer needs no wait authority. No extra registry or pool is
  allocated by standalone compatibility calls.

The dispatch layer owns conversion of `cache_runtime::node::{lookup,
query::lookup}` and their semantic memo consumers. Metadata and compilation
callers reach those nodes through dispatch and retain responsibility for their
own resumable traversal state.

## Boundaries that still block

Conditional frames retain operand preparation, absorption/distribution
classification and the completed prefix of a distributed check. An explicit
distributed member or inferred winner transfers its nested conditional to another frame;
it no longer evaluates that member synchronously inside the parent. Each
completed member's observations join the parent before its result is
retained. An unanswered member neither advances the cursor nor contributes
a partial value to the accumulated union. Resumption continues at that
member, and the final union uses the same reduction as an uninterrupted
decision. The relation and winner stages, and nested queries evaluated
inside other synchronous adapters, still require continuation conversion.

These are the production waits a component-meta or compile root can reach
in the current tree. Each one parks the polling thread inside whatever poll
reaches it, so a root that reaches one is not yet pollable. Semantic memo
flights and scoped cache-node handoffs register shared-registry wait edges.
Completion, cache admission and declaration-lowering subscriptions offer
explicit task-owned adapters; their synchronous callers still require
conversion. Resolver/store-view, workspace, carrier publication and host
load/index ownership remain incomplete in this inventory.

| Wait | Owner | Reached from | Pollable form today |
| --- | --- | --- | --- |
| Semantic memo flight join (`SemanticGraphStore::acquire_query` → `Subscription::wait`) | `verter_type_engine/src/semantic_query_memo/producer.rs` | every synchronous `ProjectSemanticDispatch` query a step evaluates in place | `Subscription::poll`, used only by `SemanticTask` frames |
| Semantic task drive (`SemanticTask::drive`) | `verter_type_engine/src/project_semantic_dispatch/query_frames.rs` | `drive_query` for instantiations, `typeof` and callback-typing flow returns | `SemanticTask::poll`, test drivers only |
| Artifact cache singleflight joiner (`cooperative_admit_impl`) | `verter_type_engine/src/cache_runtime/singleflight.rs` | flow-slice lookups; declaration-lookup, resolvability, owner-collection and shape-cache memo DBs | owned artifact admission with producer transfer; dispatch callers still synchronous |
| Query-identity candidate singleflight joiner (`cooperative_admit_with_lookup_publish`) | `verter_type_engine/src/cache_runtime/lookup_publish.rs` | imported-registry symbol resolution | owned query admission with producer transfer; dispatch callers still synchronous |
| Resolver request singleflight follower (`SingleflightGroup::run_retaining`) | `verter_session/src/resolver_core/mod.rs` | `run_stable_request` ← the cold component-meta resolve; fallthrough requests; module exports, overlays, prepared declarations, source requests | `OwnedResolverFlight::poll` with producer transfer; callers still synchronous |
| Store-view build park (`StoreViewManager::park_for_build`) | `verter_session/src/resolver_store.rs` | `capture_batch_fixed_view` at every component-meta entry | private `BuildRelease::poll`; callers still synchronous |
| Carrier publication lanes (`publish_or_get`, `await_stable_parse`) | `verter_session/src/carrier_publication_store/mod.rs` | the scheduler's source stage; session overlay structure | private `LaneWait::poll` and stable-parse poll; callers still synchronous |
| Host load (`CooperativeSchedulerAdapter::drive` → `Scheduler::wait_or_drive`) | `verter_session/src/cooperative_scheduler.rs`, `verter_scheduler/src/scheduler.rs` | `VerterHost::ensure_loaded` from component-meta registry/scope producers, prepared declarations, eval programs/environments, the frontier engine and compile entries | `VerterHost::begin_load` → owned scheduler request → `HostLoadCommit`; synchronous callers externally drive and integrate |
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
