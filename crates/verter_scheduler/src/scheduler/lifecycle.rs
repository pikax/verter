//! Lifecycle — construction, invalidation, removal and node incarnation bookkeeping.
//!
//! Part of the `scheduler` module. The root re-exports this module, so
//! every name used here is reached through the root rather than through a
//! sibling module.

use super::*;

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::Barrier;

    struct PausedSource {
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
    }

    impl StageExecutor for PausedSource {
        fn execute_source(
            &self,
            _: &str,
            _: FileLanguage,
            content: Arc<str>,
            generation: u64,
        ) -> Result<SourceSnapshot, crate::execution::executor::StageError> {
            self.entered.wait();
            self.release.wait();
            Ok(SourceSnapshot::new_empty(content, generation))
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn delayed_source_worker_cannot_republish_a_retired_incarnation() {
        for reset in [false, true] {
            let loader = Arc::new(crate::source_loader::MemorySourceLoader::new());
            loader.insert("/delayed.ts".into(), Arc::from("source"));
            let scheduler = Scheduler::test_new_sync(SchedulerConfig::default(), loader.clone());
            let old = scheduler.submit_request(Request {
                file_id: "/delayed.ts".into(),
                source: None,
                target: TargetStage::Source,
                priority: Priority::Interactive,
                file_language: None,
                request_context: None,
            });
            scheduler.drain_inbox();
            let node = scheduler.nodes.get("/delayed.ts").unwrap().clone();
            let generation = node.generation();
            let job = scheduler
                .dag
                .lock()
                .next_ready()
                .expect("source work is ready");
            let entered = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            let worker = {
                let scheduler = scheduler.clone();
                let entered = entered.clone();
                let release = release.clone();
                std::thread::spawn(move || {
                    Scheduler::execute_source_stage(
                        &node,
                        generation,
                        &PausedSource { entered, release },
                        loader.as_ref(),
                        &scheduler.inbox.sender,
                        scheduler.dag.clone(),
                        scheduler.source_root.clone(),
                    )
                })
            };
            entered.wait();
            if reset {
                scheduler.reset();
            } else {
                scheduler.remove("/delayed.ts");
            }
            // Exercise the protocol without the external-publisher restart fence.
            let fresh = Arc::new(FileNode::new(
                "/delayed.ts".into(),
                scheduler.source_loader.classify("/delayed.ts"),
            ));
            let fresh_generation = fresh.generation();
            let incarnation = fresh.incarnation_id();
            scheduler.nodes.insert("/delayed.ts".into(), fresh);
            let current = scheduler.submit_request(Request {
                file_id: "/delayed.ts".into(),
                source: None,
                target: TargetStage::Source,
                priority: Priority::Interactive,
                file_language: None,
                request_context: None,
            });
            scheduler.drive_all();
            let before = scheduler.capture_source_root();
            release.wait();
            let late = worker.join().expect("source worker panicked");
            assert_eq!(fresh_generation, 0);
            assert!(late.is_none(), "retired source must refuse publication");
            assert!(matches!(old.try_get(), Some(CompletionState::Shutdown)));
            assert!(matches!(current.try_get(), Some(CompletionState::Ready(_))));
            assert_eq!(
                scheduler.nodes.get("/delayed.ts").unwrap().incarnation_id(),
                incarnation
            );
            assert_eq!(
                before.lookup("/delayed.ts"),
                scheduler.capture_source_root().lookup("/delayed.ts")
            );
            assert!(scheduler.dag.lock().token_for(&job.identity).is_none());
            assert_eq!(scheduler.dag.lock().total_active(), 0);
        }
    }

    #[derive(Debug)]
    struct Tagged(&'static str);

    impl crate::node::SnapshotData for Tagged {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn analyze(scheduler: &Scheduler, id: &str) -> crate::node::WitnessedSource {
        scheduler.submit_request(Request {
            file_id: id.into(),
            source: None,
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            file_language: None,
            request_context: None,
        });
        scheduler.drive_all();
        scheduler
            .try_get_witnessed_source(id)
            .expect("analysis commits a current source")
    }

    fn artifact_tag(scheduler: &Scheduler, id: &str, profile_hash: u64) -> Option<&'static str> {
        scheduler
            .try_get_last_known_good(id, profile_hash)
            .and_then(|artifact| artifact.downcast_data::<Tagged>().map(|tagged| tagged.0))
    }

    #[test]
    fn delayed_external_publication_cannot_cross_a_retired_incarnation() {
        const PUBLISHED: u64 = 7;
        const CLEANED: u64 = 9;
        for reset in [false, true] {
            let loader = Arc::new(crate::source_loader::MemorySourceLoader::new());
            loader.insert("/external.vue".into(), Arc::from("same"));
            let scheduler = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
            let stale = analyze(&scheduler, "/external.vue").witness;

            // The delayed publisher captured its witness above and parks
            // between compute and publication.
            let entered = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            let publisher = {
                let scheduler = scheduler.clone();
                let stale = stale.clone();
                let entered = entered.clone();
                let release = release.clone();
                std::thread::spawn(move || {
                    entered.wait();
                    release.wait();
                    let committed =
                        scheduler.commit_artifact(&stale, PUBLISHED, Arc::new(Tagged("stale")));
                    let evicted = scheduler.remove_artifact_not_newer_than(&stale, CLEANED);
                    (committed, evicted)
                })
            };
            entered.wait();
            if reset {
                scheduler.reset();
            } else {
                scheduler.remove("/external.vue");
            }
            // Re-add the same content at the same generation: without the
            // generation floor the successor reuses the retired generation.
            scheduler.generation_floors.clear();
            let current = analyze(&scheduler, "/external.vue");
            assert_eq!(current.snapshot.source, Arc::from("same"));
            assert_eq!(current.witness.generation(), stale.generation());
            assert_ne!(current.witness.incarnation(), stale.incarnation());
            assert!(scheduler.try_get_source_for_witness(&stale).is_none());
            assert!(scheduler
                .try_get_source_for_witness(&current.witness)
                .is_some());
            assert!(scheduler.commit_artifact(
                &current.witness,
                CLEANED,
                Arc::new(Tagged("current"))
            ));

            release.wait();
            let (committed, evicted) = publisher.join().expect("publisher panicked");
            assert!(!committed, "a retired incarnation must not publish");
            assert!(!evicted, "a retired incarnation must not clean up");
            assert_eq!(artifact_tag(&scheduler, "/external.vue", PUBLISHED), None);
            assert_eq!(
                artifact_tag(&scheduler, "/external.vue", CLEANED),
                Some("current")
            );

            // The current witness still publishes and cleans up.
            assert!(scheduler.commit_artifact(
                &current.witness,
                PUBLISHED,
                Arc::new(Tagged("current"))
            ));
            assert_eq!(
                artifact_tag(&scheduler, "/external.vue", PUBLISHED),
                Some("current")
            );
            assert!(scheduler.remove_artifact_not_newer_than(&current.witness, CLEANED));
            assert_eq!(artifact_tag(&scheduler, "/external.vue", CLEANED), None);
        }
    }

    #[test]
    fn unknown_removals_do_not_allocate_restart_history() {
        let scheduler = Scheduler::test_new_sync(
            SchedulerConfig::default(),
            Arc::new(crate::source_loader::MemorySourceLoader::new()),
        );
        let capacity = scheduler.generation_floors.capacity();
        for index in 0..256 {
            scheduler.remove(&format!("/unknown-{index}.ts"));
        }
        assert!(scheduler.generation_floors.is_empty());
        assert_eq!(scheduler.generation_floors.capacity(), capacity);
        assert!(scheduler.nodes.is_empty());
        assert_eq!(scheduler.dag.lock().total_active(), 0);
    }

    #[test]
    fn delayed_failure_cannot_cancel_same_generation_successor_after_remove_or_reset() {
        for reset in [false, true] {
            let loader = Arc::new(crate::source_loader::MemorySourceLoader::new());
            loader.insert("/reused.ts".into(), Arc::from("source"));
            let scheduler = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
            let request = || Request {
                file_id: "/reused.ts".into(),
                source: None,
                target: TargetStage::Source,
                priority: Priority::Interactive,
                file_language: None,
                request_context: None,
            };
            let old = scheduler.submit_request(request());
            scheduler.drive_all();
            assert!(matches!(old.try_get(), Some(CompletionState::Ready(_))));
            let generation = scheduler.nodes.get("/reused.ts").unwrap().generation();
            let incarnation = scheduler.nodes.get("/reused.ts").unwrap().incarnation_id();
            let enter = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            let late = {
                let scheduler = Arc::clone(&scheduler);
                let enter = Arc::clone(&enter);
                let release = Arc::clone(&release);
                std::thread::spawn(move || {
                    enter.wait();
                    release.wait();
                    Scheduler::terminalize_failure(
                        &scheduler.dag,
                        &Arc::from("/reused.ts"),
                        incarnation,
                        generation,
                        &TaskKind::Load,
                        crate::job::SchedulerError::FileNotFound {
                            file_id: "/reused.ts".into(),
                        },
                    )
                })
            };
            enter.wait();
            if reset {
                scheduler.reset();
            } else {
                scheduler.remove("/reused.ts");
            }
            scheduler.nodes.insert(
                "/reused.ts".into(),
                Arc::new(FileNode::new(
                    "/reused.ts".into(),
                    scheduler.source_loader.classify("/reused.ts"),
                )),
            );
            let current = scheduler.submit_request(request());
            scheduler.drain_inbox();
            let current_generation = scheduler
                .nodes
                .get("/reused.ts")
                .map(|node| node.generation());
            release.wait();
            let stranded = late.join().expect("delayed failure panicked");
            assert!(stranded.is_empty());
            assert_eq!(
                current_generation,
                Some(generation),
                "fixture must exercise colliding work generations"
            );
            assert!(
                current.try_get().is_none(),
                "retired failure must not signal successor"
            );
            scheduler.drive_all();
            assert!(matches!(current.try_get(), Some(CompletionState::Ready(_))));
        }
    }

    #[test]
    fn removal_cannot_admit_prepared_work_between_sweep_and_unpublication() {
        let loader = Arc::new(crate::source_loader::MemorySourceLoader::new());
        loader.insert("/removed.vue".into(), Arc::from("old"));
        let scheduler = Scheduler::test_new_sync(SchedulerConfig::default(), loader);
        let request = || Request {
            file_id: "/removed.vue".into(),
            source: Some(Arc::from("new")),
            target: TargetStage::Analysis,
            priority: Priority::Interactive,
            file_language: None,
            request_context: None,
        };
        let first = scheduler.submit_request(request());
        scheduler.drive_all();
        assert!(matches!(first.try_get(), Some(CompletionState::Ready(_))));

        let (handle, sender) = completion_pair();
        let prepared = scheduler
            .prepare_request(QueuedRequest {
                file_id: "/removed.vue".into(),
                source: Some(Arc::from("crossing")),
                target: TargetStage::Analysis,
                priority: Priority::Interactive,
                file_language: None,
                request_context: None,
                submitted_lifetime: scheduler
                    .nodes
                    .get("/removed.vue")
                    .unwrap()
                    .incarnation_id(),
                sender,
            })
            .expect("live request must prepare");

        let enter = Arc::new(Barrier::new(2));
        let leave = Arc::new(Barrier::new(2));
        let competitor = {
            let scheduler = Arc::clone(&scheduler);
            let enter = Arc::clone(&enter);
            let leave = Arc::clone(&leave);
            std::thread::spawn(move || {
                enter.wait();
                let mut prepared = Some(prepared);
                let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Some(mut dag) = scheduler.dag.try_lock() {
                        let mut post = AdmissionPostWork::default();
                        scheduler.admit_prepared_under_lock(
                            &mut dag,
                            prepared.take().unwrap(),
                            &mut post,
                        );
                        let admitted = dag.total_active() > 0;
                        drop(dag);
                        post.run(&scheduler);
                        admitted
                    } else {
                        false
                    }
                }));
                // Release the remover even if the attempted admission panics.
                leave.wait();
                let admitted = attempt.expect("crossing admission panicked");
                if let Some(prepared) = prepared {
                    let mut dag = scheduler.dag.lock();
                    let mut post = AdmissionPostWork::default();
                    scheduler.admit_prepared_under_lock(&mut dag, prepared, &mut post);
                    drop(dag);
                    post.run(&scheduler);
                }
                admitted
            })
        };
        scheduler.remove_with_after_sweep("/removed.vue", || {
            enter.wait();
            leave.wait();
        });
        let admitted_during_removal = competitor.join().expect("admission observer panicked");
        assert!(
            !admitted_during_removal,
            "crossing admission escaped the removal sweep"
        );
        assert!(!scheduler.has_node("/removed.vue"));
        assert_eq!(scheduler.dag.lock().total_active(), 0);
        assert!(matches!(handle.try_get(), Some(CompletionState::Shutdown)));

        // A fresh request after the completed removal must still succeed.
        let replacement = scheduler.submit_request(request());
        scheduler.drive_all();
        assert!(matches!(
            replacement.try_get(),
            Some(CompletionState::Ready(_))
        ));
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        // Set shutdown flag
        self.shutdown.store(true, Ordering::Release);
        // Dedicated teardown signal: the inbox wake below can be consumed
        // by a cooperative pump before the driver parks on it.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.driver_teardown.0.try_send(());
        let _ = self.inbox.sender.try_send(Submission::Wake);
        self.shutdown_all_scoped_cache_flights();

        // Close inbox (causes driver recv to return Disconnected)
        // Drop the sender to close the channel
        // Note: The inbox sender is shared, but dropping the Scheduler
        // signals shutdown via the flag

        // Join driver thread
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(handle) = self.driver_handle.lock().take() {
                if should_join_driver_thread(handle.thread().id(), std::thread::current().id()) {
                    let _ = handle.join();
                }
            }
        }

        // Signal shutdown to all pending waiter groups.
        self.dag.lock().signal_all_shutdown();
    }
}

