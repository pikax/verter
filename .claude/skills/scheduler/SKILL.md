---
name: scheduler
description: Verter scheduler — Scheduler, KeyedJob, CacheNodeDagNode, TaskKind (Load/Parse/Analysis/Artifact/CacheNode), pool routing (CPU vs I/O), HostCpuPool + SchedulerCpuPool ownership, DAG submission, dedupe-hook integration
---

# Scheduler

This skill is the concise reference for the `verter_scheduler` crate.
It covers the public submission surface (`submit_request` /
`submit_dag`), the `KeyedJob` / `CacheNodeDagNode` types, the
`TaskKind` variant set, CPU vs I/O pool routing, the dual
host-CPU / scheduler-CPU pool ownership, DAG semantics (dependency
gating, priority inheritance, cancellation propagation, bounded
admission / backpressure), and the generic dedupe-hook surface.

The binding implementation spec lives in
`docs/arch/cache-runtime-overhaul-plan.md` (Blocks 6 and 7). When in
doubt, the plan wins; this skill derives from the plan body.

## Crate dependency invariant

`verter_scheduler` MUST NOT depend on any higher-level crate. The
dependency runs one-way: higher-level crates depend on
`verter_scheduler`, never the reverse. The skill never names a
symbol that lives in a higher-level crate — any such reference is a
cycle and a violation.

Guard:
`crates/verter_scheduler/tests/no_session_dep.rs::scheduler_does_not_depend_on_verter_session`
walks `crates/verter_scheduler/Cargo.toml`, every `.rs` file under
`crates/verter_scheduler/src/**` (parsed with `syn::parse_file`),
AND this skill markdown. It asserts NO mention of any higher-level
crate appears in any `use` statement, any `dependencies` /
`dev-dependencies` table, OR any skill prose substring. The guard
treats the skill as a substrate input so a relapse in this file
fails the build.

## Generic dedupe-hook surface

The scheduler exposes a generic dedupe-hook trait that the calling
crate implements over its own in-flight table. The scheduler itself
owns NO in-flight cache table — the calling crate deduplicates
BEFORE submitting to the scheduler.

```rust
// crates/verter_scheduler/src/dedupe_hook.rs
pub trait DedupeHook: Send + Sync {
    /// Probe whether `dedup_key` is already known to the caller's
    /// in-flight table. If `Some`, the caller blocks on the existing
    /// flight and the scheduler skips enqueue.
    fn probe(&self, dedup_key: &DedupKey) -> Option<DedupeJoiner>;
}

/// Opaque handle the caller may use to attach a completion as a
/// joiner on an in-flight flight.
pub struct DedupeJoiner { _opaque: () }
```

`DedupKey` is defined in `crates/verter_scheduler/src/job.rs`
alongside `KeyedJob`. Both the trait and the joiner type are fully
owned by `verter_scheduler`; no method signature or struct field on
either type references any higher-level crate.

`Scheduler::submit_request` / `submit_dag` accept an optional
`&dyn DedupeHook` argument. When present, the scheduler probes the
hook before admission. When absent (e.g. unit tests, raw scheduler
callers), the scheduler proceeds directly. The scheduler never
imports any concrete in-flight-table type from a higher-level crate.

## KeyedJob, DedupKey, and `CacheNodeDagNode` lifecycle

`KeyedJob` is the submission identity. `CacheNodeDagNode` is the
ready-queue envelope that the driver dispatches. The inbox-level
enum `driver::Submission` is a separate type that owns its own
discriminator variants.

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub canonical: std::sync::Arc<str>,
    pub stage: TargetStage,
    pub content_hash: u64,
}

