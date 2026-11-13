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
//! The computation is pollable. Each load goes through the owned load
//! handoff: while one is pending, the computation hands its driver a
//! scheduler dispatch step and returns `Pending`, so a compile root never
//! parks inside its own poll on a blocker load. The synchronous entry runs
//! the same computation on its calling thread and loads each blocker in
//! place, as the synchronous load entry does. Route resolution is still
//! answered synchronously inside the poll.
//!
//! The outcome is typed. A route that could not be admitted, or a blocker
//! that could not be loaded, is recorded as a gap and hydration continues
//! with the remaining blockers; it never stops early.

use std::collections::BTreeSet;
use std::future::Future;
use std::task::{Context, Poll, Waker};

use crate::root_continuation::{RootWork, RootWorkSlot};
use crate::types::CompileDependencyGap;
use crate::VerterHost;
use verter_session_query::resolution::{ResolutionContext, ResolvePhase, ResolveRequestKind};

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

/// How hydration loads a resolved blocker.
#[derive(Clone, Copy)]
enum BlockerLoads {
    /// Through the owned load handoff, suspending while a load is pending.
    Owned,
    /// In place on the calling thread, as the synchronous load entry does.
    InPlace,
}

fn blocker_context(kind: ResolveRequestKind) -> ResolutionContext {
    ResolutionContext {
        phase: ResolvePhase::CodegenBlocker,
        kind,
    }
}

impl VerterHost {
    /// Hydrate `canonical`'s compile blockers as a pollable computation run
    /// as the execution task installed on the polling thread.
    ///
    /// While a blocker load is pending, the computation hands the driver a
    /// scheduler dispatch step through `work` and returns `Pending`; the
    /// load's completion, or the driver's next poll, resumes it.
    pub(crate) async fn hydrate_compile_blockers_pollable<'h>(
        &'h self,
        canonical: &str,
        work: &RootWorkSlot<'h>,
    ) -> BlockerHydration {
        self.hydrate_blockers(canonical, work, BlockerLoads::Owned)
            .await
    }

    async fn hydrate_blockers<'h>(
        &'h self,
        canonical: &str,
        work: &RootWorkSlot<'h>,
        loads: BlockerLoads,
    ) -> BlockerHydration {
        let Some(blockers) = self.get_compile_blockers(canonical) else {
            return BlockerHydration::Complete;
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
            let route = self.resolve_blocker_route(
                canonical,
                &request.specifier,
                ResolveRequestKind::SfcSrcAttr,
            );
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
            let route =
                match self.resolve_blocker_route(canonical, source, ResolveRequestKind::TypeImport)
                {
                    Some(None) => {
                        self.resolve_blocker_route(canonical, source, ResolveRequestKind::EsmImport)
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
        for blocker_id in blocker_ids {
            if !self.load_compile_blocker(&blocker_id, work, loads).await {
                gaps.insert(CompileDependencyGap::File {
                    canonical: blocker_id,
                });
            }
        }
        BlockerHydration::from_gaps(gaps)
    }

    /// Resolve one blocker route: `Some(Some(target))` when it resolves,
    /// `Some(None)` when it is witnessed missing, `None` when it is refused.
    fn resolve_blocker_route(
        &self,
        canonical: &str,
        specifier: &str,
        kind: ResolveRequestKind,
    ) -> Option<Option<String>> {
        #[cfg(test)]
        if !matches!(kind, ResolveRequestKind::SfcSrcAttr)
            && self
                .test_force
                .force_type_route_refusal_for_specifier
                .lock()
                .as_deref()
                == Some(specifier)
        {
            return None;
        }
        self.resolve_for_persistent_state(canonical, specifier, blocker_context(kind))
            .into_delivered()
            .map(|resolution| resolution.map(|resolution| resolution.source_id))
    }

    /// Load one blocker, as the task polling the computation. Answers
    /// whether the blocker is integrated.
    async fn load_compile_blocker<'h>(
        &'h self,
        blocker_id: &str,
        work: &RootWorkSlot<'h>,
        loads: BlockerLoads,
    ) -> bool {
        if let BlockerLoads::InPlace = loads {
            return self.ensure_loaded(blocker_id);
        }
        let task = verter_execution::tasks::ExecutionScope::current(&self.task_registry)
            .unwrap_or_else(|| self.task_registry.register_task());
        let cancellation = self.cooperative_drive.cancellation().clone();
        let mut load = self.begin_load(blocker_id, task, cancellation);
        std::future::poll_fn(|cx| match load.poll(cx.waker()) {
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
        .and_then(|commit| commit.apply(self))
        .unwrap_or(false)
    }

    /// [`Self::hydrate_compile_blockers_pollable`] on this thread, loading
    /// each blocker in place with [`Self::ensure_loaded`]: a blocker already
    /// loaded answers without waiting, and a load that cannot progress ends
    /// as not loaded rather than suspending.
    pub(crate) fn hydrate_compile_blockers(&self, canonical: &str) -> BlockerHydration {
        let work = RootWorkSlot::new();
        let hydration = self.hydrate_blockers(canonical, &work, BlockerLoads::InPlace);
        let mut hydration = std::pin::pin!(hydration);
        // In-place loads answer before their poll returns, so the
        // computation never suspends.
        let polled = hydration
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()));
        match polled {
            Poll::Ready(outcome) => outcome,
            Poll::Pending => unreachable!("in-place blocker loads never suspend"),
        }
    }
}