impl Scheduler {
    /// Build default scheduler pools sized from `config`, matching the
    /// host's construction (CPU/IO worker counts from `cpu_threads` /
    /// `io_threads`; each transport capacity dominates the matching
    /// `resolved_dag_budget()` class).
    #[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
    pub(super) fn default_test_pools(
        config: &SchedulerConfig,
    ) -> (
        Arc<crate::execution::pool::SchedulerCpuPool>,
        Arc<crate::execution::pool::SchedulerIoPool>,
    ) {
        let budget = config.resolved_dag_budget();
        (
            crate::execution::pool::SchedulerCpuPool::new(config.cpu_threads, budget.cpu as usize),
            crate::execution::pool::SchedulerIoPool::new(config.io_threads, budget.io as usize),
        )
    }

    /// Create a FileNode for a file, respecting the generation floor
    /// for external publishers that do not yet carry an incarnation witness.
    pub(super) fn create_node(
        &self,
        file_id: &str,
        file_language: Option<FileLanguage>,
    ) -> Arc<FileNode> {
        self.create_node_at_least(file_id, file_language, 0)
    }

    pub(super) fn create_node_at_least(
        &self,
        file_id: &str,
        file_language: Option<FileLanguage>,
        min_generation: u64,
    ) -> Arc<FileNode> {
        let language = file_language.unwrap_or_else(|| self.source_loader.classify(file_id));
        Arc::new(FileNode::new_at(
            file_id.to_string(),
            language,
            self.generation_floors
                .get(file_id)
                .map_or(min_generation, |floor| {
                    min_generation.max(
                        floor
                            .checked_add(1)
                            .expect("file generation identity space exhausted"),
                    )
                }),
        ))
    }