#[derive(Clone, Debug)]
pub struct KeyedJob {
    pub dedup_key: DedupKey,
    pub stage: TargetStage,
    pub priority: Priority,
    /// World generation under which the job was enqueued. Dispatch
    /// reads `node.keyed_job.generation` directly; `CacheNodeDagNode`
    /// has no `generation()` accessor.
    pub generation: u64,
}
```

`KeyedJob` carries NO `task` / `task_kind` field. The task
discriminator lives on `CacheNodeDagNode.task_kind` only — there is
one source of truth.

Lifecycle (steady state):

1. **Caller-side dedupe.** Cache-runtime callers consult their
   in-flight table FIRST. A matching flight short-circuits — no
   scheduler submission happens.
2. **Submit.** The caller invokes `Scheduler::submit_request(req)` or
   `Scheduler::submit_dag(dag)` (optionally passing a `DedupeHook`).
   `submit_request` lands a `driver::Submission::NewRequest` on the
   inbox. `submit_dag` constructs a `CacheNodeDag` and pushes its
   ready nodes into the bounded ready queue as upstream gates fire.
3. **Scheduler-side dedupe probe.** The driver computes
   `dedup_key_for(req)` and consults `pending_requests` (the
   scheduler's own per-process inbox-level dedupe). A duplicate
   `DedupKey` attaches the caller's `CompletionSender<RequestResult>`
   as a joiner on the existing flight; no new job is enqueued.
4. **Admission.** A non-dedup submission is admitted to the priority
   ready queue
   (`Arc<crossbeam_queue::ArrayQueue<Arc<CacheNodeDagNode>>>` — the
   inner `Arc` is required because `CacheNodeDagNode` is not
   `Clone`: its `CacheNodeCompletionSender` wraps a single-use
   `tokio::sync::oneshot::Sender`, so the same node lives on both
   the ready queue and `DagState.nodes` only via `Arc`-sharing),
   subject to
   the bounded-admission policy below. Per-call CPU concurrency is
   enforced by the worker dispatch site (per-task
   `cpu_concurrency_semaphore.acquire()`), not by admission.
5. **Execution.** The driver pops a ready node and dispatches via
   `TaskKind` routing:
   - `Load` → `IoPool::submit`;
   - `Parse` / `CacheNode` / CPU `Analysis` / CPU `Artifact` →
     `SchedulerCpuPool::submit`.
6. **Completion.** `pending_requests` is cleared; every joiner
   receives the result through their attached
   `CompletionSender<RequestResult>`; DAG dependents are
   re-evaluated for readiness. The worker's per-task
   `CpuConcurrencyPermit` drops via RAII immediately after the task
   body returns, releasing the semaphore counter and notifying one
   waiter.

## Dual pool ownership

Two distinct `rayon::ThreadPool`s cooperate so the batch-orchestration
outer wait and the scheduler's CPU stage executor cannot deadlock on
the same workers. They are owned by DIFFERENT layers — the split is
the deadlock-isolation invariant.

- **Scheduler stage pool (`cpu_pool`)** — owned BY the scheduler,
  built internally in `Scheduler::with_executor` /
  `new_sync_with_executor` from `SchedulerConfig::cpu_threads`. It is
  the ONLY pool for parse/analysis/artifact/cache-node stage
  execution: the driver dispatches `TaskKind::Parse` /
  `TaskKind::CacheNode` / CPU `Analysis` / CPU `Artifact` onto it via
  `cpu_pool.spawn(...)`. Workers register `CallerKind::CpuWorker` so
  `wait_or_drive` routes them to the cooperative-pump branch. The
  scheduler also owns the bounded `io_pool`
  (`SchedulerConfig::io_threads`) for `TaskKind::Load`.
- **Coordinator pool (`HostCpuPool`)** —
  `crates/verter_scheduler/src/host_cpu_pool.rs`. Constructed once at
  startup by the external host/runtime layer via
  `verter_scheduler::HostCpuPool::new(num_threads)` and owned THERE,
  as a sibling of the `Scheduler` — NOT passed into the scheduler and
  NOT a field on it. Reserved for the outer batch coordinator's
  synchronous wait points. Its workers register `CallerKind::External`
  (8 MiB stacks), so they PARK in `wait_or_drive` rather than
  inline-executing scheduler CPU tasks, and the driver's inline-execute
  branch excludes `External` — coordinator-pool workers therefore NEVER
  run `TaskKind::Parse` / `TaskKind::CacheNode`.

The scheduler constructor takes only `(config, source_loader[,
executor])` — there is NO pool parameter. The scheduler builds and owns
its `cpu_pool` + `io_pool`; the coordinator pool lives entirely in the
external layer:

```rust
impl Scheduler {
    pub fn new(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
    ) -> Arc<Self> {
        Self::with_executor(config, source_loader, Arc::new(DefaultExecutor))
    }

    pub fn with_executor(
        config: SchedulerConfig,
        source_loader: Arc<dyn SourceLoader>,
        executor: Arc<dyn StageExecutor>,
    ) -> Arc<Self> {
        // Build `cpu_pool` (config.cpu_threads, CpuWorker-tagged) and
        // `io_pool` (config.io_threads); spawn the driver thread
        // holding `Weak<Scheduler>`. No coordinator pool here.
    }
}

