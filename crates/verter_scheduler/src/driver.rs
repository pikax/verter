//! Driver loop: admission, ordering, and dispatch.
//!
//! The driver owns all admission and ordering policy. Workers and callers
//! communicate via the [`SubmissionInbox`].

use crossbeam_channel::{Receiver, Sender};

use crate::job::{CompletionSender, RequestResult};
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
        sender: CompletionSender<RequestResult>,
        /// Removal epoch at submission time. If a tombstone exists with a
        /// higher epoch, this submission predates the removal and is rejected.
        submitted_epoch: u64,
    },
    /// A stage completed for a file.
    StageComplete {
        file_id: String,
        generation: u64,
        task_kind: TaskKind,
    },
    /// A blocker resolved — check if dependents can proceed.
    BlockerResolved {
        file_id: String,
        generation: u64,
        completed_stage: TaskKind,
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