    /// Bind queued work to the current submission lifetime under the lifecycle hold.
    /// Zero is a refused stamp; allocated incarnation ids start at one.
    pub(super) fn stamp_request(&self, id: &str, language: Option<FileLanguage>) -> u64 {
        if language.is_none() {
            let _dag = self.dag.lock();
            if self.shutdown.load(Ordering::Acquire) {
                return 0;
            }
            if let Some(node) = self.nodes.get(id) {
                return node.submission_lifetime();
            }
        }
        let language = language.unwrap_or_else(|| self.source_loader.classify(id));
        let _dag = self.dag.lock();
        if self.shutdown.load(Ordering::Acquire) {
            return 0;
        }
        self.nodes
            .entry(id.to_owned())
            .or_insert_with(|| self.create_node(id, Some(language)))
            .submission_lifetime()
    }

    /// Remove a file from the scheduler.
    ///
    /// Signals shutdown to pending request handles, removes the node,
    /// cleans up forward/reverse edges, and unblocks any dependents that
    /// were waiting on this file (since the blocker can never resolve).
    pub fn remove(&self, id: &str) {
        self.remove_with_after_sweep(
            id,
            #[cfg(test)]
            || {},
        );
    }

    fn remove_with_after_sweep(&self, id: &str, #[cfg(test)] after_sweep: impl FnOnce()) {
        let canonical: Arc<str> = Arc::from(id);
        let stranded = {
            // Admission, retirement and node unpublication share one
            // linearization point. A prepared request cannot bump the live
            // node past the sweep before that node is removed.
            let mut dag = self.dag.lock();
            let last_gen = self
                .nodes
                .get(id)
                .map(|node| {
                    node.retire(&mut dag);
                    node.generation()
                })
                .unwrap_or(0);

            // Preserve removal's Shutdown cause before the supersede sweep.
            dag.signal_file_shutdown(&canonical);
            let mut stranded = dag.retire_generations_below(&canonical, last_gen.saturating_add(1));
            let (_, also_stranded) = dag.cancel_matching(|identity| match identity {
                WorkNodeIdentity::FileStage { canonical, .. }
                | WorkNodeIdentity::Artifact { canonical, .. } => canonical.as_ref() == id,
                WorkNodeIdentity::CacheNode { .. } => false,
            });
            stranded.extend(also_stranded);
            dag.artifact_blocker_deps_remove_owner(id);
            dag.scrub_artifact_blockers_referencing(id);
            dag.scrub_terminal_dep_failures_referencing(id);

            #[cfg(test)]
            after_sweep();
            self.deferred_blocker_ids.remove(id);
            self.auto_ingested_recent.remove(&canonical);

            if self.nodes.contains_key(id) {
                self.generation_floors.insert(id.to_owned(), last_gen);
            }
            let removed = self.source_root.publish_transition(|publication| {
                let removed = self.nodes.remove(id);
                if let Some((_, node)) = removed.as_ref() {
                    publication.absent(&canonical, node.incarnation_id(), node.generation());
                }
                removed
            });
            if removed.is_some() {
                self.edges.remove_file(id);
            }
            stranded
        };

        // The wake can re-enter the pump; it belongs outside the DAG hold.
        for token in stranded {
            self.requeue_stranded_waiter(token);
        }
    }