pub struct Scheduler {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) cpu_pool: rayon::ThreadPool, // stage execution ONLY
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) io_pool: crate::pool::IoPool,
    // ... other existing state (inbox, edges, dag, overlay, source_loader,
    //     executor, tombstones, generation_floors, deferred_blocker_ids,
    //     removal_epoch, shutdown, driver_handle, counters, config) ...
}
```

**Single batch-coordination primitive (lives in the external host/
runtime layer, not in this crate).** Every host/runtime batch API
(batch component-meta, batch SFC compile, and any future batch
fan-out) routes its outer wait through ONE coordinator primitive owned
by the external layer. That primitive — not the scheduler — owns:
coordinator-pool `install`; the empty / single-item fast path;
deterministic per-input ordering; panic / cancellation / shutdown
propagation; and the non-reentrant policy below. The scheduler crate
exposes NO outer-fan-out API and performs NO `par_iter().install(...)`
outer wait on its `cpu_pool`; a batch's per-batch submission accounting
is a pool-free counter bump (`Scheduler::account_batch_submission`).

**Non-reentrant host-batch contract.** A batch item closure may call
scalar scheduler operations, but a nested batch fan-out reached from
inside an item closure must NOT issue a fresh coordinator-pool install.
The external primitive detects re-entrancy (a per-thread marker scoped
around each item's execution) and runs the nested fan-out INLINE /
sequentially on the current coordinator worker. Stacking a second outer
wait on the same finite coordinator pool would reintroduce the
starvation class one level up.

**Deadlock-free property + new invariant.** The two pools are distinct
and owned by different layers: **no worker waits for a job in its OWN
pool.** A coordinator-pool worker may block on scheduler stage work
without deadlock because the scheduler's `cpu_pool` has its own worker
set that proceeds independently; a `cpu_pool` worker running
`TaskKind::Parse` is not a coordinator worker and does not gate the
outer coordinator's wait. The invariant in full:

> Outer API fan-out may block only on scheduler-owned work;
> scheduler-owned work must never require coordinator-pool workers;
> nested host-batch fan-out is rejected or collapsed inline by the
> external batch coordinator. External host/runtime layers own the
> coordinator pool(s) and the batch-coordination primitive; they must
> not run outer waits on the scheduler stage pool.

Guards live in the external host/runtime layer (not in this crate, to
preserve the one-way dependency): a watchdog-bounded regression
characterizes the starvation deadlock (cold cross-file deps + a stage
pool sized to the batch width), and the coordinator primitive's own
reentrancy test pins the inline collapse of a nested batch.

## Per-call concurrency semaphore

`compile_many` has no per-call `threads` option — the `HostCpuPool`
worker count is sized once at host construction from
`HostConfig::host_cpu_threads`, and the pool is reused across every
`compile_many` call. Per-call concurrency capping on
`SchedulerCpuPool` admissions (the `CpuConcurrencySemaphore` handle
propagated through `CacheNodeDagNode`) is deferred to §6d; until §6d
lands, scheduler-side admission runs at the pool's default
concurrency.

When §6d lands, callers attach the handle to every
`CacheNodeDagNode.cpu_concurrency_semaphore` in the batch DAG:

```rust
impl Scheduler {
    /// Construct a per-batch CPU concurrency semaphore HANDLE.
    /// Returns the `Arc<CpuConcurrencySemaphore>` the calling crate
    /// attaches to every `CacheNodeDagNode.cpu_concurrency_semaphore`
    /// in the batch DAG. The worker dispatch site acquires a FRESH
    /// `CpuConcurrencyPermit` from the semaphore IMMEDIATELY BEFORE
    /// each task body runs; the permit drops on task completion.
    pub fn cpu_concurrency_semaphore(&self, n: usize)
        -> Arc<CpuConcurrencySemaphore> { /* ... */ }
}
```

`CpuConcurrencySemaphore` is a hand-rolled counting primitive
defined in `crates/verter_scheduler/src/cpu_concurrency.rs`. The
substrate is `parking_lot::Mutex<usize>` + `parking_lot::Condvar` —
the only synchronisation primitives `parking_lot 0.12` exports
(`parking_lot::Semaphore` does NOT exist in that version).
`CpuConcurrencyPermit` is the RAII guard returned by
`semaphore.acquire()`; dropping it increments the counter and
notifies one waiter.

Propagation model: every `CacheNodeDagNode` carries
`cpu_concurrency_semaphore: Option<Arc<CpuConcurrencySemaphore>>` —
the SEMAPHORE HANDLE, NOT a pre-acquired permit. The worker
dispatch site calls `sem.acquire()` per task immediately before the
executor runs the body; the permit drops on task completion.
Cloning the `Arc<CpuConcurrencySemaphore>` across N DAG nodes does
NOT pre-acquire N permits — only `acquire()` consumes a permit. This
is the only shape that enforces "max `capacity` concurrent CPU
tasks" across the DAG. A design propagating a shared pre-acquired
`Arc<CpuConcurrencyPermit>` would acquire ONE permit at submission
and let N>capacity tasks run concurrently.

## TaskKind routing

> Pool naming: the stage-execution CPU pool is the scheduler-owned
> `cpu_pool` (`rayon::ThreadPool`, dispatched via `cpu_pool.spawn`); see
> *Dual pool ownership* for the authoritative pool model. The
> `SchedulerCpuPool::submit` form below is the cache-runtime
> DAG-submission design target; on the current tree the same stage work
> dispatches onto `cpu_pool`.

The scheduler routes:

- `TaskKind::Load { canonical }` → I/O pool (pure I/O — reads bytes
  off disk; no executor dispatch, the source loader drives the I/O
  directly).
- `TaskKind::Parse { canonical, source, file_kind }` → stage CPU pool
  (pure CPU; payload carries `file_kind` so `execute_source`
  classifies without re-deriving from path).
- `TaskKind::Analysis { canonical, source_snapshot }` →
  `SchedulerCpuPool::submit`. Dispatch destructures `canonical` off
  the payload and passes the snapshot reference to
  `execute_analysis`. The substrate `SourceSnapshot` has no
  `canonical_id()` accessor — `canonical` lives on the variant.
- `TaskKind::Artifact { canonical, source_snapshot, analysis_snapshot, profile_hash }`
  → `SchedulerCpuPool::submit`. Same payload-bearing shape.
- `TaskKind::CacheNode { cache_id: SchedulerCacheId, key_hash: u64 }`
  → `SchedulerCpuPool::submit`. The worker dispatches through
  `execute_cache_node(&node, &ctx) -> CacheNodeOutcome` (direct
  return, NOT `Result`-wrapped). `SchedulerCacheId` is the
  scheduler-local enum defined in
  `crates/verter_scheduler/src/cache_id.rs`; it is named distinctly
  so it never silently shadows any same-short-name type from a
  downstream consumer.

`TaskKind` is no longer `Copy` — payload-bearing variants carry
`Arc<str>` / `Arc<SourceSnapshot>` etc. Every existing `Copy` call
site (e.g. `supersede_old_generations` at `scheduler.rs:388`)
becomes an `Arc` clone. The discriminating test
`task_kind_clone_is_cheap_arc_clone` pins the clone cost at < 100ns
p99.

The legacy `TaskKind::Source` (which combined load + parse on I/O)
is RETIRED. The source loader synthesises a `Load → Parse` DAG
edge. `SchedulerJobKind` (the existing non-staged component-meta
batch enum at `stage.rs:19`) is **retained** unchanged — it
discriminates `ComponentMeta { canonical_id }`. The scheduler does
NOT own the batch fan-out for it: the external host/runtime layer maps
these job items and fans them out through its own batch-coordination
primitive (see *Dual pool ownership*), calling
`Scheduler::account_batch_submission` once per non-empty batch for the
O(1) submission accounting. The new `TaskKind::CacheNode` variant
lives alongside it on the new ready-queue envelope.

### StageExecutor dispatch surface

The `StageExecutor` trait exposes five dispatch methods, one per
`TaskKind` variant. Workers route through `TaskKind` at dispatch
time; there is no bare `executor.execute(node)` method.

| TaskKind         | StageExecutor method   | Return                                |
|------------------|------------------------|---------------------------------------|
| `Parse`          | `execute_source`       | `Result<SourceSnapshot, StageError>`  |
| `Analysis` (CPU) | `execute_analysis`     | `Result<AnalysisSnapshot, StageError>`|
| `Artifact` (CPU) | `execute_artifact`     | `Result<ArtifactSnapshot, StageError>`|
| `CacheNode`      | `execute_cache_node`   | `CacheNodeOutcome` (NOT `Result`-wrapped; errors live inside `CacheNodeOutcome::CacheNode(Err(_))`) |
| `Load` (I/O)     | (no executor; source loader directly via `IoPool::submit`)     |

The trait also requires `fn as_any(&self) -> &dyn std::any::Any`
(object-safe, no default body) for the test-support
`Scheduler::last_dispatched_task` downcast. Every concrete impl
(`DefaultExecutor`, `HostStageExecutor`, the test-support
`LastDispatchedTaskRecorder`) provides the one-line body.

The worker's `dispatch_cpu_task` constructs a `CacheNodeDispatchCtx<'_>`
(dedup key, generation, optional audit observer, cancellation token)
BEFORE the `match` and passes a non-owning borrow to
`execute_cache_node`. The default body returns
`CacheNodeOutcome::stub()`; the host overrides to drive the
cache-runtime artifact / query node trait surface. The other three
CPU dispatch methods return their stage-specific result, and the
worker maps each into the unified `CacheNodeOutcome` via the
`CacheNodeOutcome::from_source` / `from_analysis` / `from_artifact`
adapters before writing it on `node.completion`.

