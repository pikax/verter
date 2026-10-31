//! Owned scheduler-backed loading. Polling transfers host integration to the
//! external caller; it never resolves workspace routes or drives a stage.

use std::sync::Arc;
use std::task::{Poll, Waker};
use verter_execution::cancellation::CancellationToken;
use verter_execution::dependency::DependencyStop;
use verter_execution::tasks::ExecutionTask;
use verter_scheduler::job::{CompletionState, SchedulerError};
use verter_scheduler::scheduler::{OwnedSchedulerRequest, Request};
use verter_scheduler::stage::{Priority, TargetStage};

use crate::VerterHost;

/// A stopped load carries no partially integrated host state.
#[derive(Clone, Debug)]
pub enum HostLoadError {
    Cancelled,
    Cycle,
    Superseded,
    Shutdown,
    Failed(SchedulerError),
    WrongHost,
    NoProgress,
}

/// An exclusive, movable dependency subscription. Its only operation polls
/// readiness; the scheduler and host integration run outside that poll.
pub struct HostLoadHandoff {
    request: Option<OwnedSchedulerRequest>,
    commit: Option<HostLoadCommit>,
}

impl HostLoadHandoff {
    pub(crate) fn awaiting_scheduler(&self) -> bool {
        self.request.is_some()
    }
    /// Transfer integration once Analysis has committed. Cancellation and
    /// scheduler failure drop all retained request state without integration.
    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<HostLoadCommit, HostLoadError>> {
        let commit = self.commit.as_ref().expect("load already completed");
        if commit.fence.stop.is_cancelled() {
            self.request = None;
            self.commit = None;
            return Poll::Ready(Err(HostLoadError::Cancelled));
        }
        if let Some(request) = &mut self.request {
            let result = match request.poll(waker) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result,
            };
            self.request = None;
            let error = match result {
                Ok(CompletionState::Ready(verter_scheduler::job::RequestResult::Analysis(
                    snapshot,
                ))) => {
                    self.commit
                        .as_mut()
                        .expect("live integration")
                        .fence
                        .version = Some(snapshot.version());
                    None
                }
                Ok(CompletionState::Ready(_)) => unreachable!("host load always requests Analysis"),
                Ok(CompletionState::Failed(error)) => Some(HostLoadError::Failed(error)),
                Ok(CompletionState::Superseded) => Some(HostLoadError::Superseded),
                Ok(CompletionState::Shutdown) => Some(HostLoadError::Shutdown),
                Err(DependencyStop::Cancelled) => Some(HostLoadError::Cancelled),
                Err(DependencyStop::Cycle) => Some(HostLoadError::Cycle),
            };
            if let Some(error) = error {
                self.commit = None;
                return Poll::Ready(Err(error));
            }
        }
        Poll::Ready(Ok(self.commit.take().expect("integration transfers once")))
    }
}

/// Owned integration inputs, transferred after the scheduler dependency is
/// complete. Applying them is external work and can resolve workspace routes.
/// The host identity prevents applying a demand to another host's state.
pub struct HostLoadCommit {
    host: u64,
    canonical: String,
    reload: bool,
    pre_evict_hash: Option<[u8; 16]>,
    integrated: bool,
    fence: HostLoadFence,
}

/// Captured lifecycle facts, validated again at the existing visibility write.
pub(crate) struct HostLoadFence {
    version: Option<verter_scheduler::node::SourceVersion>,
    eviction_revision: u64,
    project_generation: u64,
    stop: CancellationToken,
}

impl HostLoadFence {
    pub(crate) fn check(
        &self,
        host: &VerterHost,
        canonical: &str,
        eviction_revision: u64,
    ) -> Result<(), HostLoadError> {
        if self.stop.is_cancelled() {
            return Err(HostLoadError::Cancelled);
        }
        if eviction_revision != self.eviction_revision
            || host.project_type_store.project_generation() != self.project_generation
            || self.version.is_none_or(|version| {
                host.scheduler
                    .try_get_source(canonical)
                    .is_none_or(|source| source.version() != version)
                    || host
                        .scheduler
                        .try_get_analysis(canonical)
                        .is_none_or(|analysis| analysis.version() != version)
            })
        {
            return Err(HostLoadError::Superseded);
        }
        Ok(())
    }
}

