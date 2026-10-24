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
project and revision fence is rechecked before any integration writes, under
the existing host admission fence shared with source replacement and eviction.
The fence permits same-thread nested contributor loading during ingress while
excluding concurrent writers. Ambient ingestion takes admission before its
snapshot cursor, matching upsert's lock order even when ingestion loads a file.
Workspace edges and aliases land before the dependency row marks integration
complete. A refused cold commit leaves no integrated marker; a superseded commit
cannot overwrite replacement dependencies. The caller assignments above this
handoff are DISPATCH for shared resolver loading, META for metadata traversal
and COMPILE for compilation.

The synchronous `ensure_loaded` entry drives this same handoff. Its external
driver rechecks cancellation after yielding, parks external native callers on
the driver, and lets scheduler workers use the existing cooperative pump and
bounded park cadence. Inline drivers retain typed `NoProgress` when no work
can advance. These driving operations are absent from the handoff capability.

### Carrier publication and stable parse

`CarrierPublicationStore::begin_publication` takes owned accepted source and
request context plus a waiter task, and returns `OwnedPublication`. Its poll
transfers `PublicationStep::Produce(PublicationProducer)` or returns a terminal
`Outcome`. The independent producer polls a retained continuation: it transfers
`PublicationProducerStep::Work(PublicationWork)` for the initial retained-unit
read/adoption, the nested stable producer, and envelope finishing, then returns
`Complete`. Only external `Work::run` performs backend reads, validation of
retained payloads, parsing, rehoming or retention. No poll drives another
computation synchronously. `publish_or_get` externally drives these same states.

`begin_stable_parse` returns `OwnedStableParse`. Its `Produce` step transfers
one `StableParseProducer`; `run` performs the double-checked retained read or
one parser attempt. `Parsed(StableParseValue)` owns the complete immutable
product and a retirement interest. A publication producer waits on this child
under its own P task, giving W → P → S in the host's single task registry.
Every store constructor takes its composition root's `TaskRegistry`. Host
construction supplies the same registry as scheduling and semantic evaluation;
standalone callers supply their execution authority explicitly. P/S always
belong to that registry, including synchronous production and reentrant backend
calls. Foreign pending waiters receive `Cycle`; an immediately ready answer
needs no edge. Registry binding cannot be replaced on an existing cloned store.
The synchronous wrapper inherits an ambient task only from that store's
registry; an unrelated host scope does not prevent independent publication.

Request cancellation stops only its subscriber, including the initiating
subscriber after producer transfer. P and S install independent task and stop
scopes during external work, restore them before notifications, and may move
between threads. Another subscriber can claim an unclaimed producer after its
initiator detaches. Terminal/drop cleanup releases its own edge, stop and wake
registrations and task pins. `ProducerDropped` is explicit abandonment;
after existing subscribers drain, the exact failed lane retires so unchanged
source can retry. It cannot remove a successor lane or invalidate another
subscriber's terminal.
`WinnerPanicked` remains distinct, and stable rejection carries `SyntaxReject`.
Source or grammar invalidation fences every snapshot-bound envelope, including
adopted and shared products; a verified stable unit remains reusable despite a
losing generation. Retention denial still returns the complete envelope.

Successful publication delivery stays strong while attached consumers remain,
then returns to the existing weak terminal retention. Stable lanes stay listed
through each registered consumer's envelope finishing. The retirement interest
moves into the actual Finish Work and survives dropping the P continuation;
retirement rechecks consumer/producer state under table → lane locks and removes
only the identical lane instance. Backend callbacks and wakes run after owner
locks release. Adoption provenance, parser counters and snapshot rehoming use
the existing helpers.

