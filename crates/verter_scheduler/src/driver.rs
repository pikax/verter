//! Driver loop: admission, ordering, and dispatch.
//!
//! The driver owns all admission and ordering policy. Workers and callers
//! communicate via the [`SubmissionInbox`].

use crossbeam_channel::{Receiver, Sender};

use crate::job::{CompletionSender, RequestResult};
use crate::source_loader::FileKind;
use crate::stage::{Priority, TargetStage, TaskKind};

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
        file_kind: Option<FileKind>,
        sender: CompletionSender<RequestResult>,
        /// Removal epoch at submission time. If a tombstone exists with a
        /// higher epoch, this submission predates the removal and is rejected.
        submitted_epoch: u64,
        /// Optional session-side request context. When present, the driver
        /// stores the winner's context on the dedup group and routes
        /// `on_dedup_joiner` callbacks when this request joins.
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    },
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