impl HostLoadCommit {
    pub fn apply(self, host: &VerterHost) -> Result<bool, HostLoadError> {
        if self.host != host.instance_id {
            return Err(HostLoadError::WrongHost);
        }
        if self.fence.stop.is_cancelled() {
            return Err(HostLoadError::Cancelled);
        }
        let eviction_revision = host
            .derived_raw_cache()
            .get(&self.canonical)
            .map_or(0, |state| state.eviction_revision());
        self.fence.check(host, &self.canonical, eviction_revision)?;
        if self.integrated {
            return Ok(true);
        }
        let work_start = verter_type_engine::instant::Instant::now();
        let loaded = host.integrate_scheduler_snapshot(&self.canonical, &self.fence);
        host.provenance.ensure_loaded_work_ns.fetch_add(
            work_start.elapsed().as_nanos() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        let loaded = loaded?;
        if loaded {
            let content_changed = self.reload
                && match (
                    self.pre_evict_hash,
                    host.scheduler
                        .try_get_source(&self.canonical)
                        .map(|s| s.whole_hash),
                ) {
                    (Some(before), Some(after)) => before != after,
                    _ => true,
                };
            if content_changed {
                host.bump_store_view_epoch();
            } else {
                host.bump_load_generation();
            }
        }
        Ok(loaded)
    }
}

impl VerterHost {
    /// Begin host loading through the scheduler's owned admission/completion
    /// protocol. The root supplies its task from this host's shared registry.
    /// An immediately ready integrated file requires no producer edge.
    pub fn begin_load(
        &self,
        canonical: &str,
        waiter: ExecutionTask,
        stop: CancellationToken,
    ) -> HostLoadHandoff {
        use std::sync::atomic::Ordering::Relaxed;
        self.provenance.ensure_loaded_calls.fetch_add(1, Relaxed);
        #[cfg(test)]
        self.test_force
            .loads_begun
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let project_generation = self.project_type_store.project_generation();
        let canonical = self.normalized_analysis_canonical(canonical).into_owned();
        let (reload, pre_evict_hash, eviction_revision) = loop {
            let reset_lock = self
                .derived_raw_cache()
                .get(&canonical)
                .map(|derived| Arc::clone(&derived.eviction_reset));
            let _reset = reset_lock.as_ref().map(|lock| lock.lock());
            // The state is read under the lock its row owns: a row an
            // eviction created, or replaced, after the lock lookup is read
            // again under its own lock.
            let observed = self.derived_raw_cache().get(&canonical).map(|d| {
                (
                    reset_lock
                        .as_ref()
                        .is_some_and(|lock| Arc::ptr_eq(lock, &d.eviction_reset)),
                    d.evicted,
                    d.evicted_whole_hash,
                    d.eviction_revision(),
                    d.eviction_reset_epoch,
                )
            });
            if observed.is_some_and(|(owns_lock, ..)| !owns_lock) {
                continue;
            }
            break match observed
                .map(|(_, evicted, hash, epoch, reset)| (evicted, hash, epoch, reset))
            {
                Some((true, pre_evict_hash, epoch, reset_epoch)) => {
                    if reset_epoch < epoch && !stop.is_cancelled() {
                        self.scheduler.close_file(&canonical);
                        if let Some(mut derived) = self.derived_raw_cache().get_mut(&canonical) {
                            derived.eviction_reset_epoch = derived.eviction_reset_epoch.max(epoch);
                        }
                    }
                    (true, pre_evict_hash, epoch)
                }
                Some((false, _, epoch, _)) => (false, None, epoch),
                None => (false, None, 0),
            };
        };

        let source_version = self
            .scheduler
            .try_get_source(&canonical)
            .map(|source| source.version());
        let integrated = !reload
            && self.dependency_cache().contains_key(&canonical)
            && source_version.is_some()
            && self
                .scheduler
                .try_get_analysis(&canonical)
                .is_some_and(|analysis| Some(analysis.version()) == source_version);
        let request = if integrated || stop.is_cancelled() {
            None
        } else {
            Some(self.scheduler.begin_owned_request(
                Request {
                    file_id: canonical.clone(),
                    target: TargetStage::Analysis,
                    priority: Priority::Interactive,
                    source: None,
                    file_language: None,
                    request_context: verter_execution::request_context::current_context(),
                },
                waiter,
                stop.clone(),
            ))
        };
        HostLoadHandoff {
            request,
            commit: Some(HostLoadCommit {
                host: self.instance_id,
                canonical,
                reload,
                pre_evict_hash,
                integrated,
                fence: HostLoadFence {
                    version: integrated.then_some(source_version).flatten(),
                    eviction_revision,
                    project_generation,
                    stop,
                },
            }),
        }
    }
}

/// A file's load the semantic runtime waits on instead of driving it: the
/// owned scheduler handoff, integrated on this host once Analysis is ready,
/// exactly as [`VerterHost::ensure_loaded`] integrates it, followed by the
/// existing pollable indexed materialization. Source presence alone is not
/// readiness. Its terminal outcome is kept and answered again to a later poll.
type IndexedMaterialization<'h> = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = Option<verter_session_query::inputs::indexed::IndexedInputServe>,
            > + Send
            + 'h,
    >,
>;

struct HostIndexedLoad<'h> {
    host: &'h VerterHost,
    load: Option<HostLoadHandoff>,
    indexing: Option<IndexedMaterialization<'h>>,
    indexed: Option<verter_session_query::inputs::indexed::IndexedInputServe>,
    task: ExecutionTask,
    stop: Option<verter_execution::cancellation::CancellationSubscription>,
    outcome: Option<verter_type_engine::resolver_core::request_ports::IndexedLoadOutcome>,
}