Callers above these boundaries belong to DISPATCH for the scheduler Source
stage and shared base/overlay indexing, META for metadata traversal, and COMPILE
for compile hydration. Their conversion is separate from these dependency APIs.

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
  its owner; a surviving sibling keeps shared work live. Attaching owners
  collected before admission holds a temporary aggregate registration until
  every request is attached, so an already-cancelled request cannot retire
  the flight ahead of an existing live sibling.
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
  authority; a transferred producer installs its actual distinct task and
  installs a live aggregate cancellation owner while running. A cancelled waiter
  whose thread executes the producer cannot stop shared work through either
  its job or request token. Other request accounting stays installed.
  Compatibility drivers retain ambient cancellation. The waiting parent keeps its edge until
  terminalization or drop.
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
  before dropping joined values, retained callbacks, keys, group handles and
  task pins. Producer transfer has no queue or pool admission. Dropping an unrun producer wakes
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

### Base store-view capture

`VerterHost::begin_store_view(waiter, cancellation)` returns the opaque,
owned `StoreViewHandoff`. `poll` returns `StoreViewStep::Produce` with an
independent `Send` producer or `Value` with the capture's intrinsic currentness.
It never builds a snapshot or sweeps retired artifacts. A cold producer's
external `run` builds and publishes through the existing manager cache and
its exact token/reset fence. A completed producer reply revalidates the full
live token and cached snapshot identity under the manager lock when its owner
resumes; publication alone cannot mint currentness after suspension. A warm
hit returns immediately unless released roots left reclamation pending; then the same claim transfers maintenance
with the exact cached read, preserving a return-only read's reason.

The manager uses the host's shared registry and one physical claim. Each
subscription retains its own wait edge and stop registration. Typed errors
are `Cancelled`, `Cycle`, `ProducerUnowned`, `ProducerDropped`,
`ProducerPanicked` and `Reset`; none carries a partial view. Dropping or
cancelling a subscriber leaves its transferred producer and siblings live.
An abandoned producer releases only its exact claim, and owned execution
contains panic after settling subscribers. Readiness and reset take wakers
under the manager lock and invoke them after unlocking.

Reset revokes unstarted work immediately. Running work keeps the physical
claim until it exits, while pre-reset subscribers become `Reset` at once.
New subscribers wait for that physical release, then recapture under the new
generation; they do not inherit the old reset result. Successful followers
recapture and validate the cache under the original bounded main/final/fallback
rounds. The synchronous `StoreViewManager::base_view` drives this same core,
retaining its execution-scoped reentry refusal and external reclamation.
Completed handoffs release their host, task and cancellation interests; the
opaque result retains only its captured view and exposes no blocking driver.

Caller conversion above this boundary remains with DISPATCH for shared
`capture_batch_fixed_view`/resolver contexts, META for component metadata
traversal, and COMPILE for compile traversal. Overlay application and batch
membership ingestion remain caller operations, outside this base capture.

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
first poll. This returns `OwnedLoweringHandoff`, whose poll yields
`LoweringStep::Work` or `Value`, with typed `LoweringFailure` (`WorkerStopped`,
`ProducerDropped`, `Panicked` or `Reentrant`) inside its value. The existing worker job
installs its producer task from the waiter's registry; no parser arena or
borrowed AST crosses a shard boundary. A batch's pure shard operations
jointly produce one ordered answer under its batch task identity. Cancellation
drops unsubmitted jobs or abandons admitted replies; an arriving lease is
released on its owner shard. Existing synchronous entries retain their panic
compatibility and drive the same queue/reply state.

On native workers the owned step normally carries `Value`. WASM transfers a
`Send` `LoweringWork` token and polls a retained reply; no parser work or pure
callback runs inside poll. External `run` contains operation panic, and dropping
the token settles `ProducerDropped`. The shared dependency remains pending
across this transfer, retaining its wait edge, task and stop interest until the
value or stop is observed. Cancelling the subscriber after transfer leaves that
work independent. Raw handoffs expose `poll_step` for the same transfer, and
synchronous entries run it outside the reply poll. An empty batch is ready
without work or an arena borrow. The generic inline reply/work protocol has
native behavioral controls as well as target compilation; its fields carry
owned operations and answers, never an AST or parser arena.