## DAG submission semantics

The DAG API is one method, one type, one signature, carrying every
field the driver requires to dispatch each node as an executable
unit of work:

```rust
pub struct CacheNodeDag {
    pub nodes: Vec<CacheNodeDagNode>,
    pub edges: Vec<CacheNodeDagEdge>,
    pub completion_aggregator: Arc<DagCompletionAggregator>,
}

pub struct CacheNodeDagNode {
    pub id: CacheNodeId,
    pub keyed_job: KeyedJob,
    pub task_kind: TaskKind,
    pub dedup_key: DedupKey,
    pub priority: Priority,
    pub cancellation_token: CancellationToken,
    /// SEMAPHORE HANDLE (NOT a pre-acquired permit). The worker
    /// takes a fresh RAII permit on task dispatch; the permit drops
    /// on task completion.
    pub cpu_concurrency_semaphore: Option<Arc<CpuConcurrencySemaphore>>,
    /// Scheduler-local opaque wrapper
    /// (`crates/verter_scheduler/src/request_context.rs:103`,
    /// `pub struct OpaqueRequestContext(pub Arc<dyn RequestContextLike>)`).
    /// Calling crate wraps its concrete context inside
    /// `OpaqueRequestContext(arc as Arc<dyn RequestContextLike>)`
    /// when constructing the node.
    pub request_context: Arc<OpaqueRequestContext>,
    /// Cache-node-only completion channel. Wraps
    /// `tokio::sync::oneshot::Sender<CacheNodeOutcome>` in
    /// `Mutex<Option<...>>` so the worker dispatch site can `take()`
    /// the inner sender out of a shared `&CacheNodeDagNode` borrow.
    /// RENAMED from earlier drafts' `node::CompletionSender` to
    /// avoid collision with the substrate's `job::CompletionSender<T>`.
    pub completion: CacheNodeCompletionSender,
}

pub struct CacheNodeDagEdge {
    pub from: CacheNodeId,
    pub to: CacheNodeId,
    pub gate: EdgeGate,
}

pub enum EdgeGate {
    Sequential,
    ConditionalOnSuccess,
    ConditionalOnAdmission,
}

impl Scheduler {
    pub fn submit_dag(&self, dag: CacheNodeDag) -> DagHandle { /* ... */ }
}
```