impl verter_type_engine::resolver_core::request_ports::IndexedLoad for HostIndexedLoad<'_> {
    fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<verter_type_engine::resolver_core::request_ports::IndexedLoadOutcome> {
        use verter_type_engine::resolver_core::request_ports::IndexedLoadOutcome;
        if let Some(outcome) = self.outcome {
            return Poll::Ready(outcome);
        }
        if self
            .stop
            .as_mut()
            .expect("live cancellation wait")
            .poll(waker)
            .is_ready()
        {
            self.load = None;
            self.indexing = None;
            self.stop = None;
            let outcome = IndexedLoadOutcome::NotIntegrated(
                verter_type_engine::resolver_core::request_ports::IndexedLoadEnd::Cancelled,
            );
            self.outcome = Some(outcome);
            return Poll::Ready(outcome);
        }
        let _task =
            verter_execution::tasks::ExecutionScope::install(&self.host.task_registry, &self.task);
        // A failed, stopped or superseded load integrates nothing; its
        // reader then answers as after a synchronous load that failed.
        if let Some(load) = self.load.as_mut() {
            let ended = match load.poll(waker) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(ended) => ended,
            };
            self.load = None;
            match ended.and_then(|commit| commit.apply(self.host)) {
                Ok(true) => {}
                ended => {
                    let outcome = IndexedLoadOutcome::NotIntegrated(match ended {
                        Err(error) => error.load_end(),
                        Ok(false) => {
                            verter_type_engine::resolver_core::request_ports::IndexedLoadEnd::Absent
                        }
                        Ok(true) => unreachable!(),
                    });
                    self.outcome = Some(outcome);
                    self.indexing = None;
                    self.stop = None;
                    return Poll::Ready(outcome);
                }
            }
        }
        let outcome = match self
            .indexing
            .as_mut()
            .expect("live indexed computation")
            .as_mut()
            .poll(&mut std::task::Context::from_waker(waker))
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Some(serve)) => {
                self.indexed = Some(serve);
                IndexedLoadOutcome::Integrated
            }
            Poll::Ready(None) => IndexedLoadOutcome::NotIntegrated(
                verter_type_engine::resolver_core::request_ports::IndexedLoadEnd::Absent,
            ),
        };
        self.indexing = None;
        self.stop = None;
        self.outcome = Some(outcome);
        Poll::Ready(outcome)
    }

    fn take_indexed_input(
        &mut self,
    ) -> Option<verter_session_query::inputs::indexed::IndexedInputServe> {
        self.indexed.take()
    }
}

