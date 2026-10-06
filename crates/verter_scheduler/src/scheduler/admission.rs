//! Admission — turning a queued submission into one admitted DAG node.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

/// Admit work for the captured node incarnation and generation.
/// Returns the submission token. Used by every admission site
/// (`handle_new_request`, stage-completion driven transitions, etc.).
pub(super) fn admit_work(
    dag: &mut SchedulerDag,
    node: &FileNode,
    canonical: &Arc<str>,
    generation: u64,
    task: TaskKind,
    priority: Priority,
    request_context: Option<crate::request_context::OpaqueRequestContext>,
) -> Option<crate::dag::SubmissionToken> {
    let incarnation = node.incarnation_id();
    let (identity, kind) = match task {
        // The live `FileStage{Source}` DAG node maps to `Load`; the
        // load+parse work runs in one source-stage execution path, so
        // there is no separate `Parse` DAG node admitted here.
        TaskKind::Load => (
            WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Source,
            },
            WorkKind::Load,
        ),
        TaskKind::Analysis => (
            WorkNodeIdentity::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: FileStageKey::Analysis,
            },
            WorkKind::Analysis,
        ),
        TaskKind::Artifact { profile_hash } => (
            WorkNodeIdentity::Artifact {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                profile_hash: profile_hash_to_bytes(profile_hash),
                content_hash: [0u8; 16],
            },
            WorkKind::Artifact,
        ),
        // `Parse` and `CacheNode` have no `TaskKind`-driven file-stage
        // admission path in the single-`FileStage{Source}`-node model:
        // `Parse` is intrinsic to the source stage, and cache nodes are
        // submitted into the DAG directly by the cache layer, never through
        // `admit_work`.
        TaskKind::Parse | TaskKind::CacheNode { .. } => {
            unreachable!(
                "admit_work is the file-stage admission path (Load/Analysis/Artifact); \
                 Parse is intrinsic to the source stage and CacheNode is admitted \
                 directly into the DAG by the cache layer"
            )
        }
    };
    dag.submit_file(node, identity, kind, priority, Vec::new(), request_context)
}

/// Test-only rendezvous fired by [`Scheduler::handle_new_request_batch`]
/// AFTER the FIRST request in a batch has been admitted but BEFORE the
/// remaining requests are admitted — strictly WHILE the single
/// `dag.lock()` is still held.
///
/// This is the seam a §6b LOCK-CONTINUITY test uses to release a
/// concurrent observer that blocking-acquires `dag.lock()`. Because the
/// batch holds ONE continuously-held lock across all N admits, that
/// observer cannot acquire until the loop's `}` drops the lock — at
/// which point the DAG already shows ALL N admitted. It therefore
/// proves "no concurrent thread can ever first-observe the batch
/// half-admitted", the observable consequence of the continuity
/// invariant.
///
/// CONTINUITY proper — "all N admits ran under ONE lock acquisition" —
/// is proven deterministically and independently of any contention race
/// by the [`Scheduler::dag_admit_epoch`] rail: the admission-lock
/// acquisition bumps a monotonic epoch, every admit records the epoch it
/// ran under, and the test asserts all N records share one epoch. A
/// per-item lock/unlock regression acquires the lock (and bumps the
/// epoch) once per item, so the recorded epochs DIFFER and the test
/// FAILS regardless of how the lock-handoff race happens to resolve.
/// (A non-blocking `try_lock` at a single seam — the prior shape — only
/// proved the lock was held AT THE SEAM, not continuously across items
/// 2..N; and a blocking-acquire race alone is not a both-directions
/// discriminator because `parking_lot`'s barging unlock can let a
/// per-item re-lock win over a parked waiter. The epoch rail closes
/// both gaps.)
///
/// Carries zero footprint outside test builds — the field, the firing
/// site, the installer, and the epoch/trace instrumentation are all
/// `cfg`-gated to `test` / the opt-in `test-support` feature, so
/// production batch admission is unchanged.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub(crate) struct BatchAdmitSeamHook {
    /// Installed by a test; fired once per batch, on the admitting
    /// thread, between the first and second admit while the DAG lock is
    /// held. `Box<dyn Fn>` is not `Debug`, hence the manual `Debug`.
    pub(super) hook: Mutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