The executing worker opens its existing shard identity on that actual task.
An owned demand targeting a shard its waiter already owns returns `Reentrant`
before queue admission. Batches preflight every target before admitting any
share. WASM checks its single arena identity before borrowing the thread-local
shard. This is a defensive refusal; callbacks remain pure and may not reenter
the service or host.

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
  released on the shard that pinned it, leaving no abandoned lease or parse
  retained after shard cleanup. While a handoff is outstanding it holds the
  service, and so its workers, alive. After releasing unclaimed answers and
  unsubmitted closures, a final handoff atomically extracts the service and
  closes its queues without joining workers. Already-admitted jobs finish
  independently and their workers release shard state on exit. Even cancelling
  the final unsubmitted subscriber cannot enter worker drainage inside poll.
  A final external service owner retains joining shutdown; a worker dropping
  the final service handle joins only other workers. Polling after the answer
  is a contract violation.
- **Batches:** each poll advances every share still outstanding, so every
  shard's share is offered on the first poll and the shards lower in
  parallel. The batch answers once every share has answered, and the first
  job panic re-panics then.
- **wasm32:** the shard remains thread-local. Construction retains owned
  inputs; an owned poll or raw `poll_step` transfers `LoweringWork` before
  execution. External `run` installs its actual task and opens the arena
  identity around the pure operation. Later polls observe only its reply.
  Stop before transfer executes nothing; stop after transfer abandons only
  the subscriber. Panic containment requires an unwinding target; the default
  wasm panic-abort target is checked for compilation rather than recovery.

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

### Higher-caller conversion inventory

These assignments cover traversal above the dependency owner. Conversion must
retain the exact view, source and project validation and resume saved work.
A synchronous compatibility call is not a suspension boundary.

| Direct caller or caller group | Conversion owner |
| --- | --- |
| `host_executor.rs::source_snapshot`, `host_manage/overlay_materialize.rs::registered_overlay_structure` → carrier publication | DISPATCH |
| `host_lifecycle.rs::resolve_import_outcome_in`, `host_executor.rs::extract_deps`, `program_order.rs` → workspace resolution | DISPATCH; COMPILE owns compile traversal above program ordering |
| `host_manage/{prepared_decl,eval_program,eval_env,overlay_priority}.rs`, `host_resolve/frontier_engine.rs`, `resolver_core/request_bound.rs` → host loading | DISPATCH |
| `resolver_core/component_meta_query_engine/{mod,registry_cache_producers,registry_decl}.rs`, `component_meta_host.rs`, `meta.rs` → host loading | META |
| `host_compile_audit.rs`, `host_resolve/virtual_file_pipeline.rs` → host loading | COMPILE |
| `host_construction.rs` → initial program-member host loading | DISPATCH |
| `host_manage/{component_meta_entry,component_meta_entry_resolution}.rs`, `meta.rs`, `meta/output_api.rs` → fixed store-view capture | META |
| `host_manage/analysis_io.rs` → fixed store-view capture | DISPATCH |
| `host_resolve/virtual_file_pipeline.rs` → fixed store-view capture | COMPILE |
| `host_manage/overlay_materialize.rs` → lease acquisition and leased lowering; `host_manage/prepared_decl.rs` → lease acquisition with lowering | DISPATCH |
| `typeinfo/vue_macro_codegen.rs` → scoped cache-node execution | COMPILE |
| `host_upsert.rs::UpsertBatchTxn::finish` and atomic batch submission reached by `compile_many` | COMPILE |

Resolver request-flight callers are enumerated beside their owned interface
above. Artifact and query-candidate cache consumers, semantic memo joins and
semantic task driving belong to DISPATCH. META and COMPILE retain their
traversal above those shared semantic operations. Host CPU batch coordination
is the executor boundary; it is not another dependency service.

### Workspace resolution

