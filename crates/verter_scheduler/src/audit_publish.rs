//! Scheduler-internal mirror of the audit substrate's scheduler-side
//! attribution shapes. Producers populate the local `*Tag` /
//! `*Snapshot` types and convert into the canonical
//! [`verter_audit::SchedulerAudit`] family at publish time. Keeping
//! the scheduler-internal call sites typed against local mirrors
//! lets the leaf substrate evolve its own DTOs without churn deep in
//! the dispatch loop.
#![cfg(not(target_arch = "wasm32"))]

/// Scheduler-internal mirror of [`verter_audit::WorkerPool`].
#[derive(Debug, Clone, Copy)]
pub enum WorkerPoolTag {
    /// CPU pool.
    Cpu,
    /// I/O pool.
    Io,
}

impl From<WorkerPoolTag> for verter_audit::WorkerPool {
    fn from(tag: WorkerPoolTag) -> Self {
        match tag {
            WorkerPoolTag::Cpu => verter_audit::WorkerPool::Cpu,
            WorkerPoolTag::Io => verter_audit::WorkerPool::Io,
        }
    }
}

/// Scheduler-internal mirror of [`verter_audit::SchedulerDepths`].
#[derive(Debug, Clone, Copy)]
pub struct SchedulerDepthsSnapshot {
    /// Inbox channel pending submissions at dispatch time.
    pub inbox: u32,
    /// Priority queue active entries before the dispatched entry
    /// was dequeued.
    pub queue: u32,
}

impl From<SchedulerDepthsSnapshot> for verter_audit::SchedulerDepths {
    fn from(snap: SchedulerDepthsSnapshot) -> Self {
        verter_audit::SchedulerDepths {
            inbox: snap.inbox,
            queue: snap.queue,
        }
    }
}