    /// TEST-ONLY: fire `attempt` after the node snapshot and BEFORE
    /// `dag.lock()`, so a test holding the DAG lock can sample
    /// generation once the invalidator has reached the lock rather
    /// than spinning on `yield_now`.
    #[cfg(test)]
    pub(crate) fn invalidate_signaling_before_dag_lock(
        &self,
        id: &str,
        attempt: std::sync::mpsc::SyncSender<()>,
    ) {
        self.invalidate_with_lock_hooks(
            id,
            &mut || {
                let _ = attempt.send(());
            },
            &mut || {},
        );
    }

    /// TEST-ONLY: fire `attempt` after `dag.lock()` and BEFORE the
    /// publication hold, so a test holding the publication lock can
    /// sample generation without a yield loop.
    #[cfg(test)]
    pub(crate) fn invalidate_signaling_before_publication(
        &self,
        id: &str,
        attempt: std::sync::mpsc::SyncSender<()>,
    ) {
        self.invalidate_with_lock_hooks(id, &mut || {}, &mut || {
            let _ = attempt.send(());
        });
    }

    pub(super) fn invalidate_with_lock_hooks(
        &self,
        id: &str,
        before_dag_lock: &mut dyn FnMut(),
        before_publication: &mut dyn FnMut(),
    ) {
        // Snapshot the FileNode `Arc` and drop the nodes-shard `Ref`
        // BEFORE acquiring `dag.lock()`. Holding a DashMap Ref
        // across a parking_lot Mutex acquisition forms a latent
        // AB-BA ordering with any caller that takes `dag.lock`
        // first and then mutates the same nodes shard. The
        // DAG-first ordering is the canonical one for the lifecycle
        // sweeps, so the nodes-shard reader must release the Ref
        // before locking. The cloned `Arc<FileNode>` preserves
        // every field access the original Ref enabled.
        let node = match self.nodes.get(id) {
            Some(r) => Arc::clone(&r),
            None => return,
        };
        let canonical: Arc<str> = Arc::from(id);
        before_dag_lock();
        let mut dag = self.dag.lock();
        if !self
            .nodes
            .get(id)
            .is_some_and(|live| live.incarnation_id() == node.incarnation_id())
        {
            return;
        }
        before_publication();
        // The bump and the source-root publication run under ONE
        // publication hold, so a concurrent `capture_source_root` sees
        // the pre-bump node with the pre-bump root or the post-bump node
        // with the post-bump root — never a torn pair. The publication
        // lock is INNER to the DAG lock already held here.
        let new_gen = self.source_root.publish_transition(|publication| {
            let new_gen = publication.bump_node_generation(&node);
            publication.absent(&canonical, node.incarnation_id(), new_gen);
            new_gen
        });
        // Stale per-(owner, generation) Artifact blocker entries
        // for superseded generations are dropped inside
        // `supersede_old_file_generations` (it now also scrubs
        // the DAG's artifact_blocker_deps registry for stale
        // owner-canonical entries). The new generation records
        // its own blockers via `register_resolved_deps`.
        dag.supersede_old_file_generations(&canonical, new_gen);
    }
}