#[cfg(any(test, feature = "test-support"))]
impl std::fmt::Debug for BatchAdmitSeamHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchAdmitSeamHook")
            .field("installed", &self.hook.lock().is_some())
            .finish()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl BatchAdmitSeamHook {
    /// Install the seam hook. Fired once per subsequent batch, after the
    /// first admit and before the rest, while `dag.lock()` is held.
    pub(super) fn install(&self, hook: Box<dyn Fn() + Send + Sync>) {
        *self.hook.lock() = Some(hook);
    }

    /// Fire the installed hook (no-op when none is installed). Called by
    /// [`Scheduler::handle_new_request_batch`] at the mid-batch seam.
    pub(super) fn fire(&self) {
        if let Some(hook) = self.hook.lock().as_ref() {
            hook();
        }
    }
}

/// A request after pre-lock preparation (queued-incarnation gate + live-node lookup),
/// carrying everything the shared admission core needs to admit it
/// under the DAG lock. The `node` `Arc` was cloned out of the `nodes`
/// DashMap so no shard `Ref` is held across `dag.lock()`.
pub(super) struct PreparedRequest {
    pub(super) file_id: String,
    pub(super) canonical: Arc<str>,
    pub(super) node: Arc<FileNode>,
    pub(super) target: TargetStage,
    pub(super) priority: Priority,
    pub(super) source: Option<Arc<str>>,
    pub(super) sender: CompletionSender<RequestResult>,
    pub(super) request_context: Option<crate::request_context::OpaqueRequestContext>,
    /// Resolved language carried from preparation so the admission core
    /// can perform any language re-home under the DAG lock. Preparation
    /// deliberately does NOT re-home: that advances a published file's
    /// generation and must be atomic with the supersede sweep.
    pub(super) requested_language: Option<FileLanguage>,
    /// Incarnation of the `FileNode` this request was PREPARED against.
    ///
    /// Preparation runs outside `dag.lock()`, so a prepared request can
    /// cross a retirement boundary before it is admitted: a concurrent
    /// `remove()` can install the floor, cancel the DAG and delete the
    /// `FileNode` in the gap. Carrying the incarnation lets the
    /// admission core prove the captured node is still the published one
    /// BEFORE it registers a waiter or admits anything.
    pub(super) prepared_incarnation: u64,
}

/// Work accumulated by the shared admission core that MUST run AFTER
/// the DAG lock releases: deferred dedup callbacks (which may re-enter
/// the scheduler) and auto-ingest-tracking clears (which touch a
/// DashMap that would deadlock against a held DAG lock).
#[derive(Default)]
pub(super) struct AdmissionPostWork {
    /// Deferred dedup-join callbacks collected from
    /// [`SchedulerDag::register_request`].
    pub(super) dedup_events: Vec<DedupJoinerEvent>,
    /// `(canonical, generation)` pairs whose auto-ingest tracking entry
    /// should be cleared once a Source identity has been admitted.
    pub(super) auto_ingest_clears: Vec<(Arc<str>, u64, u64)>,
}

impl AdmissionPostWork {
    /// Fire every deferred dedup callback and clear every recorded
    /// auto-ingest entry. MUST be invoked only after the DAG lock has
    /// been released.
    pub(super) fn run(self, scheduler: &Scheduler) {
        for event in self.dedup_events {
            event.fire();
        }
        for (canonical, incarnation, generation) in self.auto_ingest_clears {
            scheduler.clear_auto_ingest_tracking(&canonical, incarnation, generation);
        }
    }
}

impl Scheduler {
    /// Re-enqueue a waiter token whose gating dep was cancelled.
    /// Looks up the file/generation/profile and re-submits any pending
    /// artifact work through the DAG so it dispatches.
    pub(super) fn requeue_stranded_waiter(&self, _token: crate::dag::SubmissionToken) {
        // Stranded waiter handling: in the legacy path the
        // `remove_file_as_blocker` result was iterated and pending
        // artifacts were re-enqueued via `enqueue_pending_artifacts`.
        // With the DAG, the file's pending artifact waiters at this
        // generation are still in the dag's `file_waiters` map. The
        // dispatch loop will pick them up on the next pass once the
        // dependency gate clears — which `cancel_matching` already
        // did when it dropped the cancelled identity from the
        // waiters reverse-index.
        //
        // We re-trigger the dispatch by sending a Wake into the
        // inbox; the driver picks it up, re-runs the cooperative
        // pump, and the now-ungated artifact nodes go out.
        let _ = self.inbox.sender.try_send(Submission::Wake);
    }

