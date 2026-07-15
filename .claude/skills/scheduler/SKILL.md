---
name: scheduler
description: Verter scheduler — Scheduler, submit_request/submit_batch/submit_batch_atomic (atomic DAG admission via driver-drained NewRequestBatch + shared admission core + deferred DedupJoinerEvent), wait_batch (input-order), live TaskKind (Source/Analysis/Artifact), CPU vs I/O pool routing, scheduler-owned cpu_pool + host-owned HostCpuPool coordinator (shared by every host batch API), account_batch_submission; plus the landed-but-unwired leaf substrate (CpuConcurrencySemaphore/CpuConcurrencyPermit, CancellationToken, opaque SchedulerCacheId newtype, caller-side DedupeHook trait, SubmissionResult) and the not-yet-implemented cache-runtime DAG design target (KeyedJob/CacheNodeDagNode/SchedulerCpuPool/submit_dag/DAG submission)
---

# Scheduler

Concise reference for the `verter_scheduler` crate.

**Live** surface (current tree): the `submit_request` / `submit_batch` /
`submit_batch_atomic` submission API, the live `TaskKind` variant set
(`Source` / `Analysis` / `Artifact`), CPU vs I/O pool routing, and dual
pool ownership — the scheduler-owned stage `cpu_pool` (+ `io_pool`) plus
the host-owned `HostCpuPool` coordinator shared by every host batch API,
with per-batch `account_batch_submission` accounting. *Dual pool
ownership* is the authority for the live pool model.

`submit_batch` is non-atomic (N separate `Submission::NewRequest` items;
the pump may observe the batch half-admitted, `submit_count` bumped per
item). `submit_batch_atomic` lands ONE
`Submission::NewRequestBatch { requests: Vec<QueuedRequest> }` that the
driver drains as a unit and admits under a SINGLE `dag.lock()`
acquisition via `handle_new_request_batch` (generation bumps + supersede
sweeps + waiter registration for every request inside one critical
section): the pump can never observe a half-admitted batch, one batch is
ONE wake + ONE `submit_count` bump, and a source-updating batch
supersedes every file's old generation atomically. Both paths share one
admission core — `prepare_request` (pre-lock: tombstone gate + node
ensure, cloning the `FileNode` `Arc` out of the `nodes` DashMap BEFORE
locking, the AB-BA-safe DAG-first ordering), `admit_prepared_under_lock`
(sole place a request bumps generation, runs the supersede sweep,
registers the waiter, admits work), and an `AdmissionPostWork`
accumulator firing deferred dedup callbacks + clearing auto-ingest
tracking AFTER the lock releases. `SchedulerDag::register_request`
returns `Option<DedupJoinerEvent>` (fired post-unlock via
`DedupJoinerEvent::fire`) instead of invoking `on_dedup_joiner` under the
DAG lock — the callback may re-enter the scheduler, so it must not run
while admission holds the mutex. `BatchHandle` carries one
`CompletionHandle` per input in submission order; `wait_batch(&self,
&BatchHandle)` returns results in INPUT order and never surfaces a
partial set. Pump discipline is preserved throughout: dispatch / wait /
parse / compile / callbacks all run outside the DAG lock, and capacity
stays reserved at dequeue time. `compile_many` IS wired onto atomic
batch admission: its source-upsert stage routes every input through
`VerterHost::upsert_many_with_priority` (the upsert engine), which lands
ONE `submit_batch_atomic` + ONE `wait_batch` for the whole batch rather
than one upsert per file. Per-call worker count is NOT a parameter of
`compile_many` — concurrency is the construction-time host-owned
`HostCpuPool` (`HostConfig::host_cpu_threads`); see *Dual pool
ownership*.

A **leaf substrate** for the cache-runtime DAG design has LANDED but is
UNWIRED (no submission path takes it as an argument yet): the
hand-rolled `CpuConcurrencySemaphore` + `CpuConcurrencyPermit`
(`cpu_concurrency.rs`), the `CancellationToken` (`cancellation.rs`), the
opaque `SchedulerCacheId` newtype relocated into `cache_id.rs`, the
caller-side `DedupeHook` trait + `DedupeJoiner` + `NoDedupeHook`
(`dedupe_hook.rs`), and the `SubmissionResult<T>` substrate (`Admitted` /
`DedupeJoined` / `Backpressured`). These primitives are correct and
tested in isolation; the submission API does not yet consume them.
Sections below describe each.

