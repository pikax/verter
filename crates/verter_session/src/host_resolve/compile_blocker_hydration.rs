//! Compile-blocker hydration: resolve and load the files a compile reads
//! before it captures its input.
//!
//! A Vue carrier names two kinds of blocker: external block sources
//! (`<template src>`, `<style src>`, ...) and the files its type-based
//! macros import their types from. Hydration resolves each blocker route,
//! records the owner's dependency edge for every resolved one, and loads
//! each resolved blocker. Loading is load only, not indexed materialization:
//! the macro traversal indexes what it reads.
//!
//! The computation is pollable. Each route resolves through the workspace's
//! owned resolution handoff: while one is pending, the computation hands
//! its driver the resolution's producer and returns `Pending`. Each load
//! goes through the owned load handoff: while one is pending, the
//! computation hands its driver a scheduler dispatch step and returns
//! `Pending`. So a compile root never parks inside its own poll on a
//! blocker route or load. The synchronous entry runs the same computation
//! on its calling thread, resolving each route and loading each blocker in
//! place, as the synchronous resolution and load entries do.
//!
//! The outcome is typed. A route that could not be admitted, or a blocker
//! whose load failed, is recorded as a gap and hydration continues with the
//! remaining blockers. A load stopped by cancellation, supersession or
//! shutdown is not a gap: it ends owned hydration with that abort, before
//! the compile captures anything. Owned hydration also stops when the
//! request it runs for is cancelled: it waits on each route and load
//! together with a subscription on the request's cancellation, so the
//! cancellation itself wakes it. A computation begun outside a request has
//! no request cancellation and stops only through the host's own.

use std::collections::BTreeSet;
use std::future::Future;
use std::task::Poll;

use crate::host_load::HostLoadError;
use crate::root_continuation::{RootWork, RootWorkSlot};
use crate::types::CompileDependencyGap;
use crate::VerterHost;
use verter_execution::cancellation::CancellationSubscription;
use verter_scheduler::job::SchedulerError;
use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};
use verter_type_engine::semantic_query::ExecutionAbort;

/// How far hydration established a compile's blockers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockerHydration {
    /// Every blocker route was resolved or witnessed missing, and every
    /// resolved blocker is loaded.
    Complete,
    /// Sorted, deduplicated, never empty.
    Incomplete(Vec<CompileDependencyGap>),
}

impl BlockerHydration {
    fn from_gaps(gaps: BTreeSet<CompileDependencyGap>) -> Self {
        if gaps.is_empty() {
            Self::Complete
        } else {
            Self::Incomplete(gaps.into_iter().collect())
        }
    }

    /// The gaps hydration recorded; empty when it completed.
    pub(crate) fn gaps(&self) -> &[CompileDependencyGap] {
        match self {
            Self::Complete => &[],
            Self::Incomplete(gaps) => gaps,
        }
    }
}

/// How hydration resolves a blocker route and loads a resolved blocker.
#[derive(Clone, Copy)]
enum BlockerLoads {
    /// Through the owned resolution and load handoffs, suspending while a
    /// resolution or a load is pending.
    Owned,
    /// In place on the calling thread, as the synchronous resolution and
    /// load entries do.
    InPlace,
}

fn blocker_context(kind: ResolveRequestKind) -> ResolutionContext {
    ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind,
    }
}

/// A blocker load's answer: whether it integrated, or the stop that ended
/// it. Only a load that genuinely failed is "not integrated".
pub(super) fn blocker_load_answer(
    outcome: Result<bool, HostLoadError>,
) -> Result<bool, ExecutionAbort> {
    match outcome {
        Ok(integrated) => Ok(integrated),
        Err(HostLoadError::Cancelled) => Err(ExecutionAbort::Cancelled),
        Err(HostLoadError::Superseded) => Err(ExecutionAbort::Superseded),
        Err(HostLoadError::Shutdown | HostLoadError::Failed(SchedulerError::Shutdown)) => {
            Err(ExecutionAbort::Shutdown)
        }
        Err(
            HostLoadError::Failed(_)
            | HostLoadError::Cycle
            | HostLoadError::WrongHost
            | HostLoadError::NoProgress,
        ) => Ok(false),
    }
}

/// Whether the request `stop` subscribes to is cancelled; otherwise
/// `waker` is registered to be woken when it is.
fn request_stopped(stop: &mut Option<CancellationSubscription>, waker: &std::task::Waker) -> bool {
    stop.as_mut()
        .is_some_and(|stop| stop.poll(waker).is_ready())
}