    /// Handle a single new request submission.
    ///
    /// Thin wrapper over the shared admission core: prepare the request
    /// (queued-incarnation gate + live-node lookup) outside the DAG lock, then admit
    /// it under ONE `dag.lock()` acquisition, then fire any deferred
    /// dedup callback + clear auto-ingest tracking after the lock
    /// releases. The single-request and the atomic-batch paths share
    /// the SAME core so their admission semantics cannot drift.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_new_request(
        &self,
        file_id: String,
        target: TargetStage,
        priority: Priority,
        source: Option<Arc<str>>,
        file_language: Option<FileLanguage>,
        sender: CompletionSender<RequestResult>,
        submitted_incarnation: u64,
        request_context: Option<crate::request_context::OpaqueRequestContext>,
    ) {
        let request = QueuedRequest {
            file_id,
            target,
            priority,
            source,
            file_language,
            sender,
            submitted_incarnation,
            request_context,
        };
        // Prepare outside the lock (incarnation gate, live-node lookup — the
        // node `Arc` is cloned out of the `nodes` DashMap and the shard
        // `Ref` dropped BEFORE the lock per the AB-BA rule).
        let Some(prepared) = self.prepare_request(request) else {
            return; // incarnation-rejected; sender already signalled.
        };

        // Admit under ONE DAG lock; collect the deferred dedup event +
        // auto-ingest-clear obligation. Callbacks are NOT fired here.
        let mut post = AdmissionPostWork::default();
        {
            let mut dag = self.dag.lock();
            self.admit_prepared_under_lock(&mut dag, prepared, &mut post);
        }
        post.run(self);
    }

    /// Handle an atomic batch of new request submissions drained as ONE
    /// inbox item.
    ///
    /// All N requests are prepared outside the DAG lock: the node-ensure
    /// step clones each `FileNode` `Arc` out of the `nodes` DashMap and
    /// drops the shard `Ref` BEFORE the lock is taken, so no prepared
    /// request carries a `nodes` `Ref` INTO the critical section. They
    /// are then admitted under a SINGLE `dag.lock()` acquisition:
    /// generation bumps, supersede sweeps, waiter registration, and work
    /// admission for EVERY request happen inside that one critical
    /// section. The pump therefore can never observe the batch
    /// half-admitted.
    ///
    /// The admission core itself may still read the `nodes` shard under
    /// the held DAG lock (the Artifact-gating path in
    /// [`Self::file_stage_analysis_blocker_status`]); that access obeys
    /// the canonical DAG-first order (lock held first, transient read
    /// `Ref` dropped before return, never a `Ref` held across a
    /// `dag.lock()`) — see [`Self::admit_prepared_under_lock`]. Deferred
    /// dedup callbacks and auto-ingest-tracking cleanup run AFTER the
    /// lock releases, keeping the callback-under-lock hazard out of the
    /// batch path exactly as it is kept out of the single-request path.
    pub(super) fn handle_new_request_batch(&self, requests: Vec<QueuedRequest>) {
        if requests.is_empty() {
            return;
        }
        // Prepare every request OUTSIDE the lock. Tombstone-rejected
        // requests signal their sender here and are dropped from the
        // admission set. Node `Arc`s are cloned out of the `nodes`
        // DashMap so no shard `Ref` is held when the lock is taken.
        let prepared: Vec<PreparedRequest> = requests
            .into_iter()
            .filter_map(|req| self.prepare_request(req))
            .collect();
        if prepared.is_empty() {
            return;
        }
        // Admit ALL prepared requests under ONE DAG lock.
        let mut post = AdmissionPostWork::default();
        {
            // Acquire the admission lock through the shared helper so the
            // lock-acquisition epoch is bumped exactly where the lock is
            // taken. A per-item lock/unlock regression would move this
            // acquisition INSIDE the loop and thereby bump the epoch once
            // per item — the LOCK-CONTINUITY rail detects that as differing
            // recorded epochs. In release builds the helper is a plain
            // `self.dag.lock()` with no epoch.
            #[cfg(any(test, feature = "test-support"))]
            let (mut dag, admit_epoch) = self.acquire_dag_for_admission();
            #[cfg(not(any(test, feature = "test-support")))]
            let mut dag = self.dag.lock();
            for (admit_index, p) in prepared.into_iter().enumerate() {
                self.admit_prepared_under_lock(&mut dag, p, &mut post);
                // LOCK-CONTINUITY recording: every admit records the epoch
                // of the acquisition it ran under. The helper asserts all N
                // share ONE epoch (proving one held lock) independently of
                // any thread-scheduling race. Cheap `Option` check when no
                // test recorder is installed; zero footprint outside
                // `cfg(test, feature = "test-support")`.
                #[cfg(any(test, feature = "test-support"))]
                self.record_batch_admit_epoch(admit_epoch);
                // Test-only LOCK-CONTINUITY seam: after the FIRST admit and
                // BEFORE the rest, fire the rendezvous WHILE `dag` is still
                // held. It releases a concurrent observer that BLOCKING-
                // acquires `dag.lock()`; because the loop keeps holding this
                // one lock through the remaining admits, that observer
                // cannot acquire until the lock drops below — at which point
                // the DAG already shows ALL N admitted (the observable
                // consequence of continuity). The guard `dag` is NOT
                // released here. Zero footprint outside
                // `cfg(test, feature = "test-support")`.
                #[cfg(any(test, feature = "test-support"))]
                if admit_index == 0 {
                    self.batch_admit_seam.fire();
                }
                #[cfg(not(any(test, feature = "test-support")))]
                let _ = admit_index;
            }
        }
        // After the lock has dropped: fire deferred dedup callbacks +
        // clear auto-ingest tracking.
        post.run(self);
    }

    /// Acquire the DAG lock for an admission critical section, bumping the
    /// monotonic [`Self::dag_admit_epoch`] exactly at the acquisition
    /// point and returning the new epoch alongside the guard.
    ///
    /// Co-locating the bump with the `lock()` call is what makes the
    /// LOCK-CONTINUITY rail discriminating: a single held lock bumps the
    /// epoch ONCE, so every admit in the batch records the same epoch; a
    /// per-item lock/unlock regression acquires through this helper once
    /// per item, bumping a fresh epoch each time, so the recorded epochs
    /// DIFFER. Test-only; release builds take `self.dag.lock()` directly
    /// with no epoch.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn acquire_dag_for_admission(&self) -> (DagMutexGuard<'_>, u64) {
        let guard = self.dag.lock();
        // `+ 1` so the first acquisition observes epoch 1 (epoch 0 is the
        // "no acquisition yet" sentinel, never a recorded value).
        let epoch = self.dag_admit_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        (guard, epoch)
    }

    /// Record the acquisition epoch an admit ran under, when a test has
    /// installed an epoch recorder via [`Self::test_install_batch_admit_epoch_trace`].
    /// A no-op (single `Option` check) otherwise. Test-only.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn record_batch_admit_epoch(&self, epoch: u64) {
        if let Some(trace) = self.batch_admit_epoch_trace.lock().as_mut() {
            trace.push(epoch);
        }
    }

    /// Pre-lock preparation shared by the single-request and atomic-
    /// batch admission paths.
    ///
    /// Performs the two steps that must NOT run under `dag.lock()`:
    ///
    /// Reject requests whose submitted incarnation is no longer live.
    /// Clone the current node without holding a shard guard across admission.
    /// The under-lock admission pass revalidates it before mutation.
    pub(super) fn prepare_request(&self, request: QueuedRequest) -> Option<PreparedRequest> {
        let QueuedRequest {
            file_id,
            target,
            priority,
            source,
            file_language,
            sender,
            submitted_incarnation,
            request_context,
        } = request;

        // Preparation cannot recreate a node for a retired inbox item.
        let node = match self.nodes.get(&file_id) {
            Some(live) if live.incarnation_id() == submitted_incarnation => {
                Arc::clone(live.value())
            }
            _ => {
                sender.send(CompletionState::Shutdown);
                return None;
            }
        };
        let canonical: Arc<str> = Arc::from(file_id.as_str());

        let prepared_incarnation = node.incarnation_id();
        Some(PreparedRequest {
            file_id,
            canonical,
            node,
            target,
            priority,
            source,
            sender,
            request_context,
            requested_language: file_language,
            prepared_incarnation,
        })
    }

    /// Shared admission core. Runs entirely under the caller-held
    /// `dag.lock()` — the SOLE place where a request's generation is
    /// bumped, the supersede sweep runs, the waiter is registered, and
    /// the work node is admitted. Both [`Self::handle_new_request`] and
    /// [`Self::handle_new_request_batch`] funnel through here so single
    /// and batch admission share identical semantics.
    ///
    /// Side effects that are independent of the DAG mutex (the
    /// `overlay`/`pending_source` writes, the completion-sender signals,
    /// the `set_target` stamp) run inline; they take their own
    /// independent locks (the overlay DashMap shard, the handle's inner
    /// mutex), so they are safe under the DAG lock.
    ///
    /// `nodes`-shard lock ordering under the DAG lock. The admission
    /// core DOES read the `nodes` shard while the caller holds
    /// `dag.lock()` — the Artifact-gating path reaches
    /// [`Self::file_stage_analysis_blocker_status`] (via
    /// [`Self::admit_artifact_with_blockers`] →
    /// [`Self::classify_recorded_dep`]), which calls `self.nodes.get(..)`
    /// to read the producer FileNode's generation/commit state. That is
    /// safe NOT because admission avoids the `nodes` shard, but because
    /// the access obeys the crate's canonical **DAG-first** lock order:
    /// the `dag.lock()` is acquired FIRST (by the caller), then a
    /// TRANSIENT read `Ref` is taken on the `nodes` shard and dropped at
    /// every return arm. No path here takes a `nodes`/shard `Ref` and
    /// THEN acquires `dag.lock()` — that inversion (`nodes` then `dag`)
    /// is the AB-BA hazard, and it is structurally absent because the
    /// caller already holds the DAG lock. The shard `Ref` is never held
    /// across a `dag.lock()` acquisition.
    ///
    /// Deferred work that MUST happen after the lock releases (firing
    /// dedup callbacks, clearing auto-ingest tracking on a DashMap) is
    /// pushed onto `post` rather than executed here.
    pub(super) fn admit_prepared_under_lock(
        &self,
        dag: &mut SchedulerDag,
        prepared: PreparedRequest,
        post: &mut AdmissionPostWork,
    ) {
        let PreparedRequest {
            file_id,
            canonical,
            node: _captured_node,
            target,
            priority,
            source,
            sender,
            request_context,
            requested_language,
            prepared_incarnation,
        } = prepared;

        // Language re-home, under the caller-held DAG lock.
        //
        // The node's language routes the Source stage's parse dispatch,
        // so a request whose resolved language differs must move the
        // file onto a fresh node — executing with the stale row would
        // parse through the wrong implementation (or keep failing a
        // request whose language changed back to a supported row).
        //
        // That move ADVANCES a published file's generation, so it obeys
        // the same rule as `invalidate()` and `close_file()`: the
        // advance and the supersede sweep form ONE critical section.
        // Done during preparation instead — outside this lock, with no
        // sweep at all — every identity admitted at the old generation
        // was orphaned (unreachable by any later sweep, skipped at
        // dispatch on the generation-mismatch arm, its parked capacity
        // reservation never released) and the old generation's waiters
        // were left parked.
        //
        // Lock order is the canonical DAG-first one: the caller holds
        // `dag.lock()`, and the `nodes` read/write below is transient.
        //
        // CROSSING GATE — runs before ANY publication (no waiter
        // registration, no admission, no generation bump).
        //
        // Removal can retire and unpublish the prepared object before
        // this hold. Its retained Arc cannot authorize a replacement,
        // even if the replacement has the same generation.
        //
        // So the prepared request must prove the node it captured is
        // still the published one. Terminalize the sender here rather
        // than registering a waiter that nothing can complete.
        // Declared here and assigned ONLY from the live map. That is a
        // CONVENTION, not enforcement: `_captured_node` remains an ordinary
        // readable binding and the leading underscore only suppresses an
        // unused-variable lint. Deleting the field outright is the structural
        // fix, and that work is owned by
        // `.claude/skills/scheduler/SKILL.md`.
        let mut node: Arc<FileNode>;
        match self.nodes.get(&file_id) {
            Some(live) if live.incarnation_id() == prepared_incarnation => {
                node = Arc::clone(live.value());
            }
            Some(live) => {
                // A different incarnation is published: this request was
                // prepared against a node that has since been replaced.
                //
                // Drop the shard READ guard BEFORE signalling. `let _ = live`
                // does NOT drop it — a wildcard pattern neither moves nor
                // drops a place expression — so the guard would be held across
                // the send.
                drop(live);
                sender.send(CompletionState::Superseded);
                return;
            }
            None => {
                // The file was removed while this request was in flight.
                // `remove()` signals Shutdown to the waiters it can see;
                // this one had not registered yet, so it is signalled
                // here instead of being left parked.
                sender.send(CompletionState::Shutdown);
                return;
            }
        }
        if let Some(requested) = requested_language {
            if node.file_language != requested {
                node.retire(dag);
                let fresh =
                    self.create_node_at_least(&file_id, Some(requested), node.generation() + 1);
                let fresh_gen = fresh.generation();
                // Publishing the replacement node and its `Absent`
                // source version under ONE publication hold keeps a
                // captured root from ever seeing the fresh incarnation
                // published while the root still answers with the old
                // one's committed source.
                self.source_root.publish_transition(|publication| {
                    self.nodes.insert(file_id.clone(), Arc::clone(&fresh));
                    publication.absent(&canonical, fresh.incarnation_id(), fresh_gen);
                });
                // Retire the old generation's DAG identities and signal
                // its waiters `Superseded`.
                dag.supersede_old_file_generations(&canonical, fresh_gen);
                node = fresh;
            }
        }

        // Generation: bump under the lock so the bump and the supersede
        // sweep form one critical section. A bare-atomic bump separated
        // from the lock would let a dispatcher observe the bumped
        // generation BEFORE the supersede sweep cancelled the stale
        // identity — the dispatch-time defensive `debug_assert!` would
        // then trip on a not-yet-terminalized stale identity.
        let incarnation = node.incarnation_id();
        let generation = if source.is_some() {
            // Bump + publish atomically; see [`Self::invalidate`]. The
            // new generation has no committed snapshot yet, so the
            // file's published source state is `Absent` until the Source
            // stage commits one.
            let gen = self.source_root.publish_transition(|publication| {
                let gen = publication.bump_node_generation(&node);
                publication.absent(&canonical, node.incarnation_id(), gen);
                gen
            });
            // Store source in the overlay for SourceLoader access.
            if let Some(ref src) = source {
                self.overlay.set(file_id.clone(), Arc::clone(src));
            }
            // Store in pending_source for the Source job.
            node.pending_source
                .store(Arc::new(source.map(|s| (gen, s))));
            dag.supersede_old_file_generations(&canonical, gen);
            gen
        } else {
            let gen = node.generation();
            if gen == 0 {
                // Node was just created, needs a Source job. No
                // supersede sweep is needed at generation 0 — there is
                // no prior dispatched identity. The bump still takes
                // the publication capability so a lock-free atomic
                // cannot race a concurrent publish.
                self.source_root
                    .publish_transition(|publication| publication.bump_node_generation(&node))
            } else {
                gen
            }
        };

        // Already-satisfied short-circuit. Note: `current_*` is
        // generation-coherent, so after a source-update bump these
        // return `None` and the request always admits a fresh Source.
        let already_satisfied = match &target {
            TargetStage::Source => node.current_integrated_source().is_some(),
            TargetStage::Analysis => node.current_analysis().is_some(),
            TargetStage::Artifact { profile_hash } => {
                node.current_artifact(*profile_hash).is_some()
            }
        };
        if already_satisfied {
            let result = match &target {
                TargetStage::Source => {
                    RequestResult::Source(node.current_integrated_source().unwrap())
                }
                TargetStage::Analysis => RequestResult::Analysis(node.current_analysis().unwrap()),
                TargetStage::Artifact { profile_hash } => {
                    RequestResult::Artifact(node.current_artifact(*profile_hash).unwrap())
                }
            };
            sender.send(CompletionState::Ready(result));
            return;
        }

        // Determine the first-missing work stage BEFORE registering the
        // sender, then stamp its concrete `Work` identity on the
        // sender's target so the cooperative pump's same-path
        // self-await detection matches by the exact `WorkNodeIdentity`
        // once admission has run. (See `CompletionTarget` docs for the
        // Request-shape fallback that covers the race window before
        // this stamp lands.)
        let first_missing = if node.current_integrated_source().is_none() {
            TaskKind::Load
        } else if node.current_analysis().is_none() {
            TaskKind::Analysis
        } else {
            target.required_task_kind()
        };
        let first_missing_identity: WorkNodeIdentity = match &first_missing {
            // The first missing stage is one of the request-target stages
            // (`Load` for Source, `Analysis`, or `Artifact`); `Parse` and
            // `CacheNode` are never produced by `required_task_kind`.
            TaskKind::Load => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(&canonical),
                incarnation,
                generation,
                stage: FileStageKey::Source,
            },
            TaskKind::Analysis => WorkNodeIdentity::FileStage {
                canonical: Arc::clone(&canonical),
                incarnation,
                generation,
                stage: FileStageKey::Analysis,
            },
            TaskKind::Artifact { profile_hash } => WorkNodeIdentity::Artifact {
                canonical: Arc::clone(&canonical),
                incarnation,
                generation,
                profile_hash: profile_hash_to_bytes(*profile_hash),
                content_hash: [0u8; 16],
            },
            TaskKind::Parse | TaskKind::CacheNode { .. } => unreachable!(
                "first-missing-stage planning only yields Load/Analysis/Artifact; \
                 Parse and CacheNode are not request-target stages"
            ),
        };
        sender.set_target(crate::job::CompletionTarget::Work(
            first_missing_identity.clone(),
        ));

        // Register the waiter group. The dedup callback (if any) is
        // returned as a deferred event and fired after the lock drops.
        let dedup_event = dag.register_request(
            &canonical,
            generation,
            target.clone(),
            sender,
            request_context,
        );
        if let Some(event) = dedup_event {
            post.dedup_events.push(event);
        }

        let effective_priority = priority;

        // If the next missing stage is an Artifact gated behind a
        // still-pending Analysis node at this generation, do NOT admit
        // the Artifact yet — the DAG's dep edge drives re-dispatch when
        // the gate clears. Propagate the new request's priority onto the
        // gate instead.
        let analysis_gate = WorkNodeIdentity::FileStage {
            canonical: Arc::clone(&canonical),
            incarnation,
            generation,
            stage: FileStageKey::Analysis,
        };
        if matches!(first_missing, TaskKind::Artifact { .. })
            && dag.has_pending_deps(&analysis_gate)
        {
            dag.upgrade_priority(&analysis_gate, effective_priority);
            return;
        }

        // Artifact admissions inherit any blockers persisted to the
        // per-canonical Artifact blocker registry at this
        // `(file_id, generation)` (owner Analysis is admitted ungated
        // for macro_type_deps).
        if let TaskKind::Artifact { profile_hash } = &first_missing {
            if self
                .admit_artifact_with_blockers(
                    dag,
                    &canonical,
                    incarnation,
                    generation,
                    *profile_hash,
                    effective_priority,
                    None,
                )
                .is_none()
            {
                // Refused by the live object witness. The waiter group is
                // already registered, so an ignored refusal is a hang:
                // nothing will ever produce this identity. Terminalize
                // the group instead of leaving it parked.
                Self::terminalize_refused_admission(dag, &canonical, generation);
            }
            return;
        }

        if matches!(first_missing, TaskKind::Analysis) {
            match self.ensure_analysis_for_demand(
                dag,
                &canonical,incarnation,
                generation,
                effective_priority,
                AnalysisDemandKind::DirectRequest,
            ) {
                BlockerStatus::Gating | BlockerStatus::Satisfied => {}
                BlockerStatus::Failed(_) => unreachable!(
                    "direct Analysis demand retries the producer and never consumes blocker failure state"
                ),
            }
            return;
        }

        // A Source admission (the `Load` first-missing stage) transitions the
        // dep from "queued in inbox" to "Source DAG identity admitted"; the
        // matrix must stop treating it as a pending auto-ingest. Capture the
        // discriminant before `admit_work` consumes `first_missing` by value.
        let is_source_admission = matches!(first_missing, TaskKind::Load);
        if admit_work(
            dag,
            &node,
            &canonical,
            generation,
            first_missing,
            effective_priority,
            None,
        )
        .is_none()
        {
            // See above: a refused admission with a registered waiter is
            // a hang unless the group is terminalized here.
            Self::terminalize_refused_admission(dag, &canonical, generation);
            return;
        }
        // The clear touches a DashMap, so it is deferred until the DAG lock
        // releases (it would otherwise deadlock against anyone holding the
        // DAG lock and waiting on that shard).
        if is_source_admission {
            post.auto_ingest_clears
                .push((Arc::clone(&canonical), incarnation, generation));
        }
    }

    /// Signal every waiter group registered at `(canonical, generation)`
    /// when the admission that was supposed to produce their result was
    /// refused by the live object witness.
    ///
    /// Registration happens before admission, so a refusal that returns
    /// `None` and is ignored leaves a group waiting on work no producer
    /// will ever run. `Shutdown` is the same terminal `remove()` uses for
    /// waiters it can see; this covers the ones it could not.
    pub(super) fn terminalize_refused_admission(
        dag: &mut SchedulerDag,
        canonical: &Arc<str>,
        generation: u64,
    ) {
        dag.signal_file_shutdown_at(canonical, generation);
    }

    /// Admit pending Artifact work nodes for a file that has cleared
    /// Analysis and dependency gating.
    ///
    /// Each Artifact admission inherits any late-discovered blockers
    /// recorded by [`Self::register_resolved_deps`] via
    /// [`Self::admit_artifact_with_blockers`] so a blocker that
    /// arrived AFTER the file's Analysis dispatched still gates the
    /// Artifact until its target Analysis completes.
    pub(super) fn admit_pending_artifacts(
        &self,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        inherited_priority: Priority,
    ) {
        // ONE uninterrupted hold covering the profile snapshot AND every
        // admission it drives.
        //
        // Snapshotting under one lock, releasing, then re-locking per
        // profile reopened the same window the Source→Analysis publish
        // closes: an `invalidate()` landing in the gap bumps to G+1 and
        // runs its supersede sweep, and the loop then admits Artifact-G
        // AFTER that backward-looking sweep — an identity no sweep can
        // ever reach. Dispatch reserves capacity for it, observes the
        // generation mismatch, and skips; the skip never releases the
        // reservation, so the DAG ledger leaks. Holding the lock across
        // both phases makes the bump impossible mid-loop, since
        // `invalidate()` must take the same lock.
        let mut dag = self.dag.lock();
        // The generation must still be live as of THIS acquisition: the
        // caller computed it before the lock was taken.
        if !self
            .nodes
            .get(&**canonical)
            .is_some_and(|n| n.incarnation_id() == incarnation && n.generation() == generation)
        {
            return;
        }
        let profiles: Vec<(u64, Priority)> = dag.pending_artifact_profiles(canonical, generation);
        for (profile_hash, priority) in profiles {
            // Inherited priority is the file-level urgency; the
            // per-waiter priority is what the dag bookkeeping returned.
            let effective = std::cmp::min(priority, inherited_priority);
            self.admit_artifact_with_blockers(
                &mut dag,
                canonical,
                incarnation,
                generation,
                profile_hash,
                effective,
                None,
            );
        }
    }
}