The rest of the **cache-runtime DAG design target** is still NOT on the
tree: the `submit_dag` / `CacheNodeDag` DAG surface, the `KeyedJob` /
`CacheNodeDagNode` types, the expanded `Load` / `Parse` / `CacheNode`
`TaskKind` variants on `SchedulerCpuPool`, the `SchedulerCpuPool` /
`SchedulerIoPool` typed pools, DAG semantics (dependency gating,
priority inheritance, cancellation propagation, bounded admission /
backpressure), and the wiring of `CpuConcurrencySemaphore` onto DAG
node dispatch. Every section describing those un-landed surfaces carries
an explicit "Not yet implemented" banner.

Binding implementation spec: `docs/arch/cache-runtime-overhaul-plan.md`
(Blocks 6 and 7). When in doubt, the plan wins; this skill derives from
the plan body.

## Crate dependency invariant

`verter_scheduler` MUST NOT depend on any higher-level crate. Dependency
runs one-way: higher-level crates depend on `verter_scheduler`, never the
reverse. The skill never names a symbol living in a higher-level crate —
any such reference is a cycle and a violation.

Guard:
`crates/verter_scheduler/tests/cases/no_session_dep.rs::scheduler_does_not_depend_on_verter_session`
walks `crates/verter_scheduler/Cargo.toml`, every `.rs` file under
`crates/verter_scheduler/src/**` (parsed with `syn::parse_file`), AND
this skill markdown. Asserts NO mention of any higher-level crate appears
in any `use` statement, any `dependencies` / `dev-dependencies` table, OR
any skill prose substring. The guard treats the skill as a substrate
input so a relapse in this file fails the build.

## Generic dedupe-hook surface

The `DedupeHook` trait IS on the current tree, in
`crates/verter_scheduler/src/dedupe_hook.rs`. It is the **caller-side
pre-admission singleflight** hook: the calling crate implements it over
its own in-flight table and the scheduler probes it BEFORE a submission
reaches the DAG, so a caller already holding an equivalent live flight
can skip the scheduler round-trip entirely and attach as a joiner. The
scheduler owns NO in-flight cache table — the calling crate deduplicates
BEFORE submitting.

DISTINCT from the scheduler-internal post-unlock `DedupJoinerEvent`
(`crate::dag`): that is the waiter-notify fired after the DAG lock
releases, once admission has already joined a request onto an existing
waiter group. `DedupeHook` runs on the caller's side before a submission
is even constructed; `DedupJoinerEvent` runs inside admission. Two
different lifecycle points, two different types.

```rust
// crates/verter_scheduler/src/dedupe_hook.rs
pub trait DedupeHook: Send + Sync {
    /// Probe whether `identity` is already known to the caller's
    /// in-flight table. If `Some`, the caller blocks on the existing
    /// flight and the scheduler skips enqueue; if `None`, the
    /// submission proceeds to admission as usual.
    fn probe(&self, identity: &WorkNodeIdentity) -> Option<DedupeJoiner>;
}

/// Opaque handle the caller uses to attach a completion as a joiner
/// on an in-flight flight (no public fields).
#[derive(Debug)]
pub struct DedupeJoiner { /* opaque */ }

/// The genuine no-op hook used wherever a caller supplies no in-flight
/// table — `probe` always returns `None`. NOT a stub: its contract IS
/// "never deduplicate".
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDedupeHook;
```

The probe key is `crate::dag::WorkNodeIdentity` — the scheduler's own
dedupe identity and the **single dedupe-identity authority**. NO parallel
`DedupKey` type: any public dedupe key is a thin wrapper/derivation of
`WorkNodeIdentity`, never a separate key, so there is one source of truth
for dedupe identity (leaf-boundary invariant H20). The trait,
`DedupeJoiner`, and `NoDedupeHook` are fully owned by `verter_scheduler`;
no method signature or struct field on any references a higher-level
crate.

The submission path probes the hook before admission. On `Some`, the
caller blocks on the existing flight and the scheduler skips enqueue
(surfaced as `SubmissionResult::DedupeJoined`, see *SubmissionResult
substrate*); on `None`, the submission proceeds to admission. The
scheduler never imports any concrete in-flight-table type from a
higher-level crate. Wiring the hook into `submit_request` / `submit_dag`
as an explicit `&dyn DedupeHook` argument on those entry points is a
future sub-block — the trait substrate is landed and unwired.

## SubmissionResult substrate

`SubmissionResult<T>` (`scheduler.rs`, LANDED) is the typed result of a
submission attempt, generic over the success-handle type `T`. Exactly
three variants — no speculative fourth case:

```rust
pub enum SubmissionResult<T> {
    /// Admitted into the DAG; carries the caller's handle.
    Admitted(T),
    /// Collapsed onto an in-flight flight by a caller-side
    /// `DedupeHook` probe; carries the opaque `DedupeJoiner`.
    DedupeJoined(crate::dedupe_hook::DedupeJoiner),
    /// Admission declined under the capacity ledger WITHOUT mutating
    /// readiness. The caller retries or blocks on capacity.
    Backpressured,
}
```

Landed substrate, UNWIRED: the live submission entry points
(`submit_request` / `submit_batch` / `submit_batch_atomic`) still return
their existing `CompletionHandle` / `BatchHandle` shapes, not
`SubmissionResult`. Routing those entry points through `SubmissionResult`
is a future sub-block.

## CancellationToken substrate

`CancellationToken` (`cancellation.rs`, LANDED) is a cheap, clonable,
thread-safe one-shot latch — a transparent `Arc<AtomicBool>`. `clone()`
is a refcount bump, `cancel()` a single `Release` store, `is_cancelled()`
a single `Acquire` load; all clones share one flag and `cancel()` is
idempotent. It is the substrate the un-landed DAG design uses for
per-node cancellation propagation
(`CacheNodeDagNode.cancellation_token`), but on the current tree it is
UNWIRED — no submission path or work node carries one yet.

## KeyedJob, DedupKey, and `CacheNodeDagNode` lifecycle

> **Not yet implemented — cache-runtime DAG design.** The `KeyedJob` /
> `DedupKey` / `CacheNodeDagNode` / `CacheNodeDag` / `submit_dag` types
> and the whole lifecycle here are the un-landed design target from
> `docs/arch/cache-runtime-overhaul-plan.md`; none are on the current
> tree. Live submission surface: `Scheduler::submit_request` /
> `submit_batch` (returning a `BatchHandle`) over the live `TaskKind` set
> `Source` / `Analysis` / `Artifact`, dispatched onto the scheduler-owned
> `cpu_pool` via `cpu_pool.spawn(...)` (see *Dual pool ownership*). Types
> and steps below describe the intended shape.
>
> **Dedupe-identity reconciliation:** the LANDED dedupe authority is
> `crate::dag::WorkNodeIdentity` (the `DedupeHook::probe` key — see
> *Generic dedupe-hook surface*). The illustrative `DedupKey` struct
> below is an earlier draft shape; when the DAG surface lands its dedupe
> key MUST be `WorkNodeIdentity` (or a thin derivation of it), NOT a
> parallel key type. There is one dedupe-identity source of truth.

`KeyedJob` is the submission identity. `CacheNodeDagNode` is the
ready-queue envelope the driver dispatches. The inbox-level enum
`driver::Submission` is a separate type owning its own discriminator
variants.

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

`KeyedJob` carries NO `task` / `task_kind` field. The task discriminator
lives on `CacheNodeDagNode.task_kind` only — one source of truth.

> **Not yet implemented — cache-runtime DAG design (Block 7).** The
> `submit_dag` / `CacheNodeDag` / `SchedulerCpuPool` / per-task
> `cpu_concurrency_semaphore` lifecycle below is the Block 7 design
> target from `docs/arch/cache-runtime-overhaul-plan.md`; NOT on the
> current tree. On the current tree the scheduler exposes `submit_request`
> (no `submit_dag`), the live `TaskKind` set is `Source` / `Analysis` /
> `Artifact`, and CPU stage work dispatches via the scheduler-owned
> `cpu_pool.spawn(...)` (see *Dual pool ownership* for the authoritative
> live pool model). Steps below describe the intended DAG flow once Block
> 7 lands.

Lifecycle (Block 7 design target):

1. **Caller-side dedupe.** Cache-runtime callers consult their in-flight
   table FIRST. A matching flight short-circuits — no scheduler
   submission happens.
2. **Submit.** Caller invokes `Scheduler::submit_request(req)` or
   `Scheduler::submit_dag(dag)` (optionally passing a `DedupeHook`).
   `submit_request` lands a `driver::Submission::NewRequest` on the
   inbox. `submit_dag` constructs a `CacheNodeDag` and pushes its ready
   nodes into the bounded ready queue as upstream gates fire.
3. **Scheduler-side dedupe probe.** Driver computes `dedup_key_for(req)`
   and consults `pending_requests` (the scheduler's own per-process
   inbox-level dedupe). A duplicate `DedupKey` attaches the caller's
   `CompletionSender<RequestResult>` as a joiner on the existing flight;
   no new job enqueued.