The nine-field `CacheNodeDagNode` envelope is complete: the
`task_kind` discriminator lives on the node only (NOT on
`KeyedJob`). No node enters the ready queue without all nine
fields populated; the driver does NOT enrich nodes after
submission. Guard:
`cache_node_dag_carries_required_fields_for_executable_dispatch`.

DAG contract:

- **Dependency gating.** A downstream node is not admitted to the
  ready queue until ALL of its upstream nodes have completed per
  their `EdgeGate` policy.
- **Priority inheritance.** Effective priority is
  `max(node_priority, max(root_priority for every reachable root))`.
- **Cancellation propagation.** Dropping a `DagHandle` triggers
  `CancellationToken::cancel()` on every node not yet completed;
  cancellation propagates transitively through edges.
- **Bounded admission / backpressure.** The ready queue is bounded
  by `MAX_READY_QUEUE_DEPTH = 64`
  (`crates/verter_scheduler/src/queue.rs`). When full, additional
  submissions either block or return
  `SubmissionResult::Backpressure` per caller preference.
  `Scheduler::ready_queue_depth()` exposes the current bounded
  depth for observability only.
- **In-flight dedupe inside a DAG.** Two nodes in the same DAG
  sharing a `dedup_key` collapse via scheduler-side
  `pending_requests`. Cross-DAG dedupe uses the consumer-side
  in-flight table via `DedupeHook::probe` BEFORE submission.