impl HostLoadError {
    /// The typed end of an owned load this error stopped.
    fn load_end(&self) -> verter_type_engine::resolver_core::request_ports::IndexedLoadEnd {
        use verter_type_engine::resolver_core::request_ports::IndexedLoadEnd;
        match self {
            HostLoadError::Cancelled => IndexedLoadEnd::Cancelled,
            HostLoadError::Cycle => IndexedLoadEnd::Cycle,
            HostLoadError::Superseded => IndexedLoadEnd::Superseded,
            HostLoadError::Shutdown => IndexedLoadEnd::Shutdown,
            HostLoadError::Failed(_) | HostLoadError::WrongHost | HostLoadError::NoProgress => {
                IndexedLoadEnd::Failed
            }
        }
    }
}

impl VerterHost {
    /// Whether `canonical_id`'s indexed input is servable without loading
    /// it, or the owned load to wait on first
    /// ([`verter_type_engine::resolver_core::request_ports::IndexedInputs::begin_indexed_load`]).
    /// A cold index retains its pollable materialization even when its source
    /// is present. Missing source first joins the scheduler load. A scheduler
    /// without a driver thread (every wasm32
    /// host) has no executor but the caller, so nothing would advance a load
    /// begun here: the serve performs it in place, executing its stages on
    /// this thread, as the synchronous load does.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn begin_indexed_ready_load(
        &self,
        canonical_id: &str,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'_> {
        self.begin_indexed_ready_load_for(canonical_id, &self.source_input_leases)
    }

    pub(crate) fn begin_indexed_ready_load_for<'h>(
        &'h self,
        canonical_id: &str,
        leases: &'h crate::resolver_core::request_inputs::InputArtifactLeases,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'h> {
        use verter_type_engine::resolver_core::request_ports::IndexedReadiness;
        if !self.scheduler.has_driver_thread() {
            return IndexedReadiness::Settled;
        }
        let canonical = self
            .normalized_analysis_canonical(canonical_id)
            .into_owned();
        if canonical.is_empty() || crate::host_manage::is_raw_import_specifier_id(&canonical) {
            return IndexedReadiness::Settled;
        }
        let observation = verter_type_engine::request_observers::CaptureTargets::installed::<
            verter_audit::observe::Uncaptured,
        >();
        if self
            .artifact_current_indexed_raw::<verter_audit::observe::Uncaptured>(
                &observation,
                &canonical,
            )
            .is_some_and(|indexed| self.indexed_surface_is_current(&canonical, &indexed))
        {
            return IndexedReadiness::Settled;
        }
        let scope = self.scheduler.enter_execution_scope();
        let stop = self.cooperative_drive.cancellation().clone();
        let load = self
            .scheduler
            .try_get_source(&canonical)
            .is_none()
            .then(|| self.begin_load(&canonical, scope.task().clone(), stop.clone()));
        IndexedReadiness::Loading(Box::new(HostIndexedLoad {
            host: self,
            load,
            indexing: Some(Box::pin(async move {
                self.ensure_indexed_ready_serve_pollable(&canonical)
                    .await
                    .map(|serve| leases.retain(serve))
            })),
            indexed: None,
            task: scope.task().clone(),
            stop: Some(stop.subscribe()),
            outcome: None,
        }))
    }
}