`WorkspaceRead::begin_resolution(Arc<Self>, ResolutionRequest, waiter, stop)`
returns `WorkspaceResolution`. Importer, specifier, context, optional published
root, immutable overlay and explicit project are owned inputs. The separate
`begin_project_resolution` accepts a project-only request without inventing an
importer. Engine-backed memory and filesystem workspaces implement both;
adapters without that ownership protocol return `ResolutionError::Unsupported`.

The subscriber polls `ResolutionStep::{Produce,Value}`. The independent
`ResolutionProducer` only transfers `ResolutionWork` or registers readiness;
only external `ResolutionWork::run` advances the retained continuation. Work
uses the caller's existing execution service, creates no pool or CPU authority,
and installs its producer task for that slice. Parser state remains in the
existing resolver owner; only owned resolution records cross this interface.

Phases include stable-world capture, candidate validation, bounded input
preflight and loading, filesystem discovery, freezing, replay and publication.
Flight and publication-gate contention register wakes and shared-registry wait
edges instead of parking the owner execution. Input loading transfers external
work before performing IO. An incomplete input load, gate or flight can be held
while other already-admitted work continues; resuming never admits a new root
or discards the captured attempt. Discovery and replay share the same
`InputResolutionLedger`, including attempts, bytes, churn, staged inputs and
completed-output ownership. Directory evidence is owned by that continuation
and installed only for external slices, so migration cannot lose it in worker
TLS. Frozen-reader completeness and independent live validation still govern
filesystem admission.

The host injects its TaskRegistry through `WorkspaceServices::attach`, including
workspace replacement. If a workspace is shared by hosts, attach joins their
existing cycle graphs through `TaskRegistry::share_authority_with`; it never
revokes earlier tasks or creates another task service. Generation-qualified task
ids retain their registration namespace, live wait edges survive composition,
and installed scopes follow the joined authority. Union by rank and ordered
root locks make concurrent joins safe. The last registration-owner release
removes its pool, so repeated workspace swaps retain no pool history.
Standalone workspace compatibility calls use their own composition authority. Flights own producer leases across threads; every joining
task has its own subscription and wait edge. Same-path and mixed scheduler/flight
cycles return `Cycle`, including a nested request on the same OS thread. A shared
candidate is adopted only after validation in the subscriber's captured world;
retention refusal still delivers the complete unretained answer. The final fence
also rejects a replaced pinned published root.

Cancellation wakes and detaches only the subscriber. Transferred producer work
continues for other demands. Its external owner must finish it or drop it; dropping
a producer or transferred slice publishes `ProducerDropped`, and an unwind
publishes `ProducerPanicked`. An abandoned inner candidate flight wakes followers
for their own bounded retry. Explicit request cancellation (including a host
reset/shutdown stop token) returns `Cancelled`; a changed pinned world returns
the existing noncacheable `ResolutionViewSuperseded` outcome. No stop admits a
partial answer. Terminal observation/drop releases exactly that subscriber's
registrations, wait edges and task pins. Flight retirement removes only its exact
lane; all notifications run after the table/slot/publication locks release.

The synchronous importer and project entries externally drive these same
continuations. Legacy boolean cancellation is sampled only at that external
compatibility driver; token-owned production entries register stop wakes. Polling
capabilities expose neither that driver nor the underlying engine/reader.
Completion/flight replies exert no bounded-queue backpressure; input reservations
retain their existing explicit budget and integrity outcomes. Observation remains
the existing optional VFS/audit capture, with no new history or enabled checks.

Above this handoff, host loading and index dependency extraction
(`host_lifecycle::resolve_import_outcome_in`, `host_executor::extract_deps`),
resolver import/route demands, explicit-project intrinsics and program ordering
belong to DISPATCH; metadata import traversal belongs to META; hydration and
compile traversal above program ordering belong to COMPILE. Those caller
conversions consume these APIs separately from the dependency owner.

## Boundaries that still block

These are the production waits a component-meta or compile root can reach
in the current tree. Each one parks the polling thread inside whatever poll
reaches it, so a root that reaches one is not yet pollable. Semantic memo
flights and scoped cache-node handoffs register shared-registry wait edges.
Completion, cache admission and declaration-lowering subscriptions offer
explicit task-owned adapters; their synchronous callers still require
conversion. Workspace resolution now offers the owned dependency entry above;
its higher-level traversals still use the external synchronous compatibility driver.

