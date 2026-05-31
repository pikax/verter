//! Async file scheduler for Verter.
//!
//! Files progress independently through Source → Analysis → Artifact stages.
//! Cross-file blocking is declarative — the scheduler manages wakeups.
//!
//! # Architecture
//!
//! - [`FileNode`](node::FileNode) — per-file state: ArcSwap snapshots, generation counter
//! - [`SchedulerDag`](dag::SchedulerDag) — the sole readiness/admission/reservation authority (admission, dedup, dependency gating, capacity reservation)
//! - [`CompletionHandle`](job::CompletionHandle) — request-scoped handle, resolves to exactly one terminal state
//! - [`Priority`](stage::Priority) — 4-tier scheduling with FIFO within tier and aging across tiers
//! - [`ReverseIndex`](edges::ReverseIndex) — concurrent reverse-dep index
//! - [`OverlayMap`](overlay::OverlayMap) — concurrent editor buffer storage
//! - [`SourceLoader`](source_loader::SourceLoader) — file loading trait (memory/disk)
//!
//! # Pump architecture
//!
//! The driver thread is the normal pump caller, but it is NOT the
//! sole dispatch authority. The [`SchedulerDag`](dag::SchedulerDag)
//! owns readiness, admission, and capacity reservation; the
//! [`Scheduler::pump_ready`](scheduler::Scheduler) primitive is a
//! cooperative entry that any thread holding a strong `Arc<Scheduler>`
//! may call.
//!
//! Worker threads that wait synchronously on a dependency they
//! transitively scheduled enter [`Scheduler::wait_or_drive`](scheduler::Scheduler::wait_or_drive),
//! which:
//!
//! - blocks on the condvar when the caller is a `Driver` or
//!   `External` thread with a live driver (the driver has the
//!   work),
//! - runs the cooperative pump otherwise — draining the inbox,
//!   dispatching ready jobs (inline-executing CPU-bound work when
//!   the caller is a CPU worker), and parking on `wait_timeout`
//!   between iterations.
//!
//! The cooperative-pump caller is filtered against the
//! per-thread active path so a worker never re-dispatches the
//! same identity it is parked on (same-path self-await detection
//! surfaces a typed `StageFailed { stage: "wait_or_drive" }`).
//!
//! # Lock discipline (cooperative pump architecture)
//!
//! - DAG lock NEVER held during stage execute, pool submit,
//!   CompletionHandle blocking wait, or `recv_timeout`.
//! - CompletionHandle methods (`try_get` / `wait` / `wait_timeout`)
//!   NEVER called under DAG lock.
//! - Inbox `try_recv` runs outside all locks.
//! - `SchedulerDag` is the SOLE readiness / admission / reservation
//!   authority. The driver loop and self-driving workers go
//!   through the same `pump_ready` / `wait_or_drive` code path;
//!   caller identity does not change routing.
//! - The single exception to "no DAG lock across submit" is the
//!   macro-cycle filter chokepoint (`filter_macro_cycle_deps` +
//!   `dag.submit`) which MUST run under one lock guard for TOCTOU
//!   atomicity — the region is non-blocking (<1µs typical).

#[cfg(not(target_arch = "wasm32"))]
pub mod audit_publish;
pub mod caller_kind;
pub mod dag;
pub mod driver;
pub mod edges;
pub mod executor;
pub mod invalidation;
pub mod job;
pub mod node;
pub mod overlay;
#[cfg(not(target_arch = "wasm32"))]
pub mod pool;
pub mod request_context;
pub mod scheduler;
pub mod source_loader;
pub mod stage;
