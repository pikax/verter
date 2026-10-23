# Pollable dependency handoff inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The interface itself is described in
[`docs/arch/root-continuations.md`](../../../docs/arch/root-continuations.md).

## verter_execution

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `PARK_WAKER` (thread-local unparking waker) and `UnparkThread` | `blocking::block_on` polls with it and parks between polls; `verter_type_engine`'s blocking drive and every declaration-lowering synchronous entry go through it. This supersedes the copy the continuation runtime's inventory lists in `verter_type_engine/src/semantic_execution.rs`, which no longer exists. | REQUIRED | The thread | `src/blocking.rs` | always |
| `WakerSet`, `WakeRegistration`, `TakenWakers` | Completion, queue, flight and publication owners register under their state lock, withdraw one subscriber, or take registrations for wake after unlock | REQUIRED | One producer and one entry per pending subscriber | `src/wake.rs` | always |
| `CancellationState.wakers`, `CancellationSubscription` (token and registration) | `cancel` and an aggregate's last-owner loss take and wake every registration outside the lock; semantic tasks, scoped cache-node handoffs, the synchronous semantic flight wait, carrier lanes and workspace flight waits poll it while pending | REQUIRED | One token; each entry until delivered or its subscription drops | `src/cancellation.rs` | always |
| `AggregateOwners.request_wake`, `CancellationOwnerState.request_wake` | An owner's request token wakes the aggregate's re-evaluation as it cancels; detaching withdraws it | REQUIRED | One aggregate; one registration per owner with a request token | `src/cancellation.rs` | always |
| `WaitFamily` and the `enter_wait` announcement at every synchronous dependency wait (`block_on` announces its family argument) | none in a production build: there `enter_wait` is an empty inline call | REQUIRED (the family vocabulary); the announcement body is OPTIONAL | Static | `src/blocking.rs` | vocabulary always; announcement body `cfg(any(test, feature = "test-support"))` |
| `WaitProbe` and its thread-local `PROBE` slot (recording and forbidding modes) | none; `verter_session`'s `root_continuations_tests` inventory and Pending-before-release harness read it | OPTIONAL | One installing thread, until the probe drops | `src/blocking.rs` | `cfg(any(test, feature = "test-support"))` |

## verter_scheduler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `CompletionSlot.wakers` (`WakerSet`) | `CompletionSender::send` takes every registered waker under the slot lock and wakes each after releasing it | REQUIRED | One completion handle, until it resolves | `src/job.rs` | always |
| `CompletionSubscription.registration` | `CompletionSubscription::poll` keeps or replaces it; its `Drop` withdraws exactly it | REQUIRED | One subscription | `src/job.rs` | always |
| `CompletionHandle::registered_wakers` | none; tests read it | OPTIONAL | Derived from live occupancy | `src/job.rs` | `cfg(any(test, feature = "test-support"))` |
| `SubmissionInbox.space` (`WakerSet`) | Admission offers register on full; dequeue, reset and quiesce wake capacity waiters after unlock | REQUIRED | One inbox; each entry until taken or withdrawn | `src/driver.rs` | always |
| `SchedulerSubmission` (retained scheduler, prepared input, output subscriptions), `PendingSubmission` (pending inbox item and registration) | Request and atomic-batch admission polls; synchronous submissions drive the same pending state | REQUIRED | One admission handoff | `src/scheduler/submission.rs` | always |
| `SchedulerDag.completion_wakes`, `DagMutex.delivery`, `WakeDeferral` | Terminal publication retains polling wakes until DAG unlock and any enclosing reset hold releases; blocking completion state remains immediately visible | REQUIRED | One lifecycle critical section; reset deferral spans its driver join | `src/dag.rs`, `src/scheduler/dag_lock.rs` | always |
| `Scheduler.driver_thread_id`, retained `driver_handle`, native startup handoff | Reset identifies its own callback and retains that driver's join authority until external reset/restart joins it; startup publishes identity and handle before releasing work | REQUIRED | One native driver incarnation, through its reset/restart join; the startup channel ends at admission | `src/scheduler.rs` | native |
| `ScopedCacheNodeHandoff` (admission, owner and registration), `ScopedCacheNodeProducer` (exclusive publication authority), `ScopedCacheFlightState.wakers` | Scoped cache-node polling and the synchronous `execute_scoped_cache_node` driver; dispatch and settlement defer wakes beyond lifecycle holds | REQUIRED | One flight, producer and independent subscriber; no durable retention | `verter_scheduler/src/scheduler/scoped_handoff.rs`, `cancellation.rs` | always |
| `ScopedCacheNodeHandoff.stop` (`CancellationSubscription`) | A pending handoff is woken by its request's cancellation | REQUIRED | One handoff, until it finishes | `verter_scheduler/src/scheduler/scoped_handoff.rs` | always |
| `Scheduler.task_registry`, scoped handoff/producer `task`, `ScopedCacheFlightState.producer`, handoff `wait` | Host composition shares its existing registry with semantic producers; producer execution installs the captured task and each follower holds a cycle-checked RAII edge to it | REQUIRED | One host registry; task until its last handle drops; follower edge until completion/refusal/drop | `verter_scheduler/src/scheduler.rs`, `scoped_handoff.rs`, `cancellation.rs` | always |

