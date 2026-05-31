//! Terminal-failure store + dependency-failure fan-out — child
//! module of `dag`.
//!
//! Owns the persistent [`SchedulerDag::terminal_dep_failures`]
//! store, the per-stage fan-out helpers (`fanout_source_*` and
//! `fanout_analysis_*`), and the `attach_failed_dep` helper used by
//! admission paths. The DAG owns the underlying storage: every
//! read/write happens under the DAG mutex so the terminalize-time
//! producer-side writes and the admission-time consumer-side reads
//! cannot interleave with each other.
//!
//! The storage itself stays on [`SchedulerDag`] (see
//! `terminal_dep_failures`) so the existing race-safety contract is
//! preserved structurally; this module owns the typed API that
//! wraps the underlying `FxHashMap` plus the waiter-graph
//! fan-out helpers that record the failure markers downstream.

use std::sync::Arc;

use crate::job::SchedulerError;

use super::{DepKey, FileStageKey, SchedulerDag, SubmissionToken, WorkNodeIdentity};

/// Persistent record of a terminally-failed prerequisite. Recorded
/// on a waiter's `failed_blocker_deps` (and in the DAG-level
/// `terminal_dep_failures` store) so a downstream consumer can
/// short-circuit with a typed
/// [`crate::job::SchedulerError::DependencyFailed`] that names the
/// failed [`DepKey`] AND carries the producer's terminal cause.
///
/// The cause is the [`SchedulerError`] that terminalized the
/// producer (e.g. `FileNotFound` for a missing source file,
/// `StageFailed` for an executor error). Consumers can disambiguate
/// failure kinds (FileNotFound vs StageFailed) instead of seeing
/// only a structural `DependencyFailed` envelope. The dep key is
/// the `DepKey` that was released from the waiter's
/// `deps_remaining` as a side effect of the producer's
/// terminalization.
#[derive(Clone, Debug)]
pub struct FailedDepRecord {
    /// Identity of the prerequisite whose producer failed
    /// terminally.
    pub dep_key: DepKey,
    /// Terminal cause emitted by the producer's stage executor (or
    /// the loader's FileNotFound failure). Carried verbatim so the
    /// consumer can render or programmatically inspect it.
    pub cause: SchedulerError,
}

