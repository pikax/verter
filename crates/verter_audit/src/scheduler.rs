#![deny(missing_docs)]
//! Scheduler-side attribution carried on every audit record envelope.
//!
//! [`SchedulerAudit`] captures the worker thread, pool, queue depths,
//! and queue-dwell millisecond cost observed when the scheduler
//! dispatched the request's first stage. On WASM (no scheduler), the
//! envelope's `scheduler` field stays `None`.
//!
//! The substrate exposes a typed
//! [`crate::observer::AuditObserver::record_scheduler_dispatch`] hook so
//! the scheduler crate can publish dispatch facts through TLS without
//! reaching into `verter_session` for context. The session-side
//! `RequestContext` implements the trait and stores the supplied
//! [`SchedulerAudit`] on a once-only slot that the
//! `RequestAuditRecord` builder reads when it finalises a record.

use serde::{Deserialize, Serialize};

/// Scheduler-side attribution captured at first dispatch of an audited
/// request. Populated on native (where the scheduler runs); always
/// `None` on WASM at the envelope level.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct SchedulerAudit {
    /// String form of the worker thread id (e.g. `"ThreadId(7)"`)
    /// taken via `std::thread::current().id()` on the worker that
    /// dispatched the audited stage. Always non-empty on native.
    pub worker_thread_id: String,
    /// Which scheduler pool ran the dispatch.
    pub worker_pool: WorkerPool,
    /// Queue / inbox depths sampled at dispatch time.
    pub depths: SchedulerDepths,
    /// Time the entry sat in the priority queue between enqueue and
    /// dispatch, in milliseconds.
    pub queue_dwell_ms: f64,
    /// Number of dispatches observed for this audited request through
    /// the scheduler. Defaults to 1 for synchronous component-meta
    /// requests; only retry-driven jobs increment past 1.
    pub dispatch_count: u32,
}

/// Discriminator naming the scheduler pool that dispatched the
/// audited stage.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export_to = "audit.generated.ts")]
pub enum WorkerPool {
    /// CPU pool (Rayon-backed, parse / analysis / artifact stages).
    Cpu,
    /// I/O pool (file reads, source-stage dispatch handoff).
    Io,
}

/// Snapshot of scheduler-internal queue depths at dispatch time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export_to = "audit.generated.ts")]
pub struct SchedulerDepths {
    /// Number of pending submissions in the inbox channel.
    pub inbox: u32,
    /// Number of non-cancelled entries in the priority queue.
    pub queue: u32,
}
