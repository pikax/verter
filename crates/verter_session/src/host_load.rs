//! Owned scheduler-backed loading. Polling transfers host integration to the
//! external caller; it never resolves workspace routes or drives a stage.

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
        let project_generation = self.project_type_store.project_generation();
        let canonical = self.normalized_analysis_canonical(canonical).into_owned();
        let (reload, pre_evict_hash, eviction_revision) = self
            .derived_raw_cache()
            .get(&canonical)
            .map(|d| (d.evicted, d.evicted_whole_hash, d.eviction_revision()))
            .unwrap_or((false, None, 0));
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
            if reload {
                self.scheduler.close_file(&canonical);
            }
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
/// exactly as [`VerterHost::ensure_loaded`] integrates it.
struct HostIndexedLoad<'h> {
    host: &'h VerterHost,
    load: Option<HostLoadHandoff>,
}

impl verter_type_engine::resolver_core::request_ports::IndexedLoad for HostIndexedLoad<'_> {
    fn poll(&mut self, waker: &Waker) -> Poll<()> {
        let Some(load) = self.load.as_mut() else {
            return Poll::Ready(());
        };
        let outcome = match load.poll(waker) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(outcome) => outcome,
        };
        self.load = None;
        // A failed, stopped or superseded load integrates nothing; the serve
        // then answers as it answers after a synchronous load that failed.
        if let Ok(commit) = outcome {
            let _ = commit.apply(self.host);
        }
        Poll::Ready(())
    }
}

impl VerterHost {
    /// Whether `canonical_id`'s indexed input is servable without loading
    /// it, or the owned load to wait on first
    /// ([`verter_type_engine::resolver_core::request_ports::IndexedInputs::begin_indexed_load`]).
    /// A load is begun exactly where the synchronous serve would load: a
    /// loadable canonical the scheduler holds no source for and no retained
    /// artifact serves.
    pub(crate) fn begin_indexed_ready_load(
        &self,
        canonical_id: &str,
    ) -> verter_type_engine::resolver_core::request_ports::IndexedReadiness<'_> {
        use verter_type_engine::resolver_core::request_ports::IndexedReadiness;
        let canonical = self
            .normalized_analysis_canonical(canonical_id)
            .into_owned();
        if canonical.is_empty()
            || crate::host_manage::is_raw_import_specifier_id(&canonical)
            || self.scheduler.try_get_source(&canonical).is_some()
        {
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
        let load = self.begin_load(
            &canonical,
            scope.task().clone(),
            self.cooperative_drive.cancellation().clone(),
        );
        IndexedReadiness::Loading(Box::new(HostIndexedLoad {
            host: self,
            load: Some(load),
        }))
    }
}
