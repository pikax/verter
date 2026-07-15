//! Driver loop: admission, ordering, and dispatch.
//!
//! The driver owns all admission and ordering policy. Workers and callers
//! communicate via the [`SubmissionInbox`].

use crossbeam_channel::{Receiver, Sender};

use crate::job::{CompletionSender, RequestResult};
use crate::stage::{Priority, TargetStage, TaskKind};
use verter_language::FileLanguage;

/// One request inside a [`Submission::NewRequestBatch`].
///
/// Carries the same per-request fields a standalone
/// [`Submission::NewRequest`] does. The batch variant exists so the
/// driver can admit N requests under ONE DAG-lock acquisition; the
/// fields are identical to `NewRequest` minus the inbox-coalescing
/// bookkeeping (which is shared by the whole batch).
pub struct QueuedRequest {
    pub file_id: String,
    pub target: TargetStage,
    pub priority: Priority,
    pub source: Option<std::sync::Arc<str>>,
    pub file_language: Option<FileLanguage>,
    pub sender: CompletionSender<RequestResult>,
    /// Removal epoch at submission time. If a tombstone exists with a
    /// higher epoch, this submission predates the removal and is rejected.
    pub submitted_epoch: u64,
    /// Optional session-side request context. When present, the driver
    /// stores the winner's context on the dedup group and routes
    /// `on_dedup_joiner` callbacks when this request joins.
    pub request_context: Option<crate::request_context::OpaqueRequestContext>,
}

/// A submission to the scheduler inbox.
pub enum Submission {
    /// Wake the driver so it can observe shutdown/reset immediately.
    Wake,
    /// A new request from a caller.
    NewRequest {
        file_id: String,
        target: TargetStage,
        priority: Priority,
        source: Option<std::sync::Arc<str>>,
        file_language: Option<FileLanguage>,
        sender: CompletionSender<RequestResult>,
        /// Removal epoch at submission time. If a tombstone exists with a
        /// higher epoch, this submission predates the removal and is rejected.
        submitted_epoch: u64,
        /// Optional session-side request context. When present, the driver
        /// stores the winner's context on the dedup group and routes
        /// `on_dedup_joiner` callbacks when this request joins.
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    },
    /// An atomic batch of new requests. Drained as ONE inbox item by
    /// the driver, which admits every contained request under a SINGLE
    /// DAG-lock acquisition (generation bumps + supersede sweeps +
    /// waiter registration), so the pump can never observe the batch
    /// half-admitted. Dedup callbacks discovered during registration
    /// are collected and fired AFTER the DAG lock releases. The
    /// per-item dispatch (and any wait) still happens outside the lock,
    /// preserving the pump discipline.
    NewRequestBatch { requests: Vec<QueuedRequest> },
    /// A stage completed for a file. The driver advances the file's
    /// pipeline (admit Analysis after Source, admit Artifact after
    /// Analysis when dep gates clear) and propagates the completion
    /// onto the DAG so dependent waiter edges resolve.
    StageComplete {
        file_id: String,
        generation: u64,
        task_kind: TaskKind,
    },
}

/// Lock-free MPSC inbox: workers/callers produce, driver consumes.
pub struct SubmissionInbox {
    pub sender: Sender<Submission>,
    pub receiver: Receiver<Submission>,
}

impl SubmissionInbox {
    pub fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self { sender, receiver }
    }
}

impl Default for SubmissionInbox {
    fn default() -> Self {
        Self::new()
    }
}
