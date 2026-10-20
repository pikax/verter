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

use crate::semantic_query::checker_policy::TupleSplicePosition;
use crate::semantic_query::SemanticQueryKey;

use super::prepared::PreparedKeyHandle;

/// The semantic producers of one task. A producer is one prepared key
/// demanded at one checking site: the same key at the other site is a
/// different computation, so it neither shares the producer nor closes
/// same-path recursion through it.
pub(super) trait SemanticProducers {
    /// Open this task's producer for `key` at `site` until the registration
    /// drops.
    fn open_producer(&self, key: PreparedKeyHandle, site: TupleSplicePosition) -> OpenProducer;

    /// Whether this task has a producer open for exactly `key` at `site`.
    fn produces(&self, key: &PreparedKeyHandle, site: TupleSplicePosition) -> bool;

    /// Whether this task has a producer open for exactly `key` at `site`,
    /// whose prepared hash is `key_hash`.
    fn produces_key(
        &self,
        key: &SemanticQueryKey,
        key_hash: u64,
        site: TupleSplicePosition,
    ) -> bool;
}

impl SemanticProducers for ExecutionTask {
    fn open_producer(&self, key: PreparedKeyHandle, site: TupleSplicePosition) -> OpenProducer {
        self.open(key.as_site_producer(site))
    }

    fn produces(&self, key: &PreparedKeyHandle, site: TupleSplicePosition) -> bool {
        self.produces_key(key.key(), key.cached_hash(), site)
    }

    fn produces_key(
        &self,
        key: &SemanticQueryKey,
        key_hash: u64,
        site: TupleSplicePosition,
    ) -> bool {
        self.produces_matching(
            PreparedKeyHandle::site_producer_hash(key_hash, site),
            |producer| PreparedKeyHandle::producer_matches_key(producer, key, key_hash, site),
        )
    }
}
