//! Read and admission facades over the final component-meta result store.
//!
//! The store ([`ComponentMetaResultDb`]) is engine-owned storage; this module
//! owns the native payload decision and optional hit/miss observations.
//! Schema, dependency and publication checks run through narrow operations of the
//! request's [`ProjectSemanticDispatch`], so the facade reads, validates and
//! traces against exactly the request the dispatch serves, in the original
//! order.

#[cfg(any(test, feature = "semantic-observe"))]
use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_session_query::analysis::types::Hash16;
use verter_type_engine::project_semantic_dispatch::memo::ComponentMetaTrace;

use crate::component_meta_result_db::{
    AdmittedComponentMetaResult, ComponentMetaPublishDecision, ComponentMetaResultDb,
    ComponentMetaResultEntry, ComponentMetaResultKey,
};
use crate::meta_provenance::MetaProvenance;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;

/// Warm reads of the final component-meta result store, validated against one
/// request.
pub(crate) struct ComponentMetaResultRead<'a, 'c, P, C: ResolverCapabilities> {
    db: &'a ComponentMetaResultDb<P>,
    dispatch: &'a ProjectSemanticDispatch<'c, C>,
    #[cfg(any(test, feature = "semantic-observe"))]
    observations: &'a MetaProvenance,
}

impl<'a, 'c, P: Send + Sync, C: ResolverCapabilities> ComponentMetaResultRead<'a, 'c, P, C> {
    pub(crate) fn new(
        dispatch: &'a ProjectSemanticDispatch<'c, C>,
        db: &'a ComponentMetaResultDb<P>,
        observations: &'a MetaProvenance,
    ) -> Self {
        #[cfg(not(any(test, feature = "semantic-observe")))]
        let _ = observations;
        Self {
            db,
            dispatch,
            #[cfg(any(test, feature = "semantic-observe"))]
            observations,
        }
    }

    pub(crate) fn peek(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<Arc<ComponentMetaResultEntry<P>>> {
        #[cfg(any(test, feature = "semantic-observe"))]
        let bump_miss = |observations: &MetaProvenance| {
            observations
                .component_meta_result_cache_misses
                .fetch_add(1, Ordering::Relaxed);
            // Keep the per-request `cache_layers.component_meta` audit
            // counter in sync with the `.get()` accessor so
            // joiner-accounting assertions continue to attribute a miss
            // to the cold winner.
            if let Some(ctx) = verter_type_engine::request_context::current_request_context() {
                ctx.cache_counters
                    .component_meta
                    .misses
                    .fetch_add(1, Ordering::Relaxed);
            }
        };
        let candidate =
            match self
                .dispatch
                .read_component_meta_result(self.db, key, owner_whole_hash)
            {
                Some(candidate) => candidate,
                None => {
                    #[cfg(any(test, feature = "semantic-observe"))]
                    bump_miss(self.observations);
                    return None;
                }
            };
        #[cfg(any(test, feature = "semantic-observe"))]
        if let Some(ctx) = verter_type_engine::request_context::current_request_context() {
            ctx.cache_counters
                .component_meta
                .hits
                .fetch_add(1, Ordering::Relaxed);
        }
        #[cfg(any(test, feature = "semantic-observe"))]
        self.observations
            .component_meta_result_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        Some(candidate)
    }
}

/// Count a component-meta result miss the caller decided before reaching the
/// store (no current view to validate against).
pub(crate) fn record_component_meta_result_miss(observations: &MetaProvenance) {
    #[cfg(not(any(test, feature = "semantic-observe")))]
    let _ = observations;
    #[cfg(any(test, feature = "semantic-observe"))]
    observations
        .component_meta_result_cache_misses
        .fetch_add(1, Ordering::Relaxed);
    #[cfg(any(test, feature = "semantic-observe"))]
    if let Some(ctx) = verter_type_engine::request_context::current_request_context() {
        ctx.cache_counters
            .component_meta
            .misses
            .fetch_add(1, Ordering::Relaxed);
    }
}

/// Cold compute plus typed admission into the final component-meta result
/// store, traced against one request.
pub(crate) struct ComponentMetaResultPublish<'a, 'c, P, C: ResolverCapabilities> {
    db: &'a ComponentMetaResultDb<P>,
    dispatch: &'a ProjectSemanticDispatch<'c, C>,
}

impl<'a, 'c, P: Send + Sync, C: ResolverCapabilities> ComponentMetaResultPublish<'a, 'c, P, C> {
    pub(crate) fn new(
        dispatch: &'a ProjectSemanticDispatch<'c, C>,
        db: &'a ComponentMetaResultDb<P>,
    ) -> Self {
        Self { db, dispatch }
    }

    /// Run `compute` under the request's fact tracer, then admit the value
    /// `decide` selects only when the finalised read set is cacheable. The
    /// finalised evidence always precedes the passive publication; a refused
    /// retention reservation leaves the admitted carrier unset.
    pub(crate) fn compute_and_admit_with_entry<R, Compute, Decide>(
        &self,
        _canonical: &str,
        _path_label: &str,
        compute: Compute,
        decide: Decide,
    ) -> (R, Option<AdmittedComponentMetaResult<P>>)
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> ComponentMetaPublishDecision<P>,
        P: verter_session_query::retention::RetainedFootprint,
    {
        let (value, finalise) = self
            .dispatch
            .traced_component_meta_compute(_canonical, compute);
        let mut admitted = None;
        match finalise {
            ComponentMetaTrace::Observed(evidence) => match decide(&value) {
                ComponentMetaPublishDecision::Publish {
                    key,
                    owner_whole_hash,
                    payload,
                    validated_at_generation,
                } => {
                    if let Some(entry) = self.dispatch.admit_component_meta_result(
                        self.db,
                        key.clone(),
                        owner_whole_hash,
                        evidence,
                        payload,
                        validated_at_generation,
                    ) {
                        admitted = Some(AdmittedComponentMetaResult {
                            key,
                            owner_whole_hash,
                            entry,
                        });
                    }
                }
                ComponentMetaPublishDecision::ReturnOnly(reason) => {
                    verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                    #[cfg(feature = "semantic-observe")]
                    tracing::debug!(
                        target: "verter::audit::record",
                        file = %_canonical,
                        path = %_path_label,
                        reason = %reason,
                        "skipping component-meta cache promotion: typed admission refusal",
                    );
                }
                ComponentMetaPublishDecision::NoValue => {}
            },
            ComponentMetaTrace::ReturnOnly(reason) => {
                verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                #[cfg(feature = "semantic-observe")]
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %_canonical,
                    path = %_path_label,
                    reason = %reason,
                    "skipping component-meta cache promotion: trace refused publication evidence",
                );
            }
        }
        (value, admitted)
    }

    pub(crate) fn compute_and_admit<R, Compute, Decide>(
        &self,
        canonical: &str,
        path_label: &str,
        compute: Compute,
        decide: Decide,
    ) -> R
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> ComponentMetaPublishDecision<P>,
        P: verter_session_query::retention::RetainedFootprint,
    {
        self.compute_and_admit_with_entry(canonical, path_label, compute, decide)
            .0
    }
}