`submit_batch(reqs: Vec<Request>)` is a thin shim that constructs a
no-edge `CacheNodeDag` and calls `submit_dag`.

## Scheduler surface diff (today → post-cutover)

| Method | Today | Post-cutover |
|---|---|---|
| `submit_request(req)` | inbox + per-request `CompletionHandle` | unchanged signature; gains optional `&dyn DedupeHook` arg |
| `submit_batch(reqs)` | loop over `submit_request` | thin shim over `submit_dag` (no-edge DAG) |
| `submit_dag(dag) -> DagHandle` | absent | NEW |
| `dedup_key_for(req) -> DedupKey` | absent | NEW |
| `cpu_concurrency_semaphore(n) -> Arc<CpuConcurrencySemaphore>` | absent | NEW |
| `ready_queue_depth() -> usize` | absent | NEW |
| `register_resolved_deps` | unchanged | unchanged |

## Pool routing rules

- **`io_pool`** owns `TaskKind::Load { canonical }` and any other
  pure-I/O work. A parse closure on the I/O pool is a bug
  (`pool_isolation::source_parse_runs_on_cpu_pool_not_io_pool`).
- **Scheduler stage pool (`cpu_pool`)** owns `TaskKind::Parse`,
  `TaskKind::CacheNode`, and CPU-bound `TaskKind::Analysis` /
  `TaskKind::Artifact`. The scheduler builds and owns it internally
  from `SchedulerConfig::cpu_threads`; it is NOT passed into the
  constructor. This is the only pool the driver dispatches stage work
  onto.
- **Coordinator pool (`HostCpuPool`)** owns the outer batch
  coordinator's wait points. The external host/runtime layer
  constructs it once at startup and OWNS it (a sibling of the
  `Scheduler`, never handed into the constructor). The scheduler does
  NOT reference it and NEVER dispatches tasks onto it; equally, no
  scheduler API installs an outer wait on the stage pool. The
  coordinator pool is reused across batch calls (sized once from the
  external layer's config). Guard (external layer):
  `two_back_to_back_compile_many_share_pool`.

## Test-support helpers (`feature = "test-support"`)

The crate gates a small set of fixture helpers behind
`feature = "test-support"` so the production build never compiles
them. Integration tests under `crates/verter_scheduler/tests/`
enable the feature via
`verter_scheduler = { path = ".", features = ["test-support"] }`
in their `[dev-dependencies]`.

- `Scheduler::new_for_test() -> Arc<Self>` — single-thread pools +
  `LastDispatchedTaskRecorder` executor.
- `Scheduler::enqueue_analysis(&FileNode)` — routes through
  `submit_request` with `TargetStage::Analysis`. Drives to
  quiescence by polling
  `last_dispatched_task() -> Some((_, TaskKind::Analysis { canonical, .. })) if canonical == fixture_canonical`
  (matches specifically on `Analysis`, not on the upstream `Parse`
  dispatch that the driver completes first).
- `Scheduler::last_dispatched_task() -> Option<(KeyedJob, TaskKind)>`
  — downcasts `Arc<dyn StageExecutor>` to
  `LastDispatchedTaskRecorder` via `as_any` and reads its internal
  cell.
- `LastDispatchedTaskRecorder` — records the last
  `(KeyedJob, TaskKind)` per dispatch.
- `NoopSourceLoader` — full `SourceLoader` impl (four methods:
  `load`, `exists`, `classify`, `realpath`).
- `FileNode::stub_with_canonical(&str)` — populates `canonical_id`
  from the argument; zero-state for other fields.
- `OpaqueRequestContext::test_stub()` — wraps a private no-op
  `RequestContextLike` impl.
- `KeyedJob::stub()`, `DedupKey::new_for_test()`,
  `CacheNodeDagNode::stub()`,
  `CacheNodeDispatchCtx::stub_with(&dedup_key, &cancellation)`,
  `SchedulerCacheId::Test`.

See also:
- `.claude/skills/host-session/SKILL.md` — host-side ownership.
- `.claude/skills/type-cache-architecture/SKILL.md` — the substrate
  the scheduler dispatches into; defines `DedupeHook` consumers.
- `docs/arch/cache-runtime-overhaul-plan.md` — the plan that landed
  the cache-runtime + scheduler integration (Blocks 6, 7).