## verter_semantic_source

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeclWorker.space` (`WakerSet`) | `ShardHandoff::poll` registers on a full queue; the worker wakes every waiter after a dequeue, outside the lock; a handoff's `Drop` withdraws its own | REQUIRED | One worker; each entry until the worker takes it or its handoff withdraws it | `src/decl_lowering.rs` | always |
| `ShardReply` / `ReplyState` | The worker's `ReplySender::answer` publishes into it and wakes the registered poll; a closed reply fails the poll; an abandoned reply returns the answer to the worker to drop | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `ShardHandoff` (job until admitted, the worker's sender, space registration, reply, retained service) | The pollable entries and their synchronous drivers | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `LeaseHandoff`, `LeasedRunHandoff`, `LeasedBatchHandoff` (and the worker answers `LeaseAnswer`, `RunAnswer`, `ShardAnswer`) | `begin_acquire_lease*`, `begin_run_leased*` return them; `acquire_lease*` and `run_leased*` drive them | REQUIRED | One handoff | `src/decl_lowering.rs` | always |
| `LeaseHandoff.profile` / `LeasedRunHandoff.profile` (sink and submission instant) and the answers' worker `timing` | The handoff profile records the queue, service and response split for the decl-handoff profile dump | OPTIONAL | One handoff | `src/decl_lowering.rs` | `cfg(feature = "semantic-observe")` |
| `ShardHandoff::admitted` | none; tests read it | OPTIONAL | Derived | `src/decl_lowering.rs` | `cfg(test)` |

No trace or history is added. The former uninhabited `HandoffStats`
stand-in for default builds is removed, because no default-build code
names a profile any more.

## Additional flight and publication owners

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `InflightSlotState.wakers`, `SlotJoin.registration` | Artifact-cache and lookup/publish followers poll the shared slot; synchronous admission drivers still wait outside the slot poll | REQUIRED | One cache flight / subscriber | `verter_type_engine/src/cache_runtime/singleflight.rs` | always |
| `InflightState.wakers`, semantic subscription registration | Semantic memo subscribers poll, or the external driver waits, for a producer terminal | REQUIRED | One semantic flight / subscriber | `verter_type_engine/src/semantic_query_memo` | always |
| `QueryProgram.stop_wake` (`CancellationSubscription`) | `Program::wake_on_stop` registers the pending drive on the token its stop selects | REQUIRED | One semantic task, re-registered per pending poll | `verter_type_engine/src/project_semantic_dispatch/query_frames.rs` | always |
| `FlightState.wakers`, `FlightJoin.registration` | Resolver request followers retain their existing flight reference through polling and withdraw their registration on drop | REQUIRED | One resolver flight / follower | `verter_session/src/resolver_core/mod.rs` | always |
| `BuildRelease.registration` and manager build wakers | Store-view build followers poll for release; the owning build wakes after releasing its claim | REQUIRED | One manager build / follower | `verter_session/src/resolver_store.rs` | always |
| `LaneWaiters`, `LaneWait.registration` | Carrier publication and stable-parse lane followers register against the lane state; settlement wakes after unlock | REQUIRED | One publication or stable-parse lane / follower | `verter_session/src/carrier_publication_store/mod.rs` | always |
| `FlightSlot.wakers`, `FlightSubscription.registration` | Workspace import-resolution subscribers register for settlement and withdraw independently | REQUIRED | One resolution flight / subscriber | `verter_workspace/src/resolution_flights.rs` | always |

The semantic flight's blocking condvar (`FlightCell.ready`) is removed: the
synchronous flight wait parks through the same waker registration as a
poll, with its cancellation registered beside it. The timed cancellation
slices of the semantic drive, the scoped cache-node driver, the carrier
lanes and the workspace flight wait are removed with it.

## Host roots and the state they carry across suspension

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RootTask` (`computation`, `ambient`, `work`), `ComponentMetaOutputRoot`, `CompileRequestRoot` | The public pollable roots; `get_component_meta_output` and `compile_request` drive them | REQUIRED | One root, until it answers or is dropped | `verter_session/src/root_continuation.rs` | always |
| `RootAmbient` (captured request context, job cancellation token, task registry and execution task) | Installed on every poll of its root, restored after it | REQUIRED | One root | `verter_session/src/root_continuation.rs` | always |
| `RootWorkSlot` and `RootWork` (a transferred producer or a dispatch step) | The compile root's macro bundle hands them to the root's driver; the synchronous driver runs producers through `Scheduler::run_transferred_producer` and steps through `Scheduler::dispatch_for_pending_dependency` | REQUIRED | One handed-out piece of work, until its driver takes and runs or drops it | `verter_session/src/root_continuation.rs` | always |
| `VerterHost.task_registry` | Roots register or adopt their task in it; resolver flight groups and imported roots take it at construction | REQUIRED | The host | `verter_session/src/lib.rs`, `host_construction.rs` | always |
| `SingleflightGroup.tasks`, `FlightInner::Running.owner` (`TaskId`), a follower's `WaitEdge` | `run_retaining_pollable` claims, re-entry, cycle refusal and follower waits | REQUIRED | One group; owner until the lane terminates; edge until the follower answers or drops | `verter_session/src/resolver_core/mod.rs` | always |
| `FlightLease._thread` | Makes a workspace flight lease non-`Send`, so the thread lease count stays exact for the computation polling the thread | REQUIRED | One lease | `verter_workspace/src/resolution_flights.rs` | always |
| `BuildRelease` as a future, `BuildRole` | `base_view_pollable` waits for a store-view build release, holding no manager lock | REQUIRED | One pending read | `verter_session/src/resolver_store.rs` | always |
| Per-attempt component-meta snapshot continuation (pre-read supersession fingerprint, currentness and view) | `ComponentMetaRequestExecutor::snapshot_view` awaits the base or view-bound host read; the view-bound host overlays once after readiness | REQUIRED | One snapshot attempt, including its fallback | `verter_session/src/resolver_core/component_meta_request.rs`, `host_manage/component_meta_request_impl.rs` | always |
| Overlay index continuation (artifact identity, flight generation stamps, lease acquisition and index handoffs, retained parse lease) | Component-meta input capture awaits the indexed flight and declaration-lowering owner replies; the synchronous materializer drives the same computation | REQUIRED | One overlay index build; a completed artifact adopts the parse lease | `verter_session/src/host_manage/overlay_materialize.rs` | always |
| `StoreViewManager::build_release_registrations` | none; root tests read it | OPTIONAL | Derived | `verter_session/src/resolver_store.rs` | `cfg(test)` |
| `SyncRefCell`, `SyncCell` (the dispatcher's request state, the connected demand ledger and the guard references to them) | Every dispatcher borrow, with `RefCell`/`Cell` borrow discipline and an atomic flag | REQUIRED | One dispatcher | `verter_execution/src/sync_cell.rs`, `verter_type_engine/src/project_semantic_dispatch` | always |
| `RefusalsWithin` (the operation's own refusal record) | The compile root's entry records parse-stack refusals across its polls | REQUIRED | One recording operation | `verter_parser/src/oxc_parse/stack.rs` | always |
| `WaitFamily::Root`, `block_on_when_pending` | The synchronous entries over the roots, and the synchronous store-view read | REQUIRED | Static | `verter_execution/src/blocking.rs` | always |
| `drive_pending`, `complete_now`, `on_heap` | Synchronous forms of the converted computations drive their one pollable implementation; `on_heap` keeps the cold component-meta computations off the driving stack | REQUIRED | One synchronous call | `verter_execution/src/blocking.rs` | always |
| The component-meta root's own request context | Created where the root begins when no request is current, and installed on each poll as its ambient request | REQUIRED | One root | `verter_session/src/root_continuation.rs` | always |
| `ComputeSpendLog`, `ComputeSpan`, `OwnedFactTracer::fanning_out` | `traced_component_meta_compute_pollable` owns its spend log, span and tracer and installs them for each poll | REQUIRED | One component-meta compute | `verter_type_engine/src/request_budget.rs`, `project_semantic_dispatch/connected_demand.rs`, `resolver_core/resolver_context.rs` | always |
| `SuspendableDispatch` (the dispatcher and its graph's construction arena) | The cold component-meta output path; its lane is entered for each poll | REQUIRED | One cold output computation | `verter_type_engine/src/project_semantic_dispatch/mod.rs` | always |
| `RequestAudit` (`request_id`, `RequestPhaseAudit`) | The component-meta resolve's phase audit, installed for each poll by `within_request_audit` | OPTIONAL | One audited resolve | `verter_session/src/component_meta_audit/mod.rs` | `semantic-observe` |
| `AttributedPolls` | `attribute_polls` scopes a work site to each poll of an IndexedReady build | OPTIONAL | One attributed computation | `verter_audit/src/attribution/mod.rs` | `semantic-observe` |

No trace or history is added, and every REQUIRED item above holds with capture off.