/// `computation`, ended with `Cancelled` as soon as the request `stop`
/// subscribes to is cancelled. The subscription is checked before each
/// poll of `computation` and again after one that is pending, so a
/// cancellation between the two is still observed, and the cancellation
/// itself wakes the waiting computation.
async fn until_request_stops<F: Future>(
    stop: &mut Option<CancellationSubscription>,
    computation: F,
) -> Result<F::Output, ExecutionAbort> {
    let mut computation = std::pin::pin!(computation);
    std::future::poll_fn(|cx| {
        if request_stopped(stop, cx.waker()) {
            return Poll::Ready(Err(ExecutionAbort::Cancelled));
        }
        match computation.as_mut().poll(cx) {
            Poll::Ready(outcome) => Poll::Ready(Ok(outcome)),
            Poll::Pending if request_stopped(stop, cx.waker()) => {
                Poll::Ready(Err(ExecutionAbort::Cancelled))
            }
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

impl VerterHost {
    /// Hydrate `canonical`'s compile blockers as a pollable computation run
    /// as the execution task installed on the polling thread.
    ///
    /// While a blocker route is resolving, the computation hands the driver
    /// the resolution's producer through `work` and returns `Pending`; while
    /// a blocker load is pending, it hands the driver a scheduler dispatch
    /// step. The resolution's or the load's completion, the cancellation of
    /// the request installed while it runs, or the driver's next poll,
    /// resumes it.
    ///
    /// A load stopped by cancellation, supersession or shutdown, or that
    /// request's cancellation, ends hydration with that abort.
    pub(crate) async fn hydrate_compile_blockers_pollable<'h>(
        &'h self,
        canonical: &str,
        work: &RootWorkSlot<'h>,
    ) -> Result<BlockerHydration, ExecutionAbort> {
        self.hydrate_blockers(canonical, work, BlockerLoads::Owned)
            .await
    }

    async fn hydrate_blockers<'h>(
        &'h self,
        canonical: &str,
        work: &RootWorkSlot<'h>,
        loads: BlockerLoads,
    ) -> Result<BlockerHydration, ExecutionAbort> {
        let Some(blockers) = self.get_compile_blockers(canonical) else {
            return Ok(BlockerHydration::Complete);
        };
        // In place, every route and load answers before its poll returns,
        // so there is nothing a request's cancellation could interrupt.
        let mut stop = match loads {
            BlockerLoads::Owned => {
                verter_type_engine::request_context::current_request_cancellation_token()
                    .map(|token| token.subscribe())
            }
            BlockerLoads::InPlace => None,
        };

        let mut gaps = BTreeSet::new();
        let mut blocker_ids = BTreeSet::new();
        let mut resolved_routes = Vec::new();
        let refused_route = |specifier: &str| CompileDependencyGap::Route {
            importer: canonical.to_owned(),
            specifier: specifier.to_owned(),
            refused: true,
        };

        // Blocker hydration and dependency edges are import-graph
        // bookkeeping: they take every complete answer, retained or not.
        for request in blockers.external_source_requests {
            let route = self
                .resolve_blocker_route(
                    canonical,
                    &request.specifier,
                    ResolveRequestKind::SfcSrcAttr,
                    work,
                    loads,
                    &mut stop,
                )
                .await?;
            let resolved = match route {
                Some(Some(resolved)) => {
                    resolved_routes.push(resolved.clone());
                    resolved
                }
                // A witnessed miss falls back to the parse-time canonical.
                Some(None) => request.resolved_canonical_id,
                None => {
                    gaps.insert(refused_route(&request.specifier));
                    continue;
                }
            };
            if resolved != canonical {
                blocker_ids.insert(resolved);
            }
        }

        for dep in blockers.macro_type_deps.iter() {
            let source = dep.import_source.as_str();
            // A type route with no target falls back to the module route.
            let typed = self
                .resolve_blocker_route(
                    canonical,
                    source,
                    ResolveRequestKind::TypeImport,
                    work,
                    loads,
                    &mut stop,
                )
                .await?;
            let route = match typed {
                Some(None) => {
                    self.resolve_blocker_route(
                        canonical,
                        source,
                        ResolveRequestKind::EsmImport,
                        work,
                        loads,
                        &mut stop,
                    )
                    .await?
                }
                route => route,
            };
            match route {
                Some(Some(resolved)) => {
                    resolved_routes.push(resolved.clone());
                    if resolved != canonical {
                        blocker_ids.insert(resolved);
                    }
                }
                Some(None) => {}
                None => {
                    gaps.insert(refused_route(source));
                }
            }
        }

        for resolved in resolved_routes {
            self.record_resolved_dependency_edge(canonical, &resolved);
        }
        // A request cancelled before the first load begins none.
        if stop
            .as_ref()
            .is_some_and(|stop| stop.token().is_cancelled())
        {
            return Err(ExecutionAbort::Cancelled);
        }
        for blocker_id in blocker_ids {
            if !self
                .load_compile_blocker(&blocker_id, work, loads, &mut stop)
                .await?
            {
                gaps.insert(CompileDependencyGap::File {
                    canonical: blocker_id,
                });
            }
        }
        Ok(BlockerHydration::from_gaps(gaps))
    }

    /// Resolve one blocker route: `Some(Some(target))` when it resolves,
    /// `Some(None)` when it is witnessed missing, `None` when it is refused.
    ///
    /// Owned hydration resolves through
    /// [`Self::resolve_for_persistent_state_pollable`], the synchronous one
    /// through [`Self::resolve_for_persistent_state`]; both take the same
    /// route and record the same witness and owner edge. An owned
    /// resolution ends with `Cancelled` when the request `stop` subscribes
    /// to is cancelled while it waits.
    async fn resolve_blocker_route<'h>(
        &'h self,
        canonical: &str,
        specifier: &str,
        kind: ResolveRequestKind,
        work: &RootWorkSlot<'h>,
        loads: BlockerLoads,
        stop: &mut Option<CancellationSubscription>,
    ) -> Result<Option<Option<String>>, ExecutionAbort> {
        #[cfg(test)]
        if !matches!(kind, ResolveRequestKind::SfcSrcAttr)
            && self
                .test_force
                .force_type_route_refusal_for_specifier
                .lock()
                .as_deref()
                == Some(specifier)
        {
            return Ok(None);
        }
        let context = blocker_context(kind);
        let publication = match loads {
            BlockerLoads::Owned => {
                until_request_stops(
                    stop,
                    self.resolve_for_persistent_state_pollable(canonical, specifier, context, work),
                )
                .await?
            }
            BlockerLoads::InPlace => {
                self.resolve_for_persistent_state(canonical, specifier, context)
            }
        };
        Ok(publication
            .into_delivered()
            .map(|resolution| resolution.map(|resolution| resolution.source_id)))
    }

    /// Load one blocker, as the task polling the computation. Answers
    /// whether the blocker is integrated, or the stop that ended its owned
    /// load: the load's own, or the cancellation of the request `stop`
    /// subscribes to. A stopped load is dropped and integrates nothing.
    async fn load_compile_blocker<'h>(
        &'h self,
        blocker_id: &str,
        work: &RootWorkSlot<'h>,
        loads: BlockerLoads,
        stop: &mut Option<CancellationSubscription>,
    ) -> Result<bool, ExecutionAbort> {
        if let BlockerLoads::InPlace = loads {
            return Ok(self.ensure_loaded(blocker_id));
        }
        let task = verter_execution::tasks::ExecutionScope::current(&self.task_registry)
            .unwrap_or_else(|| self.task_registry.register_task());
        let cancellation = self.cooperative_drive.cancellation().clone();
        let mut load = self.begin_load(blocker_id, task, cancellation);
        let loaded = self
            .offering_dispatch_steps(
                work,
                until_request_stops(stop, std::future::poll_fn(|cx| load.poll(cx.waker()))),
            )
            .await?;
        blocker_load_answer(loaded.and_then(|commit| commit.apply(self)))
    }

    /// `computation`, handing the driver a scheduler dispatch step through
    /// `work` each time it is pending: for a computation that waits on
    /// work the host's scheduler admits and dispatches.
    pub(super) async fn offering_dispatch_steps<'h, F: Future>(
        &'h self,
        work: &RootWorkSlot<'h>,
        computation: F,
    ) -> F::Output {
        let mut computation = std::pin::pin!(computation);
        std::future::poll_fn(|cx| match computation.as_mut().poll(cx) {
            Poll::Ready(outcome) => Poll::Ready(outcome),
            Poll::Pending => {
                let scheduler = std::sync::Arc::clone(self.scheduler());
                work.offer(RootWork::dispatch(move || {
                    scheduler.dispatch_for_pending_dependency()
                }));
                Poll::Pending
            }
        })
        .await
    }

    /// [`Self::hydrate_compile_blockers_pollable`] on this thread, resolving
    /// each route with [`Self::resolve_for_persistent_state`] and loading
    /// each blocker in place with [`Self::ensure_loaded`]: a blocker already
    /// loaded answers without waiting, and a load that cannot progress ends
    /// as not loaded rather than suspending.
    pub(crate) fn hydrate_compile_blockers(&self, canonical: &str) -> BlockerHydration {
        let work = RootWorkSlot::new();
        let hydration = self.hydrate_blockers(canonical, &work, BlockerLoads::InPlace);
        // In-place resolutions and loads answer before their poll returns,
        // so the computation never suspends, and it takes no request stop.
        let hydration = std::pin::pin!(hydration);
        verter_execution::blocking::complete_now(hydration)
            .unwrap_or_else(|_| unreachable!("in-place blocker hydration never stops typed"))
    }
}
