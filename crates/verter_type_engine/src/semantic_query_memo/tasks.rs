//! Execution tasks, as the semantic store uses them.
//!
//! The task model — generation-qualified task ids, the wait-for graph and
//! its cycle refusal, producer ownership by task rather than by thread, and
//! the synchronous entry adapter — is the shared scheduler substrate
//! ([`verter_execution::tasks`]). What is semantic is the producer: a
//! semantic producer is one prepared query identity, and same-path
//! recursion is a property of the task, not of a thread — a demand is
//! same-path exactly when the demanding task already has a producer open
//! for the same full key.

pub(super) use verter_execution::tasks::{
    ExecutionScope, ExecutionTask, OpenProducer, TaskId, TaskRegistry, WaitCycle,
};

use crate::semantic_query::SemanticQueryKey;

use super::prepared::PreparedKeyHandle;

/// The semantic producers of one task.
pub(super) trait SemanticProducers {
    /// Open this task's producer for `key` until the registration drops.
    fn open_producer(&self, key: PreparedKeyHandle) -> OpenProducer;

    /// Whether this task has a producer open for exactly `key`.
    fn produces(&self, key: &PreparedKeyHandle) -> bool;

    /// Whether this task has a producer open for exactly `key`, whose
    /// prepared hash is `key_hash`.
    fn produces_key(&self, key: &SemanticQueryKey, key_hash: u64) -> bool;
}

impl SemanticProducers for ExecutionTask {
    fn open_producer(&self, key: PreparedKeyHandle) -> OpenProducer {
        self.open(key.as_task_producer())
    }

    fn produces(&self, key: &PreparedKeyHandle) -> bool {
        self.produces_identity(key.as_task_producer_ref())
    }

    fn produces_key(&self, key: &SemanticQueryKey, key_hash: u64) -> bool {
        self.produces_matching(key_hash, |producer| {
            PreparedKeyHandle::producer_matches_key(producer, key, key_hash)
        })
    }
}