4. **Admission.** A non-dedup submission is admitted to the priority
   ready queue
   (`Arc<crossbeam_queue::ArrayQueue<Arc<CacheNodeDagNode>>>` — the inner
   `Arc` is required because `CacheNodeDagNode` is not `Clone`: its
   `CacheNodeCompletionSender` wraps a single-use
   `tokio::sync::oneshot::Sender`, so the same node lives on both the
   ready queue and `DagState.nodes` only via `Arc`-sharing), subject to
   the bounded-admission policy below. Per-call CPU concurrency is
   enforced by the worker dispatch site (per-task
   `cpu_concurrency_semaphore.acquire()`), not by admission.
5. **Execution.** Driver pops a ready node and dispatches via `TaskKind`
   routing:
   - `Load` → `IoPool::submit`;
   - `Parse` / `CacheNode` / CPU `Analysis` / CPU `Artifact` →
     `SchedulerCpuPool::submit`.
6. **Completion.** `pending_requests` cleared; every joiner receives the
   result through their attached `CompletionSender<RequestResult>`; DAG
   dependents are re-evaluated for readiness. The worker's per-task
   `CpuConcurrencyPermit` drops via RAII immediately after the task body
   returns, releasing the semaphore counter and notifying one waiter.

## Dual pool ownership

Two distinct `rayon::ThreadPool`s cooperate so the batch-orchestration
outer wait and the scheduler's CPU stage executor cannot deadlock on the
same workers. Owned by DIFFERENT layers — the split is the
deadlock-isolation invariant.

- **Scheduler stage pool (`cpu_pool`)** — owned BY the scheduler, built
  internally in `Scheduler::with_executor` / `new_sync_with_executor`
  from `SchedulerConfig::cpu_threads`. The ONLY pool for CPU stage
  execution: the driver dispatches the live `TaskKind::Source` CPU step
  (the parse folded into `Source`) plus `TaskKind::Analysis` and
  `TaskKind::Artifact` onto it via `cpu_pool.spawn(...)`. Workers register
  `CallerKind::CpuWorker` so `wait_or_drive` routes them to the
  cooperative-pump branch. The scheduler also owns the bounded `io_pool`
  (`SchedulerConfig::io_threads`) for the pure-I/O step of
  `TaskKind::Source` (reading bytes off disk).
- **Coordinator pool (`HostCpuPool`)** —
  `crates/verter_scheduler/src/host_cpu_pool.rs`. Constructed once at
  startup by the external host/runtime layer via
  `verter_scheduler::HostCpuPool::new(num_threads)` and owned THERE, as a
  sibling of the `Scheduler` — NOT passed into the scheduler and NOT a
  field on it. Shared by the outer batch coordinator of EVERY host batch
  API (batch component-meta, batch SFC compile, and any future host batch
  fan-out) for its synchronous wait points. Its workers register
  `CallerKind::External` (8 MiB stacks), so they PARK in `wait_or_drive`
  rather than inline-executing scheduler CPU tasks, and the driver's
  inline-execute branch excludes `External` — coordinator-pool workers
  therefore NEVER run scheduler CPU stage work (`TaskKind::Source` /
  `Analysis` / `Artifact`).

The scheduler constructor takes only `(config, source_loader[,
executor])` — NO pool parameter. The scheduler builds and owns its
`cpu_pool` + `io_pool`; the coordinator pool lives entirely in the
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
runtime layer, not in this crate).** Every host/runtime batch API (batch
component-meta, batch SFC compile, and any future batch fan-out) routes
its outer wait through ONE coordinator primitive owned by the external
layer, parameterised by a small per-client batch policy/context. That
primitive — not the scheduler — owns: coordinator-pool `install`; the
empty / single-item fast path; deterministic per-input ordering; a
generic per-item panic boundary (catches a panicking item and hands it to
the client's policy for domain conversion, so one item never aborts the
batch); per-batch submission accounting (when the policy carries a
scheduler handle); a per-batch tracing span; and the non-reentrant policy
below. Each client supplies only its item work and its domain
panic→result conversion. The primitive does NOT own
cancellation/shutdown — the scheduler exposes no batch-cancellation
facility today, so a batch runs to completion. The scheduler crate
exposes NO outer-fan-out API and performs NO `par_iter().install(...)`
outer wait on its `cpu_pool`; a batch's per-batch submission accounting is
a pool-free counter bump (`Scheduler::account_batch_submission`), which
the coordinator invokes once per non-empty batch.

