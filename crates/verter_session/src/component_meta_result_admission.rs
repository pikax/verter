//! Read and admission facades over the final component-meta result store.
//!
//! The store ([`ComponentMetaResultDb`]) is host-owned storage; this module
//! owns its store-specific policy — the schema and project-generation gates,
//! the hit/miss counters, the owner-route fact strip and the typed publish
//! decision. Every validation and trace runs through narrow operations of the
//! request's [`ProjectSemanticDispatch`], so the facade reads, validates and
//! traces against exactly the request the dispatch serves, in the original
//! order.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use verter_session_query::analysis::types::Hash16;
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;

use crate::component_meta_result_db::{
    strip_owner_route_fact, AdmittedComponentMetaResult, ComponentMetaPublishDecision,
    ComponentMetaResultDb, ComponentMetaResultEntry, ComponentMetaResultKey,
};
use crate::meta_provenance::MetaProvenance;
use verter_type_engine::project_semantic_dispatch::ProjectSemanticDispatch;
use verter_type_engine::resolver_core::ResolverCapabilities;

/// Warm reads of the final component-meta result store, validated against one
/// request.
pub(crate) struct ComponentMetaResultRead<'a, 'c, P, C: ResolverCapabilities> {
    db: &'a ComponentMetaResultDb<P>,
    dispatch: &'a ProjectSemanticDispatch<'c, C>,
    observations: &'a MetaProvenance,
}

impl<'a, 'c, P: Send + Sync, C: ResolverCapabilities> ComponentMetaResultRead<'a, 'c, P, C> {
    pub(crate) fn new(
        dispatch: &'a ProjectSemanticDispatch<'c, C>,
        db: &'a ComponentMetaResultDb<P>,
        observations: &'a MetaProvenance,
    ) -> Self {
        Self {
            db,
            dispatch,
            observations,
        }
    }

    pub(crate) fn peek(
        &self,
        key: &ComponentMetaResultKey,
        owner_whole_hash: Hash16,
    ) -> Option<Arc<ComponentMetaResultEntry<P>>> {
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
        if !self.db.is_current_schema() {
            bump_miss(self.observations);
            return None;
        }
        // Clone the candidate `Arc` out of the slot before validating —
        // a concurrent eviction cannot invalidate this borrow.
        let candidate = match self.db.candidate(key, owner_whole_hash) {
            Some(c) => c,
            None => {
                bump_miss(self.observations);
                return None;
            }
        };
        // Project-generation gate. The carrier validates only
        // file-content whole-hashes; a `ProjectGeneration` reset bumps
        // no file content, so an entry whose `validated_at_generation`
        // no longer equals the live generation is stale even though its
        // carrier still validates. Reject before the fact rail so the
        // miss is attributed correctly.
        if candidate.value.validated_at_generation != self.dispatch.current_project_generation() {
            bump_miss(self.observations);
            return None;
        }
        // Fact-precise validation: every entry in the signature must
        // validate under the live view. An empty signature trivially
        // passes (entries published outside an installed tracer scope —
        // typically test fixtures — fall through to the legacy validator
        // on the caller side).
        if !self
            .dispatch
            .validates_fact_signature(&candidate.value.read_set_signature.facts)
        {
            bump_miss(self.observations);
            return None;
        }
        if let Some(ctx) = verter_type_engine::request_context::current_request_context() {
            ctx.cache_counters
                .component_meta
                .hits
                .fetch_add(1, Ordering::Relaxed);
        }
        self.observations
            .component_meta_result_cache_hits
            .fetch_add(1, Ordering::Relaxed);
        Some(Arc::new(candidate.value.clone()))
    }
}

/// Count a component-meta result miss the caller decided before reaching the
/// store (no current view to validate against).
pub(crate) fn record_component_meta_result_miss(observations: &MetaProvenance) {
    observations
        .component_meta_result_cache_misses
        .fetch_add(1, Ordering::Relaxed);
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
        canonical: &str,
        path_label: &str,
        compute: Compute,
        decide: Decide,
    ) -> (R, Option<AdmittedComponentMetaResult<P>>)
    where
        Compute: FnOnce() -> R,
        Decide: FnOnce(&R) -> ComponentMetaPublishDecision<P>,
        P: verter_session_query::retention::RetainedFootprint,
    {
        let (value, finalise) = self.dispatch.traced_compute(compute);
        let mut admitted = None;
        match finalise {
            FactReadSetFinalise::Ok(facts) => match decide(&value) {
                ComponentMetaPublishDecision::Publish {
                    key,
                    owner_whole_hash,
                    payload,
                    validated_at_generation,
                } => {
                    let admitted_facts = strip_owner_route_fact(&key.owner_canonical, &facts);
                    let entry = Arc::new(ComponentMetaResultEntry {
                        payload,
                        read_set_signature:
                            verter_session_query::facts::fact_cache::ReadSetSignature::new(
                                admitted_facts,
                            ),
                        validated_at_generation,
                    });
                    // A retention refusal leaves `admitted` unset: the
                    // caller keeps its complete value, and no evidence
                    // carrier claims an entry the cache never stored.
                    if self
                        .db
                        .publish_core(key.clone(), owner_whole_hash, entry.as_ref().clone())
                    {
                        admitted = Some(AdmittedComponentMetaResult {
                            key,
                            owner_whole_hash,
                            entry,
                        });
                    }
                }
                ComponentMetaPublishDecision::ReturnOnly(reason) => {
                    verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                    tracing::debug!(
                        target: "verter::audit::record",
                        file = %canonical,
                        path = %path_label,
                        reason = %reason,
                        "skipping component-meta cache promotion: typed admission refusal",
                    );
                }
                ComponentMetaPublishDecision::NoValue => {}
            },
            FactReadSetFinalise::NonCacheable(_) => {
                let reason = verter_audit::NonAdmissionReason::UnresolvedProvenance;
                verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: cold compute consumed a non-cacheable read",
                );
            }
            FactReadSetFinalise::Overflow => {
                let reason = verter_audit::NonAdmissionReason::SignatureOverflow;
                verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: fact-signature overflowed cap",
                );
            }
            FactReadSetFinalise::MutationUnstable => {
                let reason = verter_audit::NonAdmissionReason::MutationUnstable;
                verter_type_engine::cache_runtime::admission::propagate_non_admission(reason);
                tracing::debug!(
                    target: "verter::audit::record",
                    file = %canonical,
                    path = %path_label,
                    "skipping component-meta cache promotion: a compaction domain advanced \
                     mid-compute",
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
