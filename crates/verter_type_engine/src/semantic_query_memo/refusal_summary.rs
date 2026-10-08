//! Sealed refusal summaries: what an isolated root demand refused, kept so
//! an exact repeat answers without evaluating again.
//!
//! A root demand that runs out of an allowance returns a typed partial the
//! memo never admits, so without a summary every repeat would re-run the
//! failed evaluation only to fail at the same point. The cost model makes
//! that point a function of the demand, its inputs and its allowances —
//! never of warmth or schedule — so the refusal can be sealed once and
//! replayed: the summary keeps the refusal read, the facts the failed
//! evaluation observed, and the charges it made before the trip.
//!
//! A summary answers only the exact refusal identity it was sealed under:
//! the same canonical root query, entered as an isolated root (a fresh
//! connected demand at a fresh request, outside any checker obligation),
//! under the same interned [`BudgetProfile`]. A demand with a different
//! remaining allowance, an already-paid set or an enclosing context is not
//! that identity and never consults it; a higher allowance is a different
//! profile and evaluates (and, completing, heals into the ordinary memo).
//! Delivery revalidates the observed facts against the caller's view, so
//! an edit to anything the failed evaluation read misses and re-evaluates.
//! Retention is a bounded FIFO, cleared with the memo.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::project_semantic_dispatch::cost_receipt::BudgetProfile;
use crate::semantic_query::{CacheRead, PartialReasonSet, QueryResult, SemanticQueryValue};

use super::prepared::PreparedKeyHandle;
use super::SemanticGraphStore;

/// Refusals the store keeps at once; the oldest is dropped first.
pub const REFUSAL_SUMMARY_CAP: usize = 256;

/// The charges a failed evaluation made before it tripped: what applying
/// its summary leaves on the repeat's demand and request, exactly as the
/// evaluation would have.
#[derive(Debug, Clone)]
pub struct RefusalPrefix {
    /// Work units the connected demand charged.
    pub work: usize,
    /// Construction bytes the connected demand reserved.
    pub bytes: usize,
    /// The request the evaluation ran in: where it entered it, and what it
    /// left it at. `None` outside any request.
    pub request: Option<crate::request_budget::RequestSpent>,
    /// The rails that tripped.
    pub trip: PartialReasonSet,
}

/// One sealed refusal.
#[derive(Debug)]
pub struct RefusalSummary {
    /// The refusal read the failed evaluation answered with.
    pub read: CacheRead<QueryResult<SemanticQueryValue>>,
    /// What the failed evaluation observed: the validity rail.
    pub carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
    /// The carrier's strict self-roots.
    pub self_root_canonicals: Arc<[Arc<str>]>,
    /// The charges made before the trip.
    pub prefix: RefusalPrefix,
}

type RefusalKey = (PreparedKeyHandle, BudgetProfile);

/// The store's bounded refusal-summary table.
#[derive(Default)]
pub(crate) struct RefusalSummaries {
    entries: FxHashMap<RefusalKey, Arc<RefusalSummary>>,
    order: VecDeque<RefusalKey>,
}

impl SemanticGraphStore {
    /// The sealed refusal for the isolated root `prepared` under `profile`,
    /// entered at the caller's request state (`request`, `None` outside any
    /// request), when one validates for the caller's view.
    pub(crate) fn sealed_refusal<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        prepared: &PreparedKeyHandle,
        profile: &BudgetProfile,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<RefusalSummary>> {
        let summary = {
            let summaries = self.refusal_summaries.lock();
            if summaries.entries.is_empty() {
                return None;
            }
            summaries
                .entries
                .get(&(prepared.clone(), profile.clone()))
                .cloned()?
        };
        let same_entry = match (request, &summary.prefix.request) {
            (Some(request), Some(spent)) => request.is_at(spent),
            (None, None) => true,
            _ => false,
        };
        (same_entry
            && self.serves_stored_value(&summary.read.value)
            && summary
                .carrier
                .validate_with_self_roots(ctx, &summary.self_root_canonicals))
        .then_some(summary)
    }

    /// Seal `summary` as the refusal of the isolated root `prepared` under
    /// `profile`, entered at project generation `entered_at`. Refused —
    /// nothing kept — for a cancelled evaluation, an overflowed fact rail,
    /// or one the project moved under: a torn evaluation never becomes a
    /// summary. Its facts are revalidated at every delivery.
    pub(crate) fn seal_refusal<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        prepared: PreparedKeyHandle,
        profile: BudgetProfile,
        entered_at: u64,
        summary: RefusalSummary,
    ) -> bool {
        let flags = ctx.request_flags();
        if summary.carrier.overflowed
            || flags.is_cancelled()
            || flags.current_project_generation() != entered_at
        {
            return false;
        }
        let key = (prepared, profile);
        let mut summaries = self.refusal_summaries.lock();
        if summaries
            .entries
            .insert(key.clone(), Arc::new(summary))
            .is_none()
        {
            summaries.order.push_back(key);
        }
        while summaries.order.len() > REFUSAL_SUMMARY_CAP {
            if let Some(oldest) = summaries.order.pop_front() {
                summaries.entries.remove(&oldest);
            }
        }
        true
    }

    /// Whether a stored `result` may still be served: it names no released
    /// node and no retired signature-kernel epoch — the gates every warm
    /// read applies besides its fact rail.
    pub(crate) fn serves_stored_value(&self, result: &QueryResult<SemanticQueryValue>) -> bool {
        self.result_is_live(result) && !self.names_retired_kernel_epoch(result)
    }

    /// Drop every sealed refusal.
    pub(crate) fn clear_refusal_summaries(&self) {
        let mut summaries = self.refusal_summaries.lock();
        summaries.entries.clear();
        summaries.order.clear();
    }

    /// How many sealed refusals the store keeps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_summary_count_for_tests(&self) -> usize {
        self.refusal_summaries.lock().entries.len()
    }

    /// Seal a request-less refusal of `key` under `profile`, entered at
    /// project generation `entered_at`, through the production seal.
    #[cfg(any(test, feature = "test-support"))]
    pub fn seal_refusal_for_tests<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: crate::semantic_query::SemanticQueryKey,
        profile: BudgetProfile,
        entered_at: u64,
        read: CacheRead<QueryResult<SemanticQueryValue>>,
        carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
    ) -> bool {
        self.seal_refusal(
            ctx,
            PreparedKeyHandle::prepare(key),
            profile,
            entered_at,
            RefusalSummary {
                read,
                carrier,
                self_root_canonicals: Arc::from([]),
                prefix: RefusalPrefix {
                    work: 0,
                    bytes: 0,
                    request: None,
                    trip: PartialReasonSet::PROJECTION_WORK_LIMIT,
                },
            },
        )
    }

    /// Whether a request-less isolated root of `key` under `profile` would
    /// answer from a sealed refusal now.
    #[cfg(any(test, feature = "test-support"))]
    pub fn has_sealed_refusal_for_tests<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: crate::semantic_query::SemanticQueryKey,
        profile: &BudgetProfile,
    ) -> bool {
        self.sealed_refusal(ctx, &PreparedKeyHandle::prepare(key), profile, None)
            .is_some()
    }
}

/// The store field type.
pub(crate) type RefusalSummaryTable = Mutex<RefusalSummaries>;
