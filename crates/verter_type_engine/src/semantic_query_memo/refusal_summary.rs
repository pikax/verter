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
//! Retention is bounded twice: by the table's count cap (a FIFO, cleared
//! with the memo) and by the process retention account, which charges
//! every kept summary for the bytes it holds and refuses to keep one under
//! pressure — the refusal is then returned, never sealed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_session_query::retention::{ChargeClass, RetentionAdmission, RetentionCharge};

use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::project_semantic_dispatch::cost_receipt::{BudgetProfile, BudgetProfileSpec};
use crate::semantic_query::{
    CacheRead, PartialReasonSet, QueryResult, SemanticQueryKey, SemanticQueryValue,
};

use super::prepared::PreparedKeyHandle;
use super::SemanticGraphStore;

/// Refusals the store keeps at once; the oldest is dropped first.
pub const REFUSAL_SUMMARY_CAP: usize = 256;

/// The estimated bytes one observed fact, one named computation and one
/// replayed diagnostic of a summary keep alive.
const FACT_BYTES: usize = 64;
const IDENTITY_BYTES: usize = 64;
const DIAGNOSTIC_BYTES: usize = 256;

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
    /// did to it alone. `None` outside any request.
    pub request: Option<crate::request_budget::RequestSpent>,
    /// The rails that tripped; empty for an operation's own refusal.
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
    /// The summary's reservation against the process retention account,
    /// held for as long as the summary lives.
    retention: Option<RetentionCharge>,
}

impl RefusalSummary {
    /// A summary not yet sealed.
    pub fn new(
        read: CacheRead<QueryResult<SemanticQueryValue>>,
        carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        prefix: RefusalPrefix,
    ) -> Self {
        Self {
            read,
            carrier,
            self_root_canonicals,
            prefix,
            retention: None,
        }
    }

    /// The bytes the summary keeps alive.
    fn retained_bytes(&self) -> usize {
        let self_roots: usize = self
            .self_root_canonicals
            .iter()
            .map(|canonical| canonical.len() + std::mem::size_of::<Arc<str>>())
            .sum();
        std::mem::size_of::<Self>()
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
            + (self.carrier.facts.len() + self.read.dep_signature.len()) * FACT_BYTES
            + self.read.walker_diagnostics.len() * DIAGNOSTIC_BYTES
            + self_roots
            + self
                .prefix
                .request
                .as_ref()
                .map_or(0, |spent| spent.identities() * IDENTITY_BYTES)
    }
}

type RefusalKey = (PreparedKeyHandle, BudgetProfile);

/// The store's bounded refusal-summary table.
#[derive(Default)]
pub(crate) struct RefusalSummaries {
    entries: FxHashMap<RefusalKey, Arc<RefusalSummary>>,
    order: VecDeque<RefusalKey>,
}

/// The store field: the table, and how many summaries it holds, read
/// without its lock by every isolated root while it is empty.
#[derive(Default)]
pub(crate) struct RefusalSummaryTable {
    table: Mutex<RefusalSummaries>,
    len: AtomicUsize,
}

impl SemanticGraphStore {
    /// The sealed refusal for the isolated root `key` under the allowances
    /// `profile`, when one validates for the caller's view. The caller
    /// still checks the request state it enters at, applying the refusal's
    /// charges in the same step.
    pub(crate) fn sealed_refusal<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: &SemanticQueryKey,
        profile: &BudgetProfileSpec,
        request: Option<&crate::request_budget::RequestBudget>,
    ) -> Option<Arc<RefusalSummary>> {
        if self.refusal_summaries.len.load(Ordering::Acquire) == 0 {
            return None;
        }
        let refusal_key = (
            PreparedKeyHandle::prepare(key.clone()),
            BudgetProfile::intern(*profile),
        );
        let summary = self
            .refusal_summaries
            .table
            .lock()
            .entries
            .get(&refusal_key)
            .cloned()?;
        let enters_alike = request.is_some() == summary.prefix.request.is_some();
        (enters_alike
            && self.serves_stored_value(&summary.read.value)
            && summary
                .carrier
                .validate_with_self_roots(ctx, &summary.self_root_canonicals))
        .then_some(summary)
    }

    /// Seal `summary` as the refusal of the isolated root `key` under
    /// `profile`, entered at project generation `entered_at`. Refused —
    /// nothing kept — for a cancelled evaluation, an overflowed fact rail,
    /// one the project moved under (a torn evaluation never becomes a
    /// summary), or one the retention account declines to keep. Its facts
    /// are revalidated at every delivery. A re-sealed refusal is the
    /// newest.
    pub(crate) fn seal_refusal<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        profile: BudgetProfile,
        entered_at: u64,
        mut summary: RefusalSummary,
    ) -> bool {
        let flags = ctx.request_flags();
        if summary.carrier.overflowed
            || flags.is_cancelled()
            || flags.current_project_generation() != entered_at
        {
            return false;
        }
        match self
            .retention_account()
            .reserve(ChargeClass::Retained, summary.retained_bytes())
        {
            RetentionAdmission::Admitted(charge) => summary.retention = Some(charge),
            RetentionAdmission::Refused(refusal) => {
                crate::cache_runtime::admission::propagate_non_admission(
                    refusal.non_admission_reason(),
                );
                return false;
            }
        }
        let key = (PreparedKeyHandle::prepare(key), profile);
        let evicted = {
            let mut summaries = self.refusal_summaries.table.lock();
            if summaries
                .entries
                .insert(key.clone(), Arc::new(summary))
                .is_some()
            {
                summaries.order.retain(|kept| kept != &key);
            }
            summaries.order.push_back(key);
            let mut evicted = Vec::new();
            while summaries.order.len() > REFUSAL_SUMMARY_CAP {
                if let Some(oldest) = summaries.order.pop_front() {
                    evicted.extend(summaries.entries.remove(&oldest));
                }
            }
            self.refusal_summaries
                .len
                .store(summaries.entries.len(), Ordering::Release);
            evicted
        };
        // Released outside the table's lock.
        drop(evicted);
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
        let cleared = {
            let mut summaries = self.refusal_summaries.table.lock();
            summaries.order.clear();
            self.refusal_summaries.len.store(0, Ordering::Release);
            std::mem::take(&mut summaries.entries)
        };
        drop(cleared);
    }

    /// How many sealed refusals the store keeps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_summary_count_for_tests(&self) -> usize {
        self.refusal_summaries.table.lock().entries.len()
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
            key,
            profile,
            entered_at,
            RefusalSummary::new(
                read,
                carrier,
                Arc::from([]),
                RefusalPrefix {
                    work: 0,
                    bytes: 0,
                    request: None,
                    trip: PartialReasonSet::PROJECTION_WORK_LIMIT,
                },
            ),
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
        self.sealed_refusal(ctx, &key, profile.spec(), None)
            .is_some()
    }
}
