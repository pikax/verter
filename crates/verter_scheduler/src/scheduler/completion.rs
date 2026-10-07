//! Terminal states — completion integration, failure fan-out and same-path refusal.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

/// RAII guard owned by the cooperative pump's inline-execute
/// branch. Selects between INSTALLING a winner-provided request
/// context and CLEARING the outer worker's TLS so the inner stage
/// runs under the correct attribution.
///
/// Both variants restore the prior TLS slot on drop. Two arms are
/// required because the install path returns a trait-object
/// `Box<dyn TlsUninstall>` (the concrete guard lives in the
/// session crate and isn't visible to the scheduler), while the
/// clear path is a concrete `OpaqueContextGuard` that owns the
/// prior value directly.
///
/// Constructed only on the native inline-execution path.
#[cfg(not(target_arch = "wasm32"))]
pub(super) enum InlineTlsGuard {
    /// Winner has its own request context; install it for the
    /// inner stage. Drop restores the prior TLS via the trait
    /// object's underlying guard.
    Install(#[allow(dead_code)] Box<dyn verter_execution::request_context::TlsUninstall + Send>),
    /// Winner has no context; clear ALL install_tls slots (scheduler
    /// opaque, session request context + accumulator, audit observer)
    /// so the inner stage observes `None` everywhere the outer
    /// stage's `install_tls` would have planted state. Drop restores
    /// every prior outer TLS slot via `AllSlotsClearGuard::Drop`.
    ClearAll(#[allow(dead_code)] verter_execution::request_context::AllSlotsClearGuard),
}

/// Re-reads the handle's current state and current target slot,
/// then either returns a real terminal `CompletionState` or
/// synthesizes a same-path `Failed(StageFailed)` when the handle's
/// target is on the calling thread's active path.
///
/// Centralizes three invariants:
/// - try_get is consulted FIRST so a resolved handle returns its
///   real terminal state instead of being masked by the synthetic
///   same-path Failed.
/// - The active-path probe matches the full prerequisite-stage
///   chain:
///     * Source request → matches an active Source frame on the
///       same canonical.
///     * Analysis request → matches an active Source OR Analysis
///       frame on the same canonical.
///     * Artifact request → matches an active Source OR Analysis
///       frame on the same canonical, OR an active Artifact frame
///       on the same canonical AND the same `profile_hash`. Two
///       Artifact frames for the same canonical with different
///       profiles are independent work units (they share only the
///       upstream Analysis gate, not the Artifact slot itself) and
///       must NOT collapse into a same-path match.
/// - try_get is re-checked IMMEDIATELY before synthesizing the
///   Failed so a handle that resolves during the active-path
///   probe still surfaces its real terminal state.
///
/// Re-readable across the cooperative loop: each call observes a
/// fresh `handle.try_get()` and `handle.target()`. The target slot
/// is mutated by `handle_new_request` admission (Request → Work),
/// so the cooperative pump re-runs this helper on every iteration
/// to pick up the late-stamped Work identity that the loop-entry
/// read missed.
pub(super) fn check_terminal_or_same_path<T: Clone>(
    handle: &crate::job::CompletionHandle<T>,
) -> Option<crate::job::CompletionState<T>> {
    use crate::job::{CompletionState, CompletionTarget, SchedulerError};
    if let Some(state) = handle.try_get() {
        return Some(state);
    }
    let target = handle.target()?;
    let on_active_path = match &target {
        CompletionTarget::Work(id) => caller_kind::active_path_contains_work(id),
        CompletionTarget::Request { canonical, target } => {
            caller_kind::active_path_contains_request(canonical.as_ref(), target.clone())
        }
    };
    if !on_active_path {
        return None;
    }
    // Test-only hook: fires between the active-path probe and the
    // inner try_get re-check. The hook lets tests deterministically
    // exercise the inner re-check (which otherwise sits in a tiny
    // race window between the active-path computation and the
    // synthetic-Failed synthesis) by resolving the handle from
    // outside the helper. Production builds compile without the
    // hook.
    #[cfg(test)]
    check_terminal_or_same_path_test_hook();
    // Same-path match: re-check try_get RIGHT before synthesizing
    // Failed so a handle that resolved during the active-path
    // probe surfaces its real terminal state. Without this
    // re-check, a Ready/Failed/Superseded/Shutdown that landed
    // between the entry try_get and here would be masked by the
    // synthetic `Failed(StageFailed { stage: "wait_or_drive" })`.
    if let Some(state) = handle.try_get() {
        return Some(state);
    }
    let file_id = match &target {
        CompletionTarget::Work(id) => identity_canonical(id),
        CompletionTarget::Request { canonical, .. } => canonical.to_string(),
    };
    Some(CompletionState::Failed(SchedulerError::StageFailed {
        file_id,
        stage: "wait_or_drive".into(),
        message: "same-path self-await detected".into(),
    }))
}

// Test-only hook installer. Lets a test plant a closure that
// fires between the active-path probe and the inner try_get
// re-check inside `check_terminal_or_same_path`, so the test
// can resolve the handle from outside the helper at exactly the
// right point and assert that the inner re-check observes the
// resolved state instead of synthesizing a Failed.
//
// The hook is thread-local (no global mutable state across
// tests) and clears itself on drop. Production builds compile
// without the hook field at all.
#[cfg(test)]
thread_local! {
    static CHECK_TERMINAL_HOOK: std::cell::RefCell<
        Option<Box<dyn FnMut() + Send>>,
    > = std::cell::RefCell::new(None);
}

#[cfg(test)]
pub(super) fn check_terminal_or_same_path_test_hook() {
    CHECK_TERMINAL_HOOK.with(|cell| {
        if let Some(hook) = cell.borrow_mut().as_mut() {
            hook();
        }
    });
}

/// RAII guard that installs `hook` as the test-only intercept
/// between the active-path probe and the inner try_get re-check.
/// Restores the previous (typically `None`) on drop.
#[cfg(test)]
pub(crate) struct CheckTerminalHookGuard {
    pub(super) prev: Option<Box<dyn FnMut() + Send>>,
}

#[cfg(test)]
impl CheckTerminalHookGuard {
    pub(crate) fn install(hook: Box<dyn FnMut() + Send>) -> Self {
        let prev = CHECK_TERMINAL_HOOK.with(|cell| cell.replace(Some(hook)));
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for CheckTerminalHookGuard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CHECK_TERMINAL_HOOK.with(|cell| {
            cell.replace(prev);
        });
    }
}

impl Scheduler {
    /// Deliver a stage's terminal `StageComplete` from a thread that holds
    /// the scheduler: the inline pump, or a pool worker that inline-ran a
    /// dependency. Routing through [`Self::send_submission`] lets a sole
    /// inbox consumer drain an older submission instead of parking on its
    /// own full inbox.
    pub(super) fn deliver_stage_completion(&self, completion: Option<Submission>) {
        if let Some(completion) = completion {
            let _ = self.send_submission(completion);
        }
    }

    /// Deliver a stage's terminal `StageComplete` from a pool task. A pool
    /// worker is never the inbox's consumer, so it waits for capacity while
    /// the driver drains.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn deliver_stage_completion_from_pool(
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        completion: Option<Submission>,
    ) {
        if let Some(completion) = completion {
            let _ = inbox_sender.send(completion);
        }
    }

    /// Number of stage completions refused because the owning
    /// `FileNode` moved between dispatch and publish. Expected to stay
    /// ZERO absent concurrent invalidation; the refusal is otherwise
    /// silent, so this is how a test observes that the gate fired.
    ///
    /// The counter itself is incremented in ALL builds — the refusal
    /// path is release-active. Only this reader is `cfg`-gated; nothing
    /// outside the tests consumes it today, so it is `cfg(test)` rather
    /// than carried as dead weight into shipped builds.
    #[cfg(test)]
    pub(crate) fn stale_completion_refusals(&self) -> u64 {
        self.stale_completion_refusals.load(Ordering::Relaxed)
    }

    /// Refuse a stage completion whose generation has been retired.
    ///
    /// Publishes NOTHING and releases the dequeued identity so its
    /// parked capacity reservation returns through the DAG's cancel
    /// path — the very path the dispatch-time defensive skip documents
    /// as its own safety condition. `cancel` is idempotent: when the
    /// supersede sweep already retired the identity this is a no-op
    /// returning no stranded tokens. It cannot touch a LATER
    /// generation's work because `WorkNodeIdentity::FileStage` carries
    /// the generation, so `Source-G` and `Source-(G+1)` are distinct
    /// keys — which is what keeps this from re-creating the
    /// superseded-teardown hang from the other direction.
    ///
    /// Returns the stranded waiter tokens for the caller to requeue
    /// AFTER the DAG lock drops.
    ///
    /// Release stays silent and non-leaking; the `debug_assert!` runs
    /// strictly AFTER the cleanup so debug builds stay loud without
    /// ever skipping the release.
    pub(super) fn refuse_stale_stage_completion(
        &self,
        dag: &mut SchedulerDag,
        canonical: &Arc<str>,
        generation: u64,
        identity: &WorkNodeIdentity,
    ) -> Vec<crate::dag::SubmissionToken> {
        let existed = dag.token_for(identity).is_some();
        let stranded = dag.cancel(identity);
        // A refusal must never leave a request group parked forever.
        //
        // Normally the event that retired this generation — an
        // `invalidate`, a `close_file`, or a language re-home — ran a
        // supersede sweep, and that sweep already drained this
        // generation's waiter groups (signalling them `Superseded`), so
        // the call below finds nothing and is a no-op. But a refusal
        // must be safe even when no sweep accompanied the change: the
        // waiters would otherwise wait on a completion that has just
        // been refused and will never be republished. Signalling them is
        // idempotent and strictly bounded to the retired generation, so
        // it cannot disturb a newer one.
        if existed {
            dag.signal_file_failed(
                canonical,
                generation,
                crate::job::SchedulerError::StageFailed {
                    file_id: canonical.to_string(),
                    stage: "stage_complete".into(),
                    message: "stage completion refused: the file moved to a different \
                          incarnation or generation while the stage was in flight"
                        .into(),
                },
            );
        }
        #[cfg(any(test, feature = "semantic-observe"))]
        self.stale_completion_refusals
            .fetch_add(1, Ordering::Relaxed);
        verter_debug_assert!(
            dag.token_for(identity).is_none(),
            "refused stage completion must leave no live DAG token for the retired \
             identity: {identity:?}",
        );
        stranded
    }

    /// Handle a stage completion.
    pub(super) fn handle_stage_complete(
        &self,
        file_id: &str,
        generation: u64,
        task_kind: TaskKind,
        incarnation: u64,
    ) {
        let node = match self.nodes.get(file_id) {
            Some(n) => n.clone(),
            None => return,
        };

        if node.incarnation_id() != incarnation || node.generation() != generation {
            // The file already moved on before this completion even
            // began — a newer generation, or a different node object
            // entirely. Release the dequeued identity rather than
            // returning and leaving its reservation parked.
            let canonical_arc: Arc<str> = Arc::from(file_id);
            if let Some(identity) =
                Self::dispatched_identity_for(&canonical_arc, incarnation, generation, &task_kind)
            {
                let stranded = {
                    let mut dag = self.dag.lock();
                    self.refuse_stale_stage_completion(
                        &mut dag,
                        &canonical_arc,
                        generation,
                        &identity,
                    )
                };
                for tok in stranded {
                    self.requeue_stranded_waiter(tok);
                }
            }
            return;
        }

        let canonical_arc: Arc<str> = Arc::from(file_id);
        let inherited_priority = self
            .dag
            .lock()
            .highest_priority_for_file(&canonical_arc, generation)
            .unwrap_or(Priority::Background);

        match &task_kind {
            // The source stage completes under the `Load` label (the live
            // `FileStage{Source}` node maps to `Load`).
            TaskKind::Load => {
                // The executor call is the ONLY part of this completion
                // that runs unlocked. It is pure computation over the
                // committed snapshot — it publishes nothing and consumes
                // nothing — and it must not hold the DAG mutex, since it
                // reaches into host code of unbounded cost.
                let extracted = node
                    .current_source()
                    .map(|source| self.executor.extract_deps(file_id, &source));

                // Resolve dep languages HERE, outside the hold.
                // `create_node(_, None)` falls through to
                // `source_loader.classify(..)`, a `dyn SourceLoader` seam into
                // host code — the same reason `extract_deps` is hoisted above.
                // Auto-ingest runs under the DAG lock AND inside a `nodes`
                // shard-WRITE guard, so calling it there would hold two locks
                // across a host callback, which the base revision never did.
                //
                // Both blocker sources must be covered.
                //
                // Extractor-supplied ids are classified here. Deferred ids
                // carry their language with them, resolved at their write
                // site: `register_resolved_deps` stores blockers and can then
                // return EARLY (generation 0, or Source not yet committed)
                // BEFORE its node-ensure pass runs, so a deferred id can and
                // does arrive here with no `FileNode`. Assuming otherwise
                // silently skipped creation for deferred-only blockers — the
                // absent generation-0 node reads as `Satisfied`, so no Load
                // was admitted and the owner's Artifact proceeded ungated.
                // `extract_deps` omits bare/aliased deps by design, so that is
                // the ordinary externally-resolved case, not a corner.
                let mut dep_languages: std::collections::HashMap<String, FileLanguage> = extracted
                    .as_ref()
                    .map(|deps| {
                        deps.blocker_ids
                            .iter()
                            .map(|id| (id.clone(), self.source_loader.classify(id)))
                            .collect()
                    })
                    .unwrap_or_default();

                // ── Single linearization point ──
                //
                // EVERY publish AND every state consumption this
                // completion performs — forward edges, the deferred
                // blocker drain, dependency auto-ingest and admission,
                // the Artifact blocker registry write, the Analysis
                // admission, and `complete(Source-G)` — happens under
                // ONE `dag.lock()` hold, gated on a re-validation of the
                // incarnation and generation.
                //
                // The extraction above takes real time, so an
                // `invalidate()` / `close_file()` / language re-home can
                // retire this generation inside that window. Nothing may
                // be published on the strength of the entry check alone:
                // the supersede sweep is purely backward-looking and can
                // never cancel an identity admitted after it ran, so a
                // stale admission would be unreachable by any sweep —
                // dispatch would skip it on the generation-mismatch arm
                // and its parked capacity reservation would never be
                // released.
                //
                // CONSUMING state is just as unsafe as publishing it. A
                // deferred blocker registered for a LATER generation and
                // drained here by a stale completion would be discarded,
                // letting that generation's Artifact work run ungated;
                // and stale forward edges would persist, because a later
                // extraction unions with the existing set rather than
                // replacing it.
                let source_id = WorkNodeIdentity::FileStage {
                    canonical: Arc::clone(&canonical_arc),
                    incarnation,
                    generation,
                    stage: FileStageKey::Source,
                };
                let stranded = {
                    let mut dag = self.dag.lock();
                    if !self.stage_completion_is_current(file_id, incarnation, generation) {
                        // Retired mid-flight: publish NOTHING and consume
                        // NOTHING — no forward edges, no deferred-blocker
                        // drain, no dependency admission, no blocker
                        // records, no Analysis, no Source completion.
                        // Release the dequeued identity instead.
                        self.refuse_stale_stage_completion(
                            &mut dag,
                            &canonical_arc,
                            generation,
                            &source_id,
                        )
                    } else {
                        if let Some(deps) = extracted {
                            // Merge extract_deps output with any exact-resolved bare deps
                            let mut new_deps = self.edges.get_forward_deps(file_id);
                            new_deps.extend(deps.forward_deps);
                            self.edges.record_forward_deps(file_id, new_deps);

                            // Merge any deferred bare/aliased blocker IDs.
                            let mut all_blocker_ids = deps.blocker_ids;
                            if let Some((_, deferred)) = self.deferred_blocker_ids.remove(file_id) {
                                for (dep_id, dep_language) in deferred {
                                    dep_languages.insert(dep_id.clone(), dep_language);
                                    all_blocker_ids.push(dep_id);
                                }
                            }

                            // Register blockers for deps that haven't reached Analysis yet.
                            if !all_blocker_ids.is_empty() {
                                let mut dep_keys: Vec<DepKey> = Vec::new();
                                // Failed-dep records collected from the 3-state
                                // matrix below. These ride together with
                                // `dep_keys` inside the per-canonical
                                // `PendingBlockerSet` recorded for the owner's
                                // Artifact admission. They surface as a typed
                                // `DependencyFailed` on the FIRST Artifact
                                // dispatch (via the drain + `attach_failed_dep`
                                // sequence in
                                // [`Self::admit_artifact_with_blockers`]),
                                // matching the scheduler contract that missing
                                // macro_type_deps gate the owner's Artifact
                                // (codegen consumes resolved type shapes) and
                                // never the owner's Analysis (the template /
                                // script analysis must publish for diagnostics,
                                // hover, and `defineSlots` consumers even when
                                // the type dep is unresolved).
                                let mut failed_records: Vec<crate::dag::FailedDepRecord> =
                                    Vec::new();
                                for dep_id in &all_blocker_ids {
                                    let parent_ctx =
                                        dag.winner_context_for(&canonical_arc, generation);

                                    // Atomically ENSURE the dep node exists —
                                    // never replace one. A `contains_key` test
                                    // followed by an unconditional `insert` is a
                                    // check-then-act: a concurrent creator of the
                                    // same file would have its FileNode replaced
                                    // at the same generation, orphaning the
                                    // incarnation any already-dispatched work ran
                                    // against. The vacant entry holds the shard
                                    // lock, so a concurrent creator either wins
                                    // (we observe it and reuse it) or waits.
                                    if let Some(dep_language) = dep_languages.get(dep_id) {
                                        let dep_node = Arc::clone(
                                            self.nodes
                                                .entry(dep_id.clone())
                                                .or_insert_with(|| {
                                                    self.create_node_at(
                                                        dep_id,
                                                        Some(dep_language.clone()),
                                                        1,
                                                    )
                                                })
                                                .value(),
                                        );
                                        if dep_node.source_admission_pending()
                                            && dep_node.current_source().is_none()
                                        {
                                            let dep_canonical: Arc<str> =
                                                Arc::from(dep_id.as_str());
                                            let dep_gen = if dep_node.generation() == 0 {
                                                self.source_root.publish_transition(|publication| {
                                                    let generation =
                                                        publication.bump_node_generation(&dep_node);
                                                    publication.absent(
                                                        &dep_canonical,
                                                        dep_node.incarnation_id(),
                                                        generation,
                                                    );
                                                    generation
                                                })
                                            } else {
                                                dep_node.generation()
                                            };
                                            admit_work(
                                                &mut dag,
                                                &dep_node,
                                                &dep_canonical,
                                                dep_gen,
                                                TaskKind::Load,
                                                std::cmp::min(
                                                    inherited_priority,
                                                    Priority::Interactive,
                                                ),
                                                parent_ctx,
                                            );
                                        }
                                    }

                                    // Route the blocker through the shared 3-state
                                    // classifier:
                                    //
                                    // - `Gating`    → record the DepKey for the
                                    //                 owner's Artifact registry
                                    //                 (gates Artifact admission
                                    //                 only, never Analysis).
                                    // - `Satisfied` → drop silently (producer is
                                    //                 moot or already committed).
                                    // - `Failed(r)` → drop from `dep_keys` AND
                                    //                 collect the record for the
                                    //                 registry's `failed` list.
                                    //                 The Artifact admission
                                    //                 re-classifies every
                                    //                 persisted failure against
                                    //                 the live state on each
                                    //                 drain, so a same-gen
                                    //                 recovery still re-promotes
                                    //                 the dep to gating.
                                    let dep_canonical: Arc<str> = Arc::from(dep_id.as_str());
                                    let Some(dep_node) =
                                        self.nodes.get(dep_id).map(|n| Arc::clone(n.value()))
                                    else {
                                        continue;
                                    };
                                    let dep_gen = dep_node.generation();
                                    let dep_incarnation = dep_node.incarnation_id();
                                    let status = self.ensure_analysis_for_demand(
                                        &mut dag,
                                        &dep_canonical,
                                        dep_incarnation,
                                        dep_gen,
                                        std::cmp::min(inherited_priority, Priority::Interactive),
                                        AnalysisDemandKind::ArtifactBlocker,
                                    );
                                    match status {
                                        BlockerStatus::Satisfied => continue,
                                        BlockerStatus::Failed(record) => {
                                            failed_records.push(record);
                                            continue;
                                        }
                                        BlockerStatus::Gating => {
                                            dep_keys.push(DepKey::FileStage {
                                                canonical: dep_canonical,
                                                incarnation: dep_incarnation,
                                                generation: dep_gen,
                                                stage: FileStageKey::Analysis,
                                            });
                                        }
                                    }
                                }

                                if !dep_keys.is_empty() || !failed_records.is_empty() {
                                    // Record the macro_type_dep blocker set on the
                                    // per-canonical Artifact registry. The owner's
                                    // Analysis stays UNGATED — analysis is
                                    // recoverable from the source alone (templates,
                                    // defineSlots, script-level diagnostics all
                                    // derive from the parsed source independently
                                    // of resolved type shapes). Codegen, however,
                                    // needs the resolved type shapes, so the gate
                                    // fires at Artifact admission via
                                    // [`Self::admit_artifact_with_blockers`].
                                    //
                                    // The macro-type cycle filter and the registry
                                    // write share this guard, so the filter's
                                    // bounded reachability check and the write stay
                                    // atomic — no other thread can observe a state
                                    // where two mutually cyclic deps both pass the
                                    // filter. The chokepoint lives in
                                    // [`Self::filter_macro_cycle_deps`].
                                    let (filtered_deps, _dropped_deps) =
                                        Self::filter_macro_cycle_deps(
                                            &dag,
                                            &canonical_arc,
                                            incarnation,
                                            generation,
                                            dep_keys,
                                        );
                                    let pending_set = crate::dag::PendingBlockerSet {
                                        deps: filtered_deps.into_iter().collect(),
                                        failed: failed_records,
                                    };
                                    dag.record_artifact_blockers(
                                        &canonical_arc,
                                        generation,
                                        pending_set,
                                    );
                                }
                            }
                        }
                        // The worker publishes snapshot bytes before this
                        // completion is drained. Advance the explicit
                        // integration fence only after every dependency fact
                        // and blocker registration above is complete under
                        // this same scheduler-state hold.
                        verter_debug_assert!(
                            node.mark_source_integrated(generation),
                            "current Source completion must publish its integration fence",
                        );

                        // Resolve Source-targeted waiters only after this
                        // transition has published and consumed all dependency
                        // facts. Signalling from the worker immediately after
                        // storing the Source snapshot let callers observe a
                        // half-integrated generation.
                        let source = node.current_integrated_source().expect(
                            "current Source completion must retain its integrated snapshot",
                        );
                        let result = RequestResult::Source(source);
                        dag.signal_stage_complete(
                            &canonical_arc,
                            incarnation,
                            generation,
                            &TaskKind::Load,
                            &result,
                        );

                        // Source → Analysis is demand-driven. A Source-only
                        // request has reached its target; a later Analysis or
                        // Artifact request observes the committed Source and
                        // admits the missing stage through normal admission.
                        let requires_analysis =
                            dag.has_analysis_demand(&canonical_arc, incarnation, generation);
                        // Re-read the file's urgency under THIS hold. The
                        // value sampled before extraction can be stale: an
                        // interactive request joining the same generation
                        // while extraction ran would otherwise leave the
                        // newly admitted Analysis at the older, lower
                        // priority.
                        let effective_priority = dag
                            .highest_priority_for_file(&canonical_arc, generation)
                            .unwrap_or(inherited_priority);
                        if requires_analysis {
                            let status = self.ensure_analysis_for_demand(
                                &mut dag,
                                &canonical_arc,
                                incarnation,
                                generation,
                                std::cmp::min(effective_priority, inherited_priority),
                                AnalysisDemandKind::DirectRequest,
                            );
                            verter_debug_assert!(
                                matches!(status, BlockerStatus::Gating | BlockerStatus::Satisfied),
                                "direct Analysis demand must not consume blocker failure state",
                            );
                        }
                        // When a later stage is required, admit Analysis
                        // BEFORE completing Source under this one hold: a
                        // concurrent dead-producer classification must never
                        // observe an Analysis-bound generation with no live
                        // stage identity. A Source-only request deliberately
                        // has no later-stage producer and is complete here.
                        //
                        // Marking the Source identity complete drops its
                        // DAG bookkeeping and releases the capacity permit
                        // parked at dispatch. Any waiter gating on this
                        // Source identity (rare today, but supported by
                        // DepKey::FileStage{stage:Source}) is fanned out.
                        dag.complete(&source_id);
                        Vec::new()
                    }
                };
                // Requeue stranded waiters after the lock drops — the
                // requeue path sends on the inbox and must not run under
                // the DAG lock.
                for tok in stranded {
                    self.requeue_stranded_waiter(tok);
                }
            }
            TaskKind::Analysis => {
                // Mark the Analysis identity as complete in the DAG.
                // The DAG's `complete()` clears the dep from every
                // waiter's `deps_remaining`, so dependent artifacts can
                // proceed on the next dispatch pass.
                let analysis_id = WorkNodeIdentity::FileStage {
                    canonical: Arc::clone(&canonical_arc),
                    incarnation,
                    generation,
                    stage: FileStageKey::Analysis,
                };
                let analysis_was_gated = self.dag.lock().has_pending_deps(&analysis_id);
                if !analysis_was_gated {
                    // Only complete if the file-level gate was already
                    // clear; if it's still gated (this analysis was
                    // for a different reason), we keep it.
                    self.dag.lock().complete(&analysis_id);
                }

                // For each dependent file (via reverse-index), if its
                // file-level Analysis gate is now clear, admit any
                // pending artifacts. Snapshot the dep's generation
                // and drop the nodes-shard `Ref` BEFORE acquiring
                // `dag.lock()` — holding a Ref across the DAG mutex
                // would form a latent AB-BA ordering with any caller
                // that takes `dag.lock` first and then mutates the
                // dep file's nodes-shard entry.
                let dependents = self.edges.reverse_index.get(file_id);
                for dep_file in dependents {
                    let Some(dep_node) = self.nodes.get(&dep_file).map(|n| Arc::clone(n.value()))
                    else {
                        continue;
                    };
                    let dep_gen = dep_node.generation();
                    let dep_incarnation = dep_node.incarnation_id();
                    let dep_canonical: Arc<str> = Arc::from(dep_file.as_str());
                    let dep_analysis_id = WorkNodeIdentity::FileStage {
                        canonical: Arc::clone(&dep_canonical),
                        incarnation: dep_incarnation,
                        generation: dep_gen,
                        stage: FileStageKey::Analysis,
                    };
                    if self.dag.lock().has_pending_deps(&dep_analysis_id) {
                        continue;
                    }
                    let inherited = self
                        .dag
                        .lock()
                        .highest_priority_for_file(&dep_canonical, dep_gen)
                        .unwrap_or(Priority::Background);
                    self.admit_pending_artifacts(
                        &dep_canonical,
                        dep_incarnation,
                        dep_gen,
                        inherited,
                    );
                }

                // Admit this file's pending artifact waiters if its own
                // gate is clear.
                if !self.dag.lock().has_pending_deps(&analysis_id) {
                    self.admit_pending_artifacts(
                        &canonical_arc,
                        incarnation,
                        generation,
                        inherited_priority,
                    );
                }
            }
            TaskKind::Artifact { profile_hash } => {
                // Mark the artifact identity complete so the DAG
                // drops its bookkeeping, releases the capacity
                // permit parked at dispatch, and fans out any
                // dep-edge resolution (e.g. an artifact-on-artifact
                // dep edge that another file is waiting on). No
                // further stage admission is needed — artifacts are
                // terminal.
                //
                // Also clear the Artifact blocker registry entry for
                // this `(owner, generation)` IF no other profile is
                // still pending at this generation. Pending entries
                // ride on every Artifact admission at the
                // `(owner, generation)`; once every admission has
                // completed, the entry would otherwise persist and
                // grow the registry across long-lived sessions.
                let artifact_id = WorkNodeIdentity::Artifact {
                    canonical: Arc::clone(&canonical_arc),
                    incarnation,
                    generation,
                    profile_hash: profile_hash_to_bytes(*profile_hash),
                    content_hash: [0u8; 16],
                };
                let mut dag = self.dag.lock();
                dag.complete(&artifact_id);
                if dag
                    .pending_artifact_profiles(&canonical_arc, generation)
                    .is_empty()
                {
                    dag.clear_artifact_blockers(&canonical_arc, generation);
                }
            }
            // The pipeline advances on the request-target stage completions
            // (`Load` → Analysis, `Analysis` → Artifact, `Artifact` terminal).
            // `Parse` is intrinsic to the source stage and never completes as a
            // standalone stage, and `CacheNode` completion is owned by the
            // cache dispatch path, not the file-stage pipeline.
            TaskKind::Parse | TaskKind::CacheNode { .. } => unreachable!(
                "handle_stage_complete advances the file-stage pipeline on \
                 Load/Analysis/Artifact completions; Parse and CacheNode do not \
                 drive file-stage transitions"
            ),
        }
    }

    /// Single chokepoint for failure / panic terminal paths.
    ///
    /// Releases the parked capacity reservation on the matching DAG
    /// node and signals the appropriate `Failed(error)` to file waiter
    /// groups so callers do not hang.
    ///
    /// Sites that previously called `signal_file_failed*` directly are
    /// routed through this helper — without the node-cancel step the
    /// DAG would leak the parked admission permit and a {cpu:1, io:1}
    /// budget would stall the class on a single failure.
    ///
    /// `whole_file` semantics (Source / Analysis Err, FileNotFound,
    /// non-Artifact panic) signal `Failed` to every waiter group at
    /// `(canonical, generation)`. `per_stage` semantics (Artifact Err
    /// or Artifact panic) preserve other per-profile waiters at the
    /// same `(canonical, generation)`.
    pub(super) fn terminalize_failure(
        dag: &DagMutex,
        canonical: &Arc<str>,
        incarnation: u64,
        generation: u64,
        task_kind: &TaskKind,
        error: crate::job::SchedulerError,
    ) -> Vec<crate::dag::SubmissionToken> {
        let identity = Self::dag_identity_for_task(canonical, incarnation, generation, task_kind);
        let mut guard = dag.lock();
        if guard.token_for(&identity).is_none() {
            return Vec::new();
        }
        // 1. Cancel the DAG node — releases the parked capacity
        //    reservation through the by-value `release(self)` consume
        //    in `cancel`'s reservation drop path. Source / Analysis
        //    identities can be observed as `DepKey` prerequisites by
        //    downstream work (e.g., a same-file or dep-file Artifact
        //    gating on this Analysis), so a failure cancel may leave
        //    behind stranded waiter tokens whose only remaining gate
        //    was the now-cancelled identity. Return the stranded
        //    token list so the caller can re-enqueue through the
        //    same `requeue_stranded_waiter` path used by `remove()`
        //    — the downstream Artifact still dispatches (it will
        //    see the missing prerequisite via `current_*().is_none()`
        //    checks on the FileNode and surface its own failure
        //    rather than hang).
        // 1a. Analysis-failure fan-out to already-admitted waiters
        //     BEFORE cancel. `cancel(&analysis_identity)` would
        //     release each waiter's Analysis `DepKey` entry without
        //     recording a `FailedDepRecord` on the waiter — the
        //     downstream Artifact would then dispatch and resolve
        //     `Ready` over a snapshot built from a dead prerequisite.
        //     The fan-out helper records the failure marker on every
        //     waiter so the pre-dispatch chokepoint in
        //     `execute_stage_on_worker` surfaces a typed
        //     `DependencyFailed` instead. This is symmetric with the
        //     Source-side fan-out below (a Source failure also fans
        //     out via `fanout_source_failure_to_analysis_waiters`).
        //
        //     The fan-out runs BEFORE cancel so the cancel's
        //     `self.waiters.remove(&dep_key)` observes an empty
        //     reverse-index entry — the fan-out drained it. Without
        //     this ordering, cancel would strip the `DepKey` from
        //     each waiter's `deps_remaining` first, leaving no
        //     marker for the chokepoint to fire on.
        let mut stranded = Vec::new();
        if matches!(task_kind, TaskKind::Analysis) {
            let analysis_stranded = guard.fanout_analysis_failure_to_waiters(
                canonical,
                incarnation,
                generation,
                &error,
            );
            stranded.extend(analysis_stranded);
        }
        // 1. Cancel the DAG node — releases the parked capacity
        //    reservation through the by-value `release(self)` consume
        //    in `cancel`'s reservation drop path. For Analysis-stage
        //    failures the cancel's waiter sweep observes the empty
        //    reverse-index entry left by the fan-out above; for
        //    Source-stage failures the Source-keyed waiters are
        //    handled here.
        stranded.extend(guard.cancel(&identity));
        // 1b. Source-failure fan-out to Analysis-keyed waiters at the
        //     same `(canonical, generation)`. The Source cancel above
        //     only fans out to `DepKey::FileStage { stage: Source }`
        //     waiters, but downstream blockers gate on the Analysis
        //     DepKey (Artifact admissions inherit `DepKey::FileStage
        //     { stage: Analysis }` via the typed blocker registry).
        //     Without this propagation the Analysis identity is
        //     never admitted (Analysis admission is gated on Source
        //     success), so the Analysis-keyed waiters stay pinned
        //     forever on a dep that cannot make progress. The
        //     `fanout_source_failure_to_analysis_waiters` helper
        //     drops the Analysis DepKey from each waiter's
        //     `deps_remaining` and returns any newly-stranded
        //     waiters so the caller re-enqueues them through the
        //     same path as the Source-key strand list. There is no
        //     Analysis DAG identity at this `(canonical, generation)`
        //     to double-cancel: admission requires `current_source().
        //     is_some()`, which is false on the Source-failure path.
        //
        //     The producer's terminal `error` is forwarded so the
        //     `FailedDepRecord` on each waiter carries the cause
        //     verbatim — the downstream short-circuit then surfaces
        //     a typed `DependencyFailed` instead of synthesising a
        //     stage-only envelope.
        if matches!(task_kind, TaskKind::Load | TaskKind::Parse) {
            let analysis_stranded = guard.fanout_source_failure_to_analysis_waiters(
                canonical,
                incarnation,
                generation,
                &error,
            );
            stranded.extend(analysis_stranded);
        }
        // 1c. Persistent terminal-dep-failure record. The fan-out
        //     above marks every already-admitted Analysis-keyed
        //     waiter at this `(canonical, generation)`. The
        //     persistent map closes the pre-admission race: a
        //     waiter that admits AFTER the producer terminalized
        //     consults this store (via the matrix's
        //     [`Scheduler::file_stage_analysis_blocker_status`]) and
        //     attaches the same `FailedDepRecord` to the freshly-
        //     submitted node so the pre-dispatch short-circuit fires
        //     uniformly. Recorded under the Analysis `DepKey` even
        //     for Source-stage failures because cross-file Artifact
        //     blockers always key on the producer's Analysis stage
        //     (`register_resolved_deps` records `DepKey::FileStage
        //     { stage: Analysis }`).
        if matches!(
            task_kind,
            TaskKind::Load | TaskKind::Parse | TaskKind::Analysis
        ) {
            let analysis_dep_key = crate::dag::DepKey::FileStage {
                canonical: Arc::clone(canonical),
                incarnation,
                generation,
                stage: crate::dag::FileStageKey::Analysis,
            };
            guard.insert_terminal_dep_failure(crate::dag::FailedDepRecord {
                dep_key: analysis_dep_key,
                cause: error.clone(),
            });
        }
        // 2. Signal Failed to file waiter groups. Artifact failures
        //    must NOT terminate other-profile waiters at the same
        //    (canonical, generation) — use the per-stage variant.
        match task_kind {
            TaskKind::Load | TaskKind::Parse | TaskKind::Analysis => {
                guard.signal_file_failed(canonical, generation, error);
            }
            TaskKind::Artifact { .. } => {
                guard.signal_file_failed_for_stage(canonical, generation, task_kind, error);
            }
            // CacheNode completion/release is owned by the cache dispatch path;
            // it never terminalizes through the file-stage failure chokepoint.
            TaskKind::CacheNode { .. } => unreachable!(
                "terminalize_failure addresses file-stage tasks (Load/Parse/Analysis/Artifact); \
                 CacheNode release is owned by the cache dispatch path"
            ),
        }
        stranded
    }

    /// Static-context analogue of [`Self::requeue_stranded_waiter`]:
    /// when `stranded` is non-empty, send a single `Wake` into the
    /// inbox so the driver re-runs the cooperative pump and picks
    /// up any DAG node whose `deps_remaining` cleared as a side
    /// effect of the cancel. Used by [`Self::terminalize_failure`]'s
    /// callers in static (worker) contexts where `&self` is not in
    /// scope.
    pub(super) fn requeue_terminalize_stranded(
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        stranded: &[crate::dag::SubmissionToken],
    ) {
        if !stranded.is_empty() {
            let _ = inbox_sender.try_send(Submission::Wake);
        }
    }

    /// Surface a worker-stage panic as `Failed` on all pending groups
    /// at this `(generation, task_kind)` so callers never hang on a
    /// crashed stage. The panic has been swallowed by the worker's
    /// `catch_unwind` — this helper completes the signalling that the
    /// executor's normal error path would have done AND releases the
    /// parked capacity reservation so the resource class does not
    /// stall.
    ///
    /// `terminalize_failure` runs unconditionally — BEFORE the
    /// generation guard — so a panic on a now-superseded generation
    /// still releases the parked admission permit and cancels the
    /// stale DAG identity. Both inner steps are idempotent: a
    /// `cancel` of an identity not in `by_identity` returns an empty
    /// stranded list with no side effect, and `signal_file_failed*`
    /// on a `(canonical, generation)` whose waiter groups were
    /// already drained by `supersede_old_file_generations` is a
    /// no-op. The generation guard only skips the inbox notify path
    /// (already a no-op here).
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn surface_stage_panic_as_failed(
        node: &FileNode,
        generation: u64,
        task_kind: &TaskKind,
        inbox_sender: &crossbeam_channel::Sender<Submission>,
        dag: Arc<DagMutex>,
    ) {
        // `terminalize_failure` runs UNCONDITIONALLY (no generation
        // guard) so a panic on a now-superseded generation still
        // releases the parked admission permit and cancels the
        // stale DAG identity. An early-return on generation-mismatch
        // here would let the parked permit linger between
        // `bump_generation` and the supersede sweep, stalling the
        // resource class on a panic that raced an invalidation.
        let error = crate::job::SchedulerError::StageFailed {
            file_id: node.canonical_id.clone(),
            stage: format!("{task_kind:?}"),
            message: "stage executor panicked".to_string(),
        };
        let incarnation = node.incarnation_id();
        let canonical: Arc<str> = Arc::from(node.canonical_id.as_str());
        let stranded =
            Self::terminalize_failure(&dag, &canonical, incarnation, generation, task_kind, error);
        Self::requeue_terminalize_stranded(inbox_sender, &stranded);
    }

    /// Return `true` when the per-profile artifact slot already holds
    /// a snapshot at `generation`. The DashMap `Ref` is taken, read,
    /// and dropped within this helper's body so no shard-read lock
    /// escapes to the caller. Callers MUST use this helper rather
    /// than holding a `node.artifacts.get(...)` `Ref` across a
    /// subsequent `dag.lock()` acquisition: the external
    /// `commit_artifact` path holds the DAG lock and then writes
    /// into the same DashMap shard, so a shard-read held across
    /// `dag.lock()` would invert that ordering and deadlock.
    pub(crate) fn artifact_already_committed_at(
        node: &FileNode,
        profile_hash: u64,
        generation: u64,
    ) -> bool {
        node.artifacts
            .get(&profile_hash)
            .map(|existing| existing.generation == generation)
            .unwrap_or(false)
    }
}
