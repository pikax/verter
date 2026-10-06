//! Dependency gating — blocker classification, auto-ingest and cycle filtering.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

/// Classification of a recorded `FileStage::Analysis` blocker dep
/// against the live FileNode + DAG state. Produced by
/// [`Scheduler::file_stage_analysis_blocker_status`] and consumed by
/// both the pre-admission filter ([`Scheduler::register_resolved_deps`])
/// and the recorded-blocker filter
/// ([`Scheduler::admit_artifact_with_blockers`]). Centralising the
/// classifier prevents the two predicates from drifting — the
/// dead-producer matrix has several distinct rows that an
/// independent re-implementation tends to collapse.
///
/// Three-state split. The earlier two-state encoding (`Resolved`)
/// collapsed two semantically distinct outcomes:
///
/// - `Satisfied` — the prerequisite reached committed Analysis OR
///   is genuinely moot (FileNode missing, stale generation, gen 0,
///   no persistent failure record). The blocker is dropped silently
///   and the downstream admission proceeds as usual.
/// - `Failed(record)` — the prerequisite terminalized (Source /
///   Analysis failure). The persistent
///   [`crate::dag::SchedulerDag::terminal_dep_failures`] store
///   carries a [`crate::dag::FailedDepRecord`] for this DepKey;
///   the caller MUST attach the record to the freshly-submitted
///   waiter so the pre-dispatch short-circuit in
///   [`Scheduler::execute_stage_on_worker`] surfaces a typed
///   `DependencyFailed`. Without this discrimination the matrix
///   would collapse `Failed` onto `Resolved` and the admission
///   would silently drop the blocker, resolving the waiter `Ready`
///   on a snapshot built from a dead prerequisite — the
///   pre-admission failure race.
#[derive(Clone, Debug)]
pub(super) enum BlockerStatus {
    /// The dep is still gating: an Analysis is committed (and the
    /// owner waits for completion fan-out) OR an Analysis identity is
    /// live in the DAG (queued or dispatched).
    Gating,
    /// The dep is no longer gating without a terminal-failure
    /// record: Analysis is committed, the FileNode is missing, the
    /// recorded generation is stale, or the gen is 0. The blocker is
    /// dropped silently.
    Satisfied,
    /// The dep is no longer gating because the producer terminally
    /// failed and the persistent store carries the recorded cause.
    /// The caller MUST attach this [`crate::dag::FailedDepRecord`]
    /// to its admitted node so the pre-dispatch short-circuit fires.
    Failed(crate::dag::FailedDepRecord),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AnalysisDemandKind {
    DirectRequest,
    ArtifactBlocker,
}

/// Record planted in [`Scheduler::auto_ingested_recent`] when
/// [`Scheduler::register_resolved_deps`] auto-ingests a dep blocker by
/// enqueueing a `Submission::NewRequest` to the inbox. The record makes
/// the "queued in inbox, not yet drained" state explicit so the
/// dead-producer matrix can distinguish it from "Source failed and
/// cancelled" — both leave the FileNode with `current_source().is_none()`
/// and no live Source/Analysis DAG identity.
///
/// The record is removed when the driver finally drains the queued
/// `NewRequest` and admits a Source DAG identity (see the cleanup arm
/// in [`Scheduler::handle_new_request`]). A stale entry left behind by
/// a driver-thread crash is trimmed by the [`STALE_THRESHOLD`]-based
/// sweep in the matrix consumer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct AutoIngestedRecord {
    pub(crate) incarnation: u64,
    /// Generation the dep FileNode was at when the auto-ingest fired.
    /// Matched against the dep's current generation in the matrix; a
    /// mismatch means the entry is stale (the dep has advanced beyond
    /// the auto-ingest, so the gate it set up no longer applies).
    pub(crate) generation: u64,
    /// Monotonic instant the record was planted. Used by the cleanup
    /// sweep to trim entries whose admission never landed (e.g. driver
    /// crash between insert and dequeue).
    pub(crate) since: Instant,
}