impl SchedulerDag {
    /// Drop the `DepKey::FileStage { canonical, generation, stage:
    /// Analysis }` waiter index entry as-if the Analysis identity
    /// had been cancelled, returning the list of stranded waiter
    /// tokens (waiters whose only remaining gating dep was this
    /// Analysis key). No Analysis DAG identity has to exist —
    /// callers use this to propagate a terminal **Source** failure
    /// to Analysis-keyed waiters at the same `(canonical,
    /// generation)`. The Analysis identity for that `(canonical,
    /// generation)` is never admitted on a Source failure (its
    /// admission is gated on Source success via
    /// [`crate::scheduler::Scheduler::handle_stage_complete`]), so
    /// the Analysis-keyed waiters would otherwise remain pinned
    /// on a dep that cannot make progress.
    ///
    /// `cause` is the producer's terminal [`SchedulerError`] —
    /// reused verbatim as the [`FailedDepRecord::cause`] field on
    /// every recorded marker so a downstream consumer can
    /// disambiguate the failure mode (FileNotFound vs StageFailed)
    /// instead of reconstructing it from the dep key alone.
    ///
    /// This method ONLY touches the `waiters` reverse-index entry
    /// for the Analysis DepKey; it does NOT cancel any DAG node,
    /// release any capacity permit, or alter `by_identity`. The
    /// caller (`terminalize_failure(Source)`) handles the Source
    /// identity cancellation separately.
    pub fn fanout_source_failure_to_analysis_waiters(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        cause: &SchedulerError,
    ) -> Vec<SubmissionToken> {
        let analysis_dep_key = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Analysis,
        };
        self.fanout_failure_to_analysis_dep_waiters(&analysis_dep_key, cause)
    }

    /// Symmetric Analysis-stage fan-out: a terminal Analysis failure
    /// at `(canonical, generation)` must record a
    /// [`FailedDepRecord`] on every already-admitted downstream
    /// waiter that gated on `DepKey::FileStage { canonical,
    /// generation, stage: Analysis }`. Without this marker, the
    /// generic [`SchedulerDag::cancel`] path that
    /// [`crate::scheduler::Scheduler::terminalize_failure`] runs
    /// after this fan-out would release the waiter's
    /// `deps_remaining` entry with NO `FailedDepRecord`, letting
    /// the waiter dispatch and resolve `Ready` over a snapshot
    /// built from a dead prerequisite. (Source-side has the same
    /// fan-out via
    /// [`Self::fanout_source_failure_to_analysis_waiters`]; this is
    /// the symmetric Analysis-stage helper.)
    ///
    /// `cause` is the producer's terminal [`SchedulerError`] —
    /// reused verbatim as the [`FailedDepRecord::cause`] field on
    /// every recorded marker so a downstream consumer can
    /// disambiguate the failure mode (FileNotFound vs StageFailed)
    /// instead of reconstructing it from the dep key alone.
    ///
    /// This method ONLY touches the `waiters` reverse-index entry
    /// for the Analysis DepKey; it does NOT cancel the Analysis
    /// DAG identity or release its capacity permit. The caller
    /// (`terminalize_failure(Analysis)`) calls this BEFORE
    /// `cancel(&analysis_identity)` so the cancel's waiter sweep
    /// observes an empty reverse-index entry and does not strip
    /// the dep without recording the failure.
    pub fn fanout_analysis_failure_to_waiters(
        &mut self,
        canonical: &Arc<str>,
        generation: u64,
        cause: &SchedulerError,
    ) -> Vec<SubmissionToken> {
        let analysis_dep_key = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Analysis,
        };
        self.fanout_failure_to_analysis_dep_waiters(&analysis_dep_key, cause)
    }

    /// Shared fan-out helper for both Source- and Analysis-stage
    /// failure paths: drains `self.waiters[analysis_dep_key]`,
    /// removes the dep from each waiter's `deps_remaining`, and
    /// records a [`FailedDepRecord`] on the waiter so the
    /// pre-dispatch chokepoint surfaces a typed
    /// [`crate::job::SchedulerError::DependencyFailed`]. Returns
    /// the list of waiters newly stranded by the dep removal.
    fn fanout_failure_to_analysis_dep_waiters(
        &mut self,
        analysis_dep_key: &DepKey,
        cause: &SchedulerError,
    ) -> Vec<SubmissionToken> {
        let mut stranded = Vec::new();
        let Some(waiters) = self.waiters.remove(analysis_dep_key) else {
            return stranded;
        };
        for waiter_tok in waiters {
            let Some(waiter) = self.nodes.get_mut(&waiter_tok) else {
                continue;
            };
            if waiter.cancelled {
                continue;
            }
            waiter.deps_remaining.remove(analysis_dep_key);
            // Record the failed DepKey on the waiter so the
            // executor short-circuits with a typed
            // `DependencyFailed` instead of running the user-side
            // stage executor over a dep whose Analysis can never
            // commit. Without this marker the Artifact (or other
            // downstream) executor reads the OWNER's
            // `current_source()` / `current_analysis()` only — it
            // never re-consults the failed dep — and silently
            // returns `Ready` even though the declared blocker died.
            //
            // The recorded `FailedDepRecord` carries `cause` so the
            // surfaced `DependencyFailed` preserves the producer's
            // terminal error (FileNotFound, StageFailed, etc.)
            // instead of synthesising a stage-only envelope.
            waiter.failed_blocker_deps.insert(
                analysis_dep_key.clone(),
                FailedDepRecord {
                    dep_key: analysis_dep_key.clone(),
                    cause: cause.clone(),
                },
            );
            if waiter.deps_remaining.is_empty() && !waiter.dispatched {
                stranded.push(waiter_tok);
            }
        }
        stranded
    }

    /// Record a terminal producer-failure record on the persistent
    /// [`SchedulerDag::terminal_dep_failures`] store, keyed by the
    /// [`DepKey`] of the failed prerequisite.
    ///
    /// Inserted by [`crate::scheduler::Scheduler::terminalize_failure`]
    /// (Source or Analysis) under the dep's Analysis [`DepKey`] so
    /// future admissions that consult the dead-producer matrix see
    /// the failure regardless of fan-out timing.
    ///
    /// Re-insertion replaces the prior record — the latest terminal
    /// cause is authoritative.
    pub fn insert_terminal_dep_failure(&mut self, record: FailedDepRecord) {
        self.terminal_dep_failures
            .insert(record.dep_key.clone(), record);
    }

    /// Consult the persistent
    /// [`SchedulerDag::terminal_dep_failures`] store for a
    /// previously-recorded terminal failure at `dep_key`. Returns a
    /// clone so the caller can attach the record to a
    /// freshly-admitted waiter without holding the DAG lock for the
    /// attach step.
    pub fn lookup_terminal_dep_failure(&self, dep_key: &DepKey) -> Option<FailedDepRecord> {
        self.terminal_dep_failures.get(dep_key).cloned()
    }

    /// Attach a [`FailedDepRecord`] to the node currently dedup-
    /// associated with `identity` (if any). Used by the admission
    /// path to mark a freshly-submitted Artifact / Analysis node
    /// with a previously-recorded terminal dep failure so the
    /// pre-dispatch short-circuit in `execute_stage_on_worker` fires
    /// before the user-side stage executor runs.
    ///
    /// Idempotent: re-attaching the same `FailedDepRecord` replaces
    /// in place (the carrier is keyed by `DepKey`).
    ///
    /// Returns `true` when the attach landed on a live (non-
    /// cancelled, non-removed) node; `false` otherwise. The boolean
    /// is informational — admission paths use it for debug-assert
    /// invariants, not control flow.
    pub fn attach_failed_dep(
        &mut self,
        identity: &WorkNodeIdentity,
        record: FailedDepRecord,
    ) -> bool {
        let Some(&token) = self.by_identity.get(identity) else {
            return false;
        };
        let Some(node) = self.nodes.get_mut(&token) else {
            return false;
        };
        if node.cancelled {
            return false;
        }
        node.failed_blocker_deps
            .insert(record.dep_key.clone(), record);
        true
    }

    /// Drop every entry in
    /// [`SchedulerDag::terminal_dep_failures`] whose [`DepKey`]
    /// references `canonical` (either as the dep's canonical file or
    /// as an Artifact / FileStage canonical payload). Called from
    /// `Scheduler::remove(canonical)` so a stale terminal-failure
    /// record on a removed file cannot pin a future admission as
    /// `Failed`. Idempotent.
    pub fn scrub_terminal_dep_failures_referencing(&mut self, canonical: &str) {
        self.terminal_dep_failures.retain(|key, _record| match key {
            DepKey::FileStage { canonical: c, .. } | DepKey::Artifact { canonical: c, .. } => {
                c.as_ref() != canonical
            }
            DepKey::CacheNode { .. } => true,
        });
    }

    /// Drop the persistent terminal-dep-failure entry for
    /// `(canonical, generation, Analysis)` — the `DepKey` shape
    /// cross-file blockers always observe. Called from the
    /// Source/Analysis completion path so a same-generation
    /// recovery (e.g. a Source-failed dep retried at the same
    /// generation and succeeding) does not leave a stale record
    /// behind that would misclassify the dep as `Failed` on the
    /// next admission. Idempotent: a missing entry is a no-op.
    pub fn clear_terminal_dep_failure_for_gen(&mut self, canonical: &Arc<str>, generation: u64) {
        let analysis_key = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            generation,
            stage: FileStageKey::Analysis,
        };
        self.terminal_dep_failures.remove(&analysis_key);
    }
}
