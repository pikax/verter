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
                submitted_epoch: scheduler.removal_epoch.load(Ordering::Acquire),
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
    /// from prior incarnations so stale completions never match.
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
        let floor_gen = self
            .generation_floors
            .get(file_id)
            .map(|floor| *floor + 1)
            .unwrap_or(0);
        Arc::new(FileNode::new_at(
            file_id.to_string(),
            language,
            floor_gen.max(min_generation),
        ))
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
            let epoch = self.removal_epoch.fetch_add(1, Ordering::AcqRel) + 1;
            self.tombstones.insert(id.to_string(), epoch);
            let last_gen = self.nodes.get(id).map(|n| n.generation()).unwrap_or(0);

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

            // Publish the floor before removing the node: preparation may
            // ensure a replacement outside the DAG lock as soon as the shard
            // entry disappears, and must start that replacement above it.
            if self.nodes.contains_key(id) {
                self.generation_floors.insert(id.to_string(), last_gen);
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