/// Trim threshold for [`Scheduler::auto_ingested_recent`] entries. An
/// entry older than this is dropped on consumption — belt-and-suspenders
/// against a driver-thread crash between
/// [`Scheduler::register_resolved_deps`]'s insert and
/// [`Scheduler::handle_new_request`]'s removal arm. Under normal
/// operation the removal arm fires on the next driver tick, so the
/// threshold is generous.
pub(super) const AUTO_INGESTED_RECENT_STALE_THRESHOLD: std::time::Duration =
    std::time::Duration::from_secs(60);

impl Scheduler {
    /// Drop any dep whose Analysis transitively waits on the
    /// owner's Analysis — the single chokepoint for macro-type-dep
    /// cycle filtering shared by both blocker-registration paths:
    ///
    /// 1. The immediate path at the bottom of
    ///    [`Self::register_resolved_deps`] (Source already complete
    ///    when blockers arrive).
    /// 2. The Source-completion replay path inside
    ///    [`Self::handle_stage_complete`] (TaskKind::Load arm).
    ///
    /// Catches three cycle classes uniformly via the DAG's bounded
    /// reachability walk ([`SchedulerDag::dep_reaches_owner`]):
    ///
    /// - Direct self: `A → A`.
    /// - Direct mutual: `A ↔ B`.
    /// - Transitive: `A → B → C → A` (bounded BFS).
    ///
    /// The caller MUST hold the DAG lock through this call and the
    /// subsequent `record_artifact_blockers` so two concurrent
    /// completions cannot race past the filter into mutually-blocking
    /// registry entries.
    ///
    /// Returns the `(kept, dropped)` split for traceability. The
    /// `dropped` half is currently unused at the call-site but
    /// preserves the diagnostic the test suite asserts against.
    pub(super) fn filter_macro_cycle_deps(
        dag: &crate::dag::SchedulerDag,
        owner_canonical: &Arc<str>,
        incarnation: u64,
        owner_generation: u64,
        deps: Vec<DepKey>,
    ) -> (Vec<DepKey>, Vec<DepKey>) {
        let mut kept = Vec::with_capacity(deps.len());
        let mut dropped = Vec::new();
        for dep in deps {
            let drop_this = if let DepKey::FileStage {
                canonical: dep_canonical,
                incarnation: dep_incarnation,
                generation: dep_generation,
                stage: FileStageKey::Analysis,
            } = &dep
            {
                dag.dep_reaches_owner(
                    owner_canonical,
                    incarnation,
                    owner_generation,
                    dep_canonical,
                    *dep_incarnation,
                    *dep_generation,
                )
            } else {
                // Non-Analysis deps are not part of the macro-type
                // cycle class the filter is responsible for.
                false
            };
            if drop_this {
                dropped.push(dep);
            } else {
                kept.push(dep);
            }
        }
        (kept, dropped)
    }

    /// Remove a [`Self::auto_ingested_recent`] entry for
    /// `(canonical, generation)` when the matching Source DAG
    /// identity has been admitted. The matrix consumes this set to
    /// detect the "auto-ingest queued in inbox, not yet drained"
    /// state; once the driver dequeues the `NewRequest` and admits
    /// the Source identity, that state is over — the live Source
    /// identity in `by_identity` is now the source of truth and the
    /// tracking entry would only confuse future matrix lookups.
    pub(super) fn clear_auto_ingest_tracking(
        &self,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
    ) {
        // Atomic value-conditional removal: only drop the entry when
        // the live entry's generation still matches the one we are
        // clearing for. `DashMap::remove_if` evaluates the predicate
        // under the same shard write lock that performs the remove,
        // so a concurrent insert of a later generation between a
        // non-atomic `get` + `remove` (the previous pattern) can no
        // longer delete the newer entry by accident. An entry for a
        // later generation stays so the next driver tick's admission
        // of the newer generation's Source request finds it and
        // clears it on its own match.
        self.auto_ingested_recent.remove_if(canonical, |_k, v| {
            v.incarnation == incarnation && v.generation == generation
        });
    }