| Wait | Owner | Reached from | Pollable form today |
| --- | --- | --- | --- |
| Semantic memo flight join (`SemanticGraphStore::acquire_query` → `Subscription::wait`) | `verter_type_engine/src/semantic_query_memo/producer.rs` | every synchronous `ProjectSemanticDispatch` query a step evaluates in place | `Subscription::poll`, used only by `SemanticTask` frames |
| Semantic task drive (`SemanticTask::drive`) | `verter_type_engine/src/project_semantic_dispatch/query_frames.rs` | `drive_query` for instantiations, `typeof` and callback-typing flow returns | `SemanticTask::poll`, test drivers only |
| Artifact cache singleflight joiner (`cooperative_admit_impl`) | `verter_type_engine/src/cache_runtime/singleflight.rs` | flow-slice lookups; declaration-lookup, resolvability, owner-collection and shape-cache memo DBs | owned artifact admission with producer transfer; dispatch callers still synchronous |
| Query-identity candidate singleflight joiner (`cooperative_admit_with_lookup_publish`) | `verter_type_engine/src/cache_runtime/lookup_publish.rs` | imported-registry symbol resolution | owned query admission with producer transfer; dispatch callers still synchronous |
| Resolver request singleflight follower (`SingleflightGroup::run_retaining`) | `verter_session/src/resolver_core/mod.rs` | `run_stable_request` ← the cold component-meta resolve; fallthrough requests; module exports, overlays, prepared declarations, source requests | `OwnedResolverFlight::poll` with producer transfer; callers still synchronous |
| Store-view capture (`StoreViewManager::base_view`) | `verter_session/src/store_view_handoff.rs` | `capture_batch_fixed_view` at every component-meta entry | `VerterHost::begin_store_view` with owned build/maintenance producer; callers still synchronous |
| Carrier publication and stable parse | `verter_session/src/carrier_publication_store/owned.rs` | scheduler Source stage; base/overlay structure | `OwnedPublication` → external Work and owned S subscription; `OwnedStableParse` → external stable producer; callers still synchronous |
| Host load (`CooperativeSchedulerAdapter::drive` → `Scheduler::wait_or_drive`) | `verter_session/src/cooperative_scheduler.rs`, `verter_scheduler/src/scheduler.rs` | `VerterHost::ensure_loaded` from component-meta registry/scope producers, prepared declarations, eval programs/environments, the frontier engine and compile entries | `VerterHost::begin_load` → owned scheduler request → `HostLoadCommit`; synchronous callers externally drive and integrate |
| Batch upsert completion (`Scheduler::wait_batch`) | `verter_scheduler/src/scheduler.rs` | `PreparedUpsertBatch::finish` ← `compile_many` | as above |
| Scoped cache-node producer drive (`execute_scoped_cache_node`; external polling plus a rayon `install`) | `verter_scheduler/src/scheduler/scoped_handoff.rs` | Vue macro codegen in compile and the Vue API projector | `ScopedCacheNodeHandoff::poll` transfers a producer to the external driver; semantic callers still use the synchronous wrapper |
| Scheduler inbox submission (`send_submission`) | `verter_scheduler/src/scheduler/submission.rs` | `submit_request`, `submit_batch_atomic`, scoped cache nodes | owned `SchedulerSubmission::poll` for requests/batches; existing root callers still use synchronous wrappers |
| Workspace resolution (synchronous compatibility entries) | `verter_workspace/src/resolution_flights.rs` | DISPATCH: host loading, analysis-stage dependencies and shared resolver import/route demands; META: metadata's import traversal; COMPILE: hydration and compile traversal | `WorkspaceRead::begin_resolution` / `begin_project_resolution` transfer retained phase work and register owned flight/publication readiness; traversal callers still synchronous |
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
