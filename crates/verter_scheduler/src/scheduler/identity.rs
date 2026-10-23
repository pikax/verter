//! Work identity — how a request, task or scoped flight becomes a DAG identity.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

/// Extract a canonical-id string from a `WorkNodeIdentity` so a
/// cooperative-pump self-await report can name the file the caller
/// was waiting on. `CacheNode` variants return an empty string —
/// they have no file canonical.
pub(super) fn identity_canonical(identity: &crate::dag::WorkNodeIdentity) -> String {
    match identity {
        crate::dag::WorkNodeIdentity::FileStage { canonical, .. } => canonical.to_string(),
        crate::dag::WorkNodeIdentity::Artifact { canonical, .. } => canonical.to_string(),
        crate::dag::WorkNodeIdentity::CacheNode { .. } => String::new(),
    }
}

impl Scheduler {
    /// Is `flight` still the registry's CURRENT incarnation for `identity`?
    ///
    /// The scoped-cache rendezvous is per-incarnation but the DAG node it
    /// drives is keyed by [`WorkNodeIdentity`] alone. A retired incarnation
    /// that tore the node down by identity would therefore cancel the node a
    /// LATER incarnation had already been admitted onto: the successor's
    /// aggregate token latches cancelled, its `by_identity` entry disappears,
    /// and — when the cancel lands before the successor's dispatch — no
    /// `mark_dispatched` ever arrives, so every caller parked in
    /// [`ScopedCacheNodeHandoff::poll`] waits forever. Every teardown that
    /// can run on a superseded flight gates its DAG cancel on this predicate.
    ///
    /// The caller holds `scoped_cache_gate`, which serializes incarnation
    /// replacement against teardown. The registry `Ref` is dropped before
    /// return so no shard guard is held across the subsequent `dag.lock()`.
    pub(super) fn scoped_flight_is_current(
        &self,
        identity: &WorkNodeIdentity,
        flight: &Arc<ScopedCacheFlight>,
    ) -> bool {
        self.scoped_cache_flights
            .get(identity)
            .is_some_and(|current| Arc::ptr_eq(current.value(), flight))
    }

    /// Remove only `flight`'s registry incarnation. Caller holds
    /// `scoped_cache_gate`, which serializes replacement with stale inbox work.
    pub(super) fn remove_scoped_flight_locked(
        &self,
        identity: &WorkNodeIdentity,
        flight: &Arc<ScopedCacheFlight>,
    ) {
        if let dashmap::mapref::entry::Entry::Occupied(entry) =
            self.scoped_cache_flights.entry(identity.clone())
        {
            if Arc::ptr_eq(entry.get(), flight) {
                entry.remove();
            }
        }
    }

    pub(super) fn handle_scoped_cache_node_submission(
        &self,
        identity: WorkNodeIdentity,
        priority: Priority,
        flight: Arc<ScopedCacheFlight>,
        request_context: Option<verter_execution::request_context::OpaqueRequestContext>,
    ) {
        let _wakes = self.dag.defer_wakes();
        let _gate = self.scoped_cache_gate.lock();
        let is_current = self
            .scoped_cache_flights
            .get(&identity)
            .is_some_and(|current| Arc::ptr_eq(current.value(), &flight));
        if !is_current || flight.terminal().is_some() {
            return;
        }
        let aggregate = {
            let mut dag = self.dag.lock();
            dag.submit_cache(identity.clone(), priority, request_context);
            dag.set_execution_task(&identity, flight.task.clone());
            dag.cancellation_for(&identity)
                .expect("fresh or deduplicated scoped cache node must own a token")
        };
        if !flight.attach_aggregate(aggregate) {
            let _ = self.dag.lock().cancel(&identity);
            self.remove_scoped_flight_locked(&identity, &flight);
        }
    }

    /// `true` when `node` is STILL the authority for `file_id` at
    /// `generation` — same incarnation, same generation, and a
    /// generation-coherent committed Source snapshot.
    ///
    /// MUST be evaluated under the caller-held `dag.lock()`. It is the
    /// validation half of the single linearization point for a stage
    /// completion's publish: the supersede sweep is purely
    /// backward-looking, so it can never retire an identity admitted
    /// AFTER it ran. Publishing is therefore only safe while the lock
    /// that a concurrent `invalidate()` / `close_file()` / language
    /// re-home must also hold is held here.
    ///
    /// The incarnation check is not redundant with the generation
    /// check: a replacement publishes a FRESH `FileNode` for the same
    /// canonical, and two node objects can sit at the SAME generation,
    /// so only the incarnation id separates the node the job actually
    /// ran against from its replacement.
    ///
    /// `dispatched_incarnation` MUST come from the completion message —
    /// i.e. from the node the work ran against. Re-deriving it here by
    /// map lookup would compare the live node with itself and pass
    /// vacuously, which is exactly the hole this parameter closes.
    ///
    /// Lock order: the caller already holds `dag.lock()`, so the
    /// transient `nodes` shard read below is the canonical DAG-first
    /// order. The `Ref` is dropped before returning — never held across
    /// a `dag.lock()` acquisition.
    pub(super) fn stage_completion_is_current(
        &self,
        file_id: &str,
        dispatched_incarnation: u64,
        generation: u64,
    ) -> bool {
        match self.nodes.get(file_id) {
            Some(live) => {
                live.incarnation_id() == dispatched_incarnation
                    && live.generation() == generation
                    && live.current_source().is_some()
            }
            None => false,
        }
    }

    /// The DAG identity a completing stage was dispatched under.
    pub(super) fn dispatched_identity_for(
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        task_kind: &TaskKind,
    ) -> Option<WorkNodeIdentity> {
        match task_kind {
            TaskKind::Load => Some(WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Source,
            }),
            TaskKind::Analysis => Some(WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Analysis,
            }),
            // Artifact completions carry a content hash the caller does
            // not have here, and `Parse` / `CacheNode` never reach a
            // file-stage completion, so no identity is released for
            // them on the refusal path.
            TaskKind::Artifact { .. } | TaskKind::Parse | TaskKind::CacheNode { .. } => None,
        }
    }

    /// Construct the [`WorkNodeIdentity`] for `(canonical, generation,
    /// task_kind)` so that a failure / panic terminal path can address
    /// the matching DAG node. The mapping is the inverse of [`admit_work`].
    pub(super) fn dag_identity_for_task(
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        task_kind: &TaskKind,
    ) -> WorkNodeIdentity {
        match task_kind {
            // The live `FileStage{Source}` node maps to the `Load` label.
            TaskKind::Load => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Source,
            },
            TaskKind::Analysis => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Analysis,
            },
            TaskKind::Artifact { profile_hash } => WorkNodeIdentity::Artifact {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                profile_hash: profile_hash_to_bytes(*profile_hash),
                content_hash: [0u8; 16],
            },
            // Failure/terminalization addressing covers the file-stage tasks
            // only. `Parse` shares the source stage's identity (it never
            // terminalizes on its own), and `CacheNode` completion/release is
            // owned by the cache dispatch path, not this inverse map.
            TaskKind::Parse | TaskKind::CacheNode { .. } => unreachable!(
                "dag_identity_for_task addresses file-stage nodes (Load/Analysis/Artifact) \
                 for terminalization; Parse and CacheNode are not addressed here"
            ),
        }
    }
}