**Non-reentrant host-batch contract.** A batch item closure may call
scalar scheduler operations, but a nested batch fan-out reached from
inside an item closure must NOT issue a fresh coordinator-pool install.
The external primitive detects re-entrancy (a per-thread marker scoped
around each item's execution) and runs the nested fan-out INLINE /
sequentially on the current coordinator worker. Stacking a second outer
wait on the same finite coordinator pool would reintroduce the starvation
class one level up.

**Deadlock-free property + new invariant.** The two pools are distinct
and owned by different layers: **no worker waits for a job in its OWN
pool.** A coordinator-pool worker may block on scheduler stage work
without deadlock because the scheduler's `cpu_pool` has its own
independently-proceeding worker set; a `cpu_pool` worker running
`TaskKind::Source` stage work is not a coordinator worker and does not
gate the outer coordinator's wait. The invariant in full:

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

Current state: batch fan-out has no per-call `threads` option — the host
coordinator pool's worker count is sized once at host construction (from
the host's CPU-thread config) and reused across every batch call, and the
scheduler's stage `cpu_pool` runs at its configured concurrency.

The `CpuConcurrencySemaphore` / `CpuConcurrencyPermit` TYPES are LANDED
(`crates/verter_scheduler/src/cpu_concurrency.rs`) and tested in
isolation, but UNWIRED — no submission path or DAG node consumes a
semaphore handle yet.

> **Not yet implemented.** The `Scheduler::cpu_concurrency_semaphore(n)`
> constructor method and per-call concurrency capping on
> `SchedulerCpuPool` admissions (the `CpuConcurrencySemaphore` handle
> propagated through `CacheNodeDagNode.cpu_concurrency_semaphore`) are
> part of the un-landed cache-runtime DAG design target in
> `docs/arch/cache-runtime-overhaul-plan.md`. The rest of this section
> describes that intended design. Once it lands, callers attach the
> handle to every `CacheNodeDagNode.cpu_concurrency_semaphore` in the
> batch DAG:

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

`CpuConcurrencySemaphore` (LANDED) is a hand-rolled counting primitive.
Substrate: `parking_lot::Mutex<usize>` (the free-permit count) +
`parking_lot::Condvar` — the only synchronisation primitives
`parking_lot 0.12` exports (`parking_lot::Semaphore` does NOT exist in
that version; absence pinned by `tests/cases/no_parking_lot_semaphore.rs`).
`new(capacity)` PANICS on `capacity == 0` (a release-active assert: a
zero-permit semaphore would deadlock every `acquire`; the cap is
configured once at construction so the check is off the hot path).
`acquire()` BLOCKS in a predicate-rechecking `while *available == 0` loop
until a permit is free, then decrements and returns the RAII
`CpuConcurrencyPermit` (`#[must_use]`, non-`Clone` — one permit is exactly
one held slot). `Drop` increments the count and `notify_one`s a single
waiter, on BOTH the normal path AND stack-unwind on panic, so a panicking
holder still frees its slot. The `Mutex<usize>` count is the single
source of truth for available permits. Guards:
`tests/cases/cpu_concurrency_semaphore.rs` pins the capacity cap (deterministic
channel-handshake blocking proof), RAII normal-drop release, and
panic-unwind release; the `cpu_concurrency` module is
`#[cfg(not(target_arch = "wasm32"))]` (the limiter caps the native-only
scheduler CPU pool — wasm runs the scheduler inline), so the test file
compiles native-only.

Propagation model: every `CacheNodeDagNode` carries
`cpu_concurrency_semaphore: Option<Arc<CpuConcurrencySemaphore>>` — the
SEMAPHORE HANDLE, NOT a pre-acquired permit. The worker dispatch site
calls `sem.acquire()` per task immediately before the executor runs the
body; the permit drops on task completion. Cloning the
`Arc<CpuConcurrencySemaphore>` across N DAG nodes does NOT pre-acquire N
permits — only `acquire()` consumes a permit. This is the only shape that
enforces "max `capacity` concurrent CPU tasks" across the DAG. A design
propagating a shared pre-acquired `Arc<CpuConcurrencyPermit>` would
acquire ONE permit at submission and let N>capacity tasks run
concurrently.

## TaskKind routing

> **Current state.** The live `TaskKind` set is `Source` / `Analysis` /
> `Artifact`. CPU stage work (`Analysis` / `Artifact`, and the parse step
> folded into `Source`) dispatches onto the scheduler-owned `cpu_pool` via
> `cpu_pool.spawn(...)`; `Load`-style I/O runs on the `io_pool`. See *Dual
> pool ownership* for the authoritative live pool model.
>
> **Not yet implemented — cache-runtime DAG design (Block 7).** The
> expanded `TaskKind` shape below (`Load` / `Parse` / `CacheNode`
> variants) and the `SchedulerCpuPool::submit` dispatch form are the
> Block 7 design target from `docs/arch/cache-runtime-overhaul-plan.md`;
> NOT on the current tree. Wherever a routing bullet below says
> `SchedulerCpuPool::submit`, the current tree dispatches the equivalent
> stage work onto `cpu_pool` via `cpu_pool.spawn(...)`. The bullets
> describe the intended Block 7 routing.

The scheduler routes (Block 7 design target):

- `TaskKind::Load { canonical }` → I/O pool (pure I/O — reads bytes off
  disk; no executor dispatch, the source loader drives the I/O directly).
- `TaskKind::Parse { canonical, source, file_language }` → stage CPU pool
  (pure CPU; payload carries the resolved `verter_language::FileLanguage`
  row so `execute_source` dispatches without re-classifying the path).
- `TaskKind::Analysis { canonical, source_snapshot }` →
  `SchedulerCpuPool::submit`. Dispatch destructures `canonical` off the
  payload and passes the snapshot reference to `execute_analysis`. The
  substrate `SourceSnapshot` has no `canonical_id()` accessor —
  `canonical` lives on the variant.
- `TaskKind::Artifact { canonical, source_snapshot, analysis_snapshot, profile_hash }`
  → `SchedulerCpuPool::submit`. Same payload-bearing shape.
- `TaskKind::CacheNode { cache_id: SchedulerCacheId, key_hash: u64 }` →
  `SchedulerCpuPool::submit`. The worker dispatches through
  `execute_cache_node(&node, &ctx) -> CacheNodeOutcome` (direct return,
  NOT `Result`-wrapped). `SchedulerCacheId` is the scheduler-local OPAQUE
  NEWTYPE `pub struct SchedulerCacheId(pub u64)` defined in
  `crates/verter_scheduler/src/cache_id.rs` (`Clone, Copy, Debug, Eq,
  Hash, Ord`). Deliberately NOT an enum — an enum would leak session
  cache-family meaning into the scheduler and create a second source of
  truth for cache identity. The scheduler stays domain-agnostic: the
  opaque `u64` is the discriminator on `WorkNodeIdentity::CacheNode`, and
  the session owns its interpretation. No `dag.rs` re-export shim for the
  type — it lives in `cache_id.rs` and is re-exported from the crate root
  only.

`TaskKind` is no longer `Copy` — payload-bearing variants carry
`Arc<str>` / `Arc<SourceSnapshot>` etc. Every existing `Copy` call site
(e.g. `supersede_old_generations` at `scheduler.rs:388`) becomes an `Arc`
clone. The discriminating test `task_kind_clone_is_cheap_arc_clone` pins
the clone cost at < 100ns p99.

Under the Block 7 design target, `TaskKind::Source` (which on the current
tree combines load + parse, with the I/O step on `io_pool` and the parse
step folded onto `cpu_pool`) is split: the source loader synthesises a
`Load → Parse` DAG edge. **On the current tree `TaskKind::Source` is the
live first stage and is NOT split or retired** — `Load` / `Parse` are not
separate variants yet. `SchedulerJobKind` (the existing non-staged
component-meta batch enum at `stage.rs:19`) is **retained** unchanged —
it discriminates `ComponentMeta { canonical_id }`. The scheduler does NOT
own the batch fan-out for it: the external host/runtime layer maps these
job items and fans them out through its own batch-coordination primitive
(see *Dual pool ownership*), calling `Scheduler::account_batch_submission`
once per non-empty batch for the O(1) submission accounting. The Block 7
`TaskKind::CacheNode` variant lives alongside it on the new ready-queue
envelope.

### StageExecutor dispatch surface

> **Not yet implemented — cache-runtime DAG design (Block 7).** The
> five-method dispatch surface, the `CacheNodeDispatchCtx` /
> `execute_cache_node` machinery, and the `Parse` / `CacheNode` / `Load`
> rows below are the Block 7 design target from
> `docs/arch/cache-runtime-overhaul-plan.md`; NOT on the current tree. On
> the current tree the `StageExecutor` dispatches the live
> `TaskKind::Source` / `Analysis` / `Artifact` stages. The surface below
> describes the intended Block 7 dispatch.

The `StageExecutor` trait exposes five dispatch methods, one per
`TaskKind` variant. Workers route through `TaskKind` at dispatch time; no
bare `executor.execute(node)` method.

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
BEFORE the `match` and passes a non-owning borrow to `execute_cache_node`.
The default body returns `CacheNodeOutcome::stub()`; the host overrides to
drive the cache-runtime artifact / query node trait surface. The other
three CPU dispatch methods return their stage-specific result, and the
worker maps each into the unified `CacheNodeOutcome` via the
`CacheNodeOutcome::from_source` / `from_analysis` / `from_artifact`
adapters before writing it on `node.completion`.

## DAG submission semantics

> **Not yet implemented — cache-runtime DAG design (Block 7).** The
> `submit_dag` / `CacheNodeDag` / `submit_batch`-as-DAG-shim surface in
> this whole section is the Block 7 design target from
> `docs/arch/cache-runtime-overhaul-plan.md`; NOT on the current tree. On
> the current tree the scheduler exposes `submit_request` (no `submit_dag`
> and no `CacheNodeDag` envelope), and the live `TaskKind` set is
> `Source` / `Analysis` / `Artifact` dispatched onto the scheduler-owned
> `cpu_pool` via `cpu_pool.spawn(...)` (see *Dual pool ownership* for the
> authoritative live pool model). The DAG submission contract below
> describes the intended flow once Block 7 lands.

The DAG API (Block 7 design target) is one method, one type, one
signature, carrying every field the driver requires to dispatch each node
as an executable unit of work:

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

The nine-field `CacheNodeDagNode` envelope is complete: the `task_kind`
discriminator lives on the node only (NOT on `KeyedJob`). No node enters
the ready queue without all nine fields populated; the driver does NOT
enrich nodes after submission. Guard:
`cache_node_dag_carries_required_fields_for_executable_dispatch`.

DAG contract:

- **Dependency gating.** A downstream node is not admitted to the ready
  queue until ALL of its upstream nodes have completed per their
  `EdgeGate` policy.
- **Priority inheritance.** Effective priority is
  `max(node_priority, max(root_priority for every reachable root))`.
- **Cancellation propagation.** Dropping a `DagHandle` triggers
  `CancellationToken::cancel()` on every node not yet completed;
  cancellation propagates transitively through edges.
- **Bounded admission / backpressure.** The ready queue is bounded by
  `MAX_READY_QUEUE_DEPTH = 64` (`crates/verter_scheduler/src/queue.rs`).
  When full, additional submissions either block or return
  `SubmissionResult::Backpressure` per caller preference.
  `Scheduler::ready_queue_depth()` exposes the current bounded depth for
  observability only.
- **In-flight dedupe inside a DAG.** Two nodes in the same DAG sharing a
  `dedup_key` collapse via scheduler-side `pending_requests`. Cross-DAG
  dedupe uses the consumer-side in-flight table via `DedupeHook::probe`
  BEFORE submission.

Under Block 7, `submit_batch(reqs: Vec<Request>)` becomes a thin shim
constructing a no-edge `CacheNodeDag` and calling `submit_dag`. On the
current tree it loops over `submit_request` (see the surface table below).

## Scheduler surface (current → Block 7 planned)

The right column is the Block 7 cache-runtime design target from
`docs/arch/cache-runtime-overhaul-plan.md`; NOT on the current tree. The
left column is the live surface.

| Method | Current | Block 7 (planned) |
|---|---|---|
| `submit_request(req)` | inbox + per-request `CompletionHandle` | unchanged signature; gains optional `&dyn DedupeHook` arg |
| `submit_batch(reqs)` | loop over `submit_request` | thin shim over `submit_dag` (no-edge DAG) |
| `submit_dag(dag) -> DagHandle` | absent | NEW |
| `dedup_key_for(req) -> DedupKey` | absent | NEW |
| `cpu_concurrency_semaphore(n) -> Arc<CpuConcurrencySemaphore>` | absent | NEW |
| `ready_queue_depth() -> usize` | absent | NEW |
| `register_resolved_deps` | unchanged | unchanged |

## Pool routing rules

The live `TaskKind` set is `Source` / `Analysis` / `Artifact`. (The
expanded `Load` / `Parse` / `CacheNode` shape and the
`SchedulerCpuPool::submit` dispatch form are the demarcated Block 7 design
target — see *TaskKind routing* and *Scheduler surface*; NOT current
routing rules.)

- **`io_pool`** owns the pure-I/O step of `TaskKind::Source` (reading
  bytes off disk) and any other pure-I/O work. A parse closure on the I/O
  pool is a bug
  (`pool_isolation::source_parse_runs_on_cpu_pool_not_io_pool`).
- **Scheduler stage pool (`cpu_pool`)** owns the CPU stage work — the
  parse step folded into `TaskKind::Source`, plus `TaskKind::Analysis` and
  `TaskKind::Artifact` — dispatched via `cpu_pool.spawn(...)`. The
  scheduler builds and owns it internally from
  `SchedulerConfig::cpu_threads`; NOT passed into the constructor. The
  only pool the driver dispatches stage work onto.
- **Coordinator pool (`HostCpuPool`)** owns the outer batch coordinator's
  wait points for EVERY host batch API (batch component-meta, batch SFC
  compile, and any future host batch fan-out). The external host/runtime
  layer constructs it once at startup and OWNS it (a sibling of the
  `Scheduler`, never handed into the constructor). The scheduler does NOT
  reference it and NEVER dispatches tasks onto it; equally, no scheduler
  API installs an outer wait on the stage pool. The coordinator pool is
  reused across batch calls (sized once from the external layer's config).
  Guard (external layer): `two_back_to_back_compile_many_share_pool`.

## Test-support helpers (`feature = "test-support"`)

> **Not yet implemented — cache-runtime DAG design (Block 7).** The
> `feature = "test-support"` gate itself is live, but on the current tree
> it exposes only `host_cpu_pool_token` (see *Dual pool ownership*). The
> fixture catalogue below — `Scheduler::new_for_test`, `enqueue_analysis`,
> `last_dispatched_task`, `LastDispatchedTaskRecorder`, the `KeyedJob` /
> `CacheNodeDagNode` / `DedupKey` / `SchedulerCacheId` stubs, etc. — is
> the Block 7 design target from
> `docs/arch/cache-runtime-overhaul-plan.md` and is NOT on the current
> tree. The helpers below describe the intended Block 7 test surface.

The crate gates a small set of fixture helpers behind
`feature = "test-support"` so the production build never compiles them.
Integration tests under `crates/verter_scheduler/tests/` enable the
feature via
`verter_scheduler = { path = ".", features = ["test-support"] }` in their
`[dev-dependencies]`.

- `Scheduler::new_for_test() -> Arc<Self>` — single-thread pools +
  `LastDispatchedTaskRecorder` executor.
- `Scheduler::enqueue_analysis(&FileNode)` — routes through
  `submit_request` with `TargetStage::Analysis`. Drives to quiescence by
  polling
  `last_dispatched_task() -> Some((_, TaskKind::Analysis { canonical, .. })) if canonical == fixture_canonical`
  (matches specifically on `Analysis`, not on the upstream `Parse`
  dispatch the driver completes first).
- `Scheduler::last_dispatched_task() -> Option<(KeyedJob, TaskKind)>` —
  downcasts `Arc<dyn StageExecutor>` to `LastDispatchedTaskRecorder` via
  `as_any` and reads its internal cell.
- `LastDispatchedTaskRecorder` — records the last `(KeyedJob, TaskKind)`
  per dispatch.
- `NoopSourceLoader` — full `SourceLoader` impl (four methods: `load`,
  `exists`, `classify`, `realpath`).
- `FileNode::stub_with_canonical(&str)` — populates `canonical_id` from
  the argument; zero-state for other fields.
- `OpaqueRequestContext::test_stub()` — wraps a private no-op
  `RequestContextLike` impl.
- `KeyedJob::stub()`, `DedupKey::new_for_test()`,
  `CacheNodeDagNode::stub()`,
  `CacheNodeDispatchCtx::stub_with(&dedup_key, &cancellation)`,
  `SchedulerCacheId(0)` (the opaque newtype constructed directly — no
  `::Test` variant; `SchedulerCacheId` is
  `pub struct SchedulerCacheId(pub u64)`, not an enum).

See also:
- `.claude/skills/host-session/SKILL.md` — host-side ownership.
- `.claude/skills/type-cache-architecture/SKILL.md` — the substrate
  the scheduler dispatches into; defines `DedupeHook` consumers.
- `docs/arch/cache-runtime-overhaul-plan.md` — the plan that landed
  the cache-runtime + scheduler integration (Blocks 6, 7).