    /// Admit an Artifact work node with any late-discovered blocker
    /// `DepKey`s attached. Reads the per-(owner, generation) blocker
    /// set from the DAG's typed registry, filters out blockers whose
    /// Analysis is already committed (no longer gating), filters out
    /// dead-producer entries (FileNode gone AND no live Analysis
    /// identity in the DAG), and submits the Artifact identity with
    /// the remaining `DepKey`s as deps.
    ///
    /// Called from every Artifact admission site so a blocker
    /// registered via [`Self::register_resolved_deps`] AFTER the
    /// owner's Analysis has dispatched (or completed) still gates
    /// the Artifact run on the blocker's Analysis. The in-flight
    /// Analysis node itself never depends on these late-discovered
    /// blockers (its incoming edges are immutable once dispatched).
    ///
    /// Registry lifecycle:
    ///
    /// - When every recorded blocker has already resolved (or every
    ///   entry was a dead producer) the entry is cleared so future
    ///   Artifact admissions at this generation do not consult a
    ///   stale registry view.
    /// - Otherwise the entry stays in place so a re-admission (e.g.
    ///   a same-generation re-request for a different profile) still
    ///   picks up the unresolved blockers.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn admit_artifact_with_blockers(
        &self,
        dag: &mut SchedulerDag,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        profile_hash: u64,
        priority: Priority,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) -> Option<crate::dag::SubmissionToken> {
        let live = self
            .nodes
            .get(canonical.as_ref())
            .map(|entry| Arc::clone(entry.value()))?;
        if !live.admits_work(incarnation, generation) {
            return None;
        }

        let mut blocker_deps: Vec<DepKey> = Vec::new();
        // Failed-dep records to attach to the just-submitted
        // Artifact node so the pre-dispatch short-circuit in
        // `execute_stage_on_worker` surfaces a typed
        // `DependencyFailed` even when the producer failed BEFORE
        // this Artifact was admitted (pre-admission failure race —
        // the matrix consults `terminal_dep_failures` AND the
        // registry-attached `failed` list both as Failed sources).
        let mut failed_records: Vec<crate::dag::FailedDepRecord> = Vec::new();
        // Drain + re-record under the DAG lock: read the recorded
        // blockers (live deps + persisted failure records), route
        // EVERY entry — live deps AND previously-failed deps — through
        // the 3-state matrix against the current DAG state, then write
        // back the rebuilt set so a subsequent admission for a different
        // profile at the same generation picks up the correct view. An
        // empty re-record clears the entry (see
        // `SchedulerDag::record_artifact_blockers`).
        //
        // Rebuilding the persisted `failed` set from classification (not
        // pass-through) is load-bearing in two directions:
        //
        //   1. A live dep that fails between admissions must populate the
        //      NEXT-admission `failed` set, not just the current Artifact.
        //      If we keep the prior `stored.failed` set verbatim we strand
        //      the new failure on the current Artifact alone — a later
        //      profile admission would drain an empty registry and resolve
        //      Ready over the dead prerequisite.
        //   2. A previously-failed dep that recovers at the same gen
        //      (the same-gen recovery path cleared
        //      `terminal_dep_failures`) must drop from the next
        //      `failed` set, not ride through verbatim — otherwise the
        //      next admission attaches a stale `DependencyFailed` record
        //      to a now-Satisfied dep.
        //
        // Both behaviours fall out of routing every persisted entry
        // through `classify_recorded_dep` against the live state.
        let stored = dag.drain_artifact_blockers(canonical, generation);
        let mut still_pending: std::collections::BTreeSet<DepKey> =
            std::collections::BTreeSet::new();
        let mut next_failed: Vec<crate::dag::FailedDepRecord> = Vec::new();
        // Classify every live dep first.
        for dep in stored.deps.into_iter() {
            match self.classify_recorded_dep(dag, &dep) {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(record) => {
                    // Producer terminally failed between record time
                    // and this Artifact admission. Drop from
                    // `blocker_deps` so the Artifact does not gate
                    // on it, attach to the current Artifact via
                    // `failed_records`, AND persist the failure into
                    // `next_failed` so a future profile admission at
                    // the same gen still surfaces the same
                    // `DependencyFailed`.
                    failed_records.push(record.clone());
                    next_failed.push(record);
                    continue;
                }
                BlockerStatus::Gating => {
                    blocker_deps.push(dep.clone());
                    still_pending.insert(dep);
                }
            }
        }
        // Re-classify each persisted failure record against the live
        // state. A persisted failure can transition out of `Failed`
        // when the same-gen recovery path clears
        // `terminal_dep_failures` on a same-gen Source/Analysis
        // recovery: the matrix returns `Satisfied`
        // (and we drop the record) or `Gating` (and the dep is still
        // a live blocker again). The producer's terminalized DAG
        // identity cannot re-emerge as a different live identity at
        // the SAME generation, so a `Gating` verdict on a previously-
        // failed dep only fires after a successful recovery — in which
        // case treating it as a live gating dep again is correct.
        for record in stored.failed.into_iter() {
            match self.classify_recorded_dep(dag, &record.dep_key) {
                BlockerStatus::Satisfied => continue,
                BlockerStatus::Failed(current_record) => {
                    // Producer still terminally failed at this gen.
                    // Attach to the current Artifact AND persist
                    // for future admissions. Reuse the freshly-looked-
                    // up record (its cause is identical, but using the
                    // classifier's return keeps a single source of
                    // truth for the persisted failure record).
                    failed_records.push(current_record.clone());
                    next_failed.push(current_record);
                }
                BlockerStatus::Gating => {
                    // Producer recovered at the same generation
                    // (the same-gen recovery path cleared the
                    // persistent failure record and the pipeline is
                    // alive again — Source / Analysis queued or in
                    // flight, or auto-ingest pending). Promote back
                    // to a live gating dep so the Artifact gates on
                    // the resumed Analysis. Persist as a gating dep,
                    // not a failure.
                    blocker_deps.push(record.dep_key.clone());
                    still_pending.insert(record.dep_key);
                }
            }
        }
        // Re-record both the still-pending live deps AND the
        // rebuilt failure records under the same key so a future
        // Artifact admission at this generation (e.g. a different
        // profile) still picks them up. An empty re-record (no deps
        // AND no failed) drops the entry (which
        // `record_artifact_blockers` treats as a remove).
        let next_pending = crate::dag::PendingBlockerSet {
            deps: still_pending,
            failed: next_failed,
        };
        dag.record_artifact_blockers(canonical, generation, next_pending);

        let identity = WorkNodeIdentity::Artifact {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            profile_hash: profile_hash_to_bytes(profile_hash),
            content_hash: [0u8; 16],
        };
        let token = dag.submit_file(
            &live,
            identity.clone(),
            WorkKind::Artifact,
            priority,
            blocker_deps,
            request_context,
        );
        // Attach every failed-dep record to the just-submitted
        // Artifact node. The dispatched-node dedup branch of
        // `submit` would not pick these up (incoming edges are
        // immutable after dispatch), so `attach_failed_dep`'s no-op
        // return for that case is the correct shape: a dispatched
        // in-flight Artifact already carries its own marker (or
        // none — meaning the producer failed AFTER dispatch and
        // the fan-out path will deliver it via `signal_file_failed`).
        for record in failed_records {
            dag.attach_failed_dep(&identity, record);
        }
        token
    }

    /// Classify a recorded blocker dep against the live FileNode +
    /// DAG state. Wraps [`Self::file_stage_analysis_blocker_status`]
    /// for the only [`DepKey`] variant cross-file blockers use
    /// (`FileStage::Analysis`). Other variants (Source-stage and
    /// Artifact and CacheNode) cannot appear in the recorded
    /// blocker registry; classify them as `Satisfied` defensively
    /// so a future producer that mis-records returns to the safe
    /// "drop the blocker" path rather than pinning the admission.
    pub(super) fn classify_recorded_dep(&self, dag: &SchedulerDag, dep: &DepKey) -> BlockerStatus {
        match dep {
            DepKey::FileStage {
                canonical,
                incarnation,
                generation,
                stage: FileStageKey::Analysis,
            } => self.file_stage_analysis_blocker_status(dag, canonical, *incarnation, *generation),
            _ => BlockerStatus::Satisfied,
        }
    }

    /// Atomically ensure the producer pipeline needed by one Analysis demand.
    /// Callers hold the scheduler-state mutex, so observing the integrated Source fence,
    /// checking existing identities, and admitting Analysis are one state
    /// transition. Blockers preserve terminal failure; direct requests may
    /// retry Analysis at the same generation.
    pub(super) fn ensure_analysis_for_demand(
        &self,
        dag: &mut SchedulerDag,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        priority: Priority,
        kind: AnalysisDemandKind,
    ) -> BlockerStatus {
        let Some(node) = self
            .nodes
            .get(canonical.as_ref())
            .map(|entry| entry.clone())
        else {
            return if self.auto_ingest_tracking_gates(canonical, incarnation, generation) {
                BlockerStatus::Gating
            } else {
                BlockerStatus::Satisfied
            };
        };
        if node.incarnation_id() != incarnation || node.generation() != generation {
            return BlockerStatus::Satisfied;
        }
        if node.current_analysis().is_some() {
            return BlockerStatus::Satisfied;
        }

        let analysis_dep = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Analysis,
        };
        if kind == AnalysisDemandKind::ArtifactBlocker {
            if let Some(record) = dag.lookup_terminal_dep_failure(&analysis_dep) {
                return BlockerStatus::Failed(record);
            }
        }

        let analysis_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Analysis,
        };
        if dag.token_for(&analysis_identity).is_some() {
            return BlockerStatus::Gating;
        }

        if node.current_integrated_source().is_some() {
            if kind == AnalysisDemandKind::DirectRequest {
                dag.clear_terminal_dep_failure_for_gen(canonical, incarnation, generation);
            }
            return if admit_work(
                dag,
                &node,
                canonical,
                generation,
                TaskKind::Analysis,
                priority,
                None,
            )
            .is_some()
            {
                BlockerStatus::Gating
            } else {
                if kind == AnalysisDemandKind::DirectRequest {
                    Self::terminalize_refused_admission(dag, canonical, generation);
                }
                BlockerStatus::Satisfied
            };
        }

        let source_identity = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Source,
        };
        if dag.token_for(&source_identity).is_some()
            || self.auto_ingest_tracking_gates(canonical, incarnation, generation)
        {
            BlockerStatus::Gating
        } else {
            BlockerStatus::Satisfied
        }
    }

    /// Shared classifier for a `FileStage::Analysis` blocker dep
    /// recorded for `(canonical, generation)`. Returns whether the
    /// dep is still gating an owner's Artifact admission, OR whether
    /// the blocker can be dropped (either because the dep is already
    /// satisfied OR because the producer is dead and will never
    /// satisfy it).
    ///
    /// Dead-producer matrix. The previous shape only distinguished
    /// "FileNode missing" from "FileNode present"; that missed the
    /// case where Source or Analysis previously FAILED at this same
    /// generation: the DAG identity was cancelled by
    /// [`Self::terminalize_failure`], but the FileNode remains with
    /// `current_*().is_none()`, and the old predicate reported the
    /// blocker as still gating forever.
    ///
    /// | FileNode + DAG state                                                                              | status     |
    /// |---|---|
    /// | FileNode missing, `auto_ingested_recent` entry present at matching gen                          | Gating (auto-ingest queued, FileNode lookup raced ahead of insert) |
    /// | FileNode missing                                                                                  | **Resolved** (producer gone) |
    /// | FileNode present, generation mismatch (including the `generation == 0` recorded blocker)         | **Resolved** (recorded blocker is stale) |
    /// | FileNode present, same gen, `current_analysis().is_some()`                                       | **Resolved** (Analysis already committed; DAG identity is gone and no fan-out remains) |
    /// | FileNode present, same gen, no committed Analysis, but a live Analysis DAG identity exists       | Gating (Analysis in flight or queued; completion/cancel fan-out will fire) |
    /// | FileNode present, same gen, no committed Analysis, no Analysis DAG identity, but Source DAG identity exists | Gating (Source queued or dispatched; Analysis will be admitted on Source completion) |
    /// | FileNode present, same gen, no live Source/Analysis DAG identity, `auto_ingested_recent` entry present at matching gen | Gating (auto-ingest queued in inbox, driver has not yet drained the NewRequest) |
    /// | FileNode present, same gen, no committed Analysis, no Analysis DAG identity, no Source DAG identity, `current_source().is_some()` | **Resolved** (Source committed but Analysis failed/cancelled — dead producer) |
    /// | FileNode present, same gen, no committed Analysis, no Source DAG identity, `current_source().is_none()` | **Resolved** (Source failed and was cancelled — dead producer) |
    ///
    /// The Source-DAG-identity check distinguishes "Source pending /
    /// in flight" from "Source failed and cancelled." Both leave
    /// `current_source().is_none()` on the FileNode; only the live
    /// Source identity in `by_identity` proves the pipeline is
    /// alive. `terminalize_failure(Source)` removes the identity,
    /// so the absence is the dead-producer signal.
    ///
    /// The `auto_ingested_recent` consultation closes the pre-drain
    /// window: [`Self::register_resolved_deps`] inserts a tracking
    /// entry BEFORE enqueueing the auto-ingest `NewRequest`. Until
    /// the driver drains the inbox and [`Self::handle_new_request`]
    /// admits a Source DAG identity (which removes the entry), the
    /// dep is structurally indistinguishable from a Source-failed
    /// corpse — same FileNode shape, no live DAG identity. Without
    /// this check the matrix would classify the queued-but-undrained
    /// state as `Resolved` and the owner's Artifact would be admitted
    /// prematurely. The lookup is matched against the live FileNode
    /// generation: a stale tracking entry from a previous incarnation
    /// (or an entry older than [`AUTO_INGESTED_RECENT_STALE_THRESHOLD`])
    /// is ignored and dropped so it cannot pin future admissions.
    pub(super) fn file_stage_analysis_blocker_status(
        &self,
        dag: &SchedulerDag,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
    ) -> BlockerStatus {
        // First: consult the persistent terminal-dep-failure store.
        // `terminalize_failure(Source|Analysis)` records an entry
        // under the Analysis `DepKey` for this `(canonical,
        // generation)`. A match means the producer terminally
        // failed — return `Failed(record)` so the caller attaches
        // the record to its admitted node and the pre-dispatch
        // short-circuit fires.
        let analysis_dep_key = DepKey::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Analysis,
        };
        if let Some(record) = dag.lookup_terminal_dep_failure(&analysis_dep_key) {
            return BlockerStatus::Failed(record);
        }

        let canonical_str: &str = canonical.as_ref();
        let node = match self.nodes.get(canonical_str) {
            Some(n) => n,
            None => {
                // FileNode gone or not yet inserted. The producer
                // either cannot make progress (Satisfied — moot) OR
                // is in the pre-drain auto-ingest window — consult
                // the tracking set before classifying.
                if self.auto_ingest_tracking_gates(canonical, incarnation, generation) {
                    return BlockerStatus::Gating;
                }
                return BlockerStatus::Satisfied;
            }
        };
        if generation == 0 {
            // Generation 0 never carries a live Analysis identity —
            // the first scheduler admission bumps the node above 0
            // before submitting any DAG identity. A recorded blocker
            // at gen 0 is stale.
            return BlockerStatus::Satisfied;
        }
        if node.incarnation_id() != incarnation || node.generation() != generation {
            // Different generation — the recorded blocker is for
            // a generation that no longer exists. Stale.
            //
            // Opportunistic cleanup: drop any tracking entry that
            // matches the stale generation under a value-conditional
            // removal. The matrix would otherwise leave the stale
            // entry to age out through the
            // `AUTO_INGESTED_RECENT_STALE_THRESHOLD` (60 s) sweep
            // in `auto_ingest_tracking_gates`, holding memory for
            // every invalidated dep across that window. The
            // remove_if predicate guards against concurrent
            // re-insertion at a newer generation (the value-
            // conditional removal pattern).
            self.auto_ingested_recent.remove_if(canonical, |_k, v| {
                v.incarnation == incarnation && v.generation == generation
            });
            return BlockerStatus::Satisfied;
        }
        if node.current_analysis().is_some() {
            // Analysis committed at the recorded generation. The
            // DAG identity is already gone (`dag.complete(...)` removed
            // it), so recording this dep on a new Artifact admission
            // would put a DepKey nobody will fire into the waiter
            // reverse-index. Drop it — the Analysis output is
            // already on the node and the owner does not need to
            // gate on it.
            return BlockerStatus::Satisfied;
        }
        // Analysis not committed. Decide between "in flight",
        // "pending behind Source", and "moot" by consulting the DAG
        // for both stage identities.
        let analysis_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Analysis,
        };
        if dag.token_for(&analysis_id).is_some() {
            // Analysis is queued or dispatched — still gating
            // (whether or not Source is committed yet).
            return BlockerStatus::Gating;
        }
        let source_id = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(canonical),
            incarnation,
            generation,
            stage: FileStageKey::Source,
        };
        if dag.token_for(&source_id).is_some() {
            // Source is queued or in flight. Analysis has not been
            // admitted yet (Source completion is the trigger). The
            // pipeline is alive — keep the blocker so the eventual
            // Analysis completion fans out to the owner's Artifact.
            return BlockerStatus::Gating;
        }
        // No live DAG identity for Source AND no live DAG identity
        // for Analysis. The FileNode + DAG shape matches both
        // "auto-ingest queued in inbox, driver not yet drained" AND
        // "producer terminalized but the persistent terminal-dep-
        // failure record has been cleaned up (e.g. by supersede)."
        // Consult the tracking set to disambiguate: a matching
        // entry proves the auto-ingest pipeline is alive and the
        // driver will admit it on the next tick. Without an entry
        // the producer is dead-but-moot (Satisfied), since
        // `terminal_dep_failures` already returned None at the top
        // of this matrix — the record (if any) was cleaned up and
        // the blocker is no longer a discriminator.
        if self.auto_ingest_tracking_gates(canonical, incarnation, generation) {
            return BlockerStatus::Gating;
        }
        BlockerStatus::Satisfied
    }

    /// Consult [`Self::auto_ingested_recent`] for a tracking entry on
    /// `(canonical, generation)`. Returns `true` when an entry exists
    /// at the matching generation AND the entry is not older than
    /// [`AUTO_INGESTED_RECENT_STALE_THRESHOLD`]; in that case the
    /// matrix MUST gate so the owner's Artifact waits for the
    /// in-flight auto-ingest. Otherwise (no entry, mismatched gen,
    /// or stale by age) the entry is dropped if present and the
    /// matrix continues to its dead-producer arm.
    ///
    /// Stale-by-age handling is belt-and-suspenders for the case
    /// where the driver thread crashes between
    /// [`Self::register_resolved_deps`]'s insert and
    /// [`Self::handle_new_request`]'s removal arm. Under normal
    /// operation the removal arm fires on the next driver tick and
    /// this branch never sees an aged entry.
    pub(super) fn auto_ingest_tracking_gates(
        &self,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
    ) -> bool {
        // DashMap entries are short-lived here — the typical removal
        // path is `handle_new_request` admitting the Source DAG
        // identity, which runs synchronously after the matrix
        // consult. Hold the entry only across the freshness check.
        let entry = match self.auto_ingested_recent.get(canonical) {
            Some(e) => e,
            None => return false,
        };
        let entry_incarnation = entry.incarnation;
        let entry_gen = entry.generation;
        let entry_since = entry.since;
        drop(entry);
        if entry_incarnation != incarnation || entry_gen != generation {
            // Stale generation — drop the entry under a value-conditional
            // remove keyed on the generation we observed. `remove_if`
            // evaluates the predicate under the shard write lock so a
            // concurrent insert of a later generation between this
            // observation and the removal does not delete the newer
            // entry by accident. A live auto-ingest for a different
            // gen will re-insert with the matching gen on the next
            // call.
            self.auto_ingested_recent.remove_if(canonical, |_k, v| {
                v.incarnation == entry_incarnation && v.generation == entry_gen
            });
            return false;
        }
        if entry_since.elapsed() > AUTO_INGESTED_RECENT_STALE_THRESHOLD {
            // Aged out — the auto-ingest never landed (driver
            // crash). Drop the entry so future admissions are not
            // pinned on a ghost. The value-conditional removal again
            // protects a concurrent newer-gen re-insertion: only the
            // aged entry we observed is dropped, never a fresh
            // re-insert at a later generation that happens to land
            // between the observation and the removal.
            self.auto_ingested_recent.remove_if(canonical, |_k, v| {
                v.incarnation == entry_incarnation && v.generation == entry_gen
            });
            return false;
        }
        true
    }
}
