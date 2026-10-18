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
//! A kept summary is registered in the table's own direct-edge reverse
//! index, failed-prefix receipts included, so an explicit invalidation of
//! any canonical the failed evaluation reached drops it as it drops the
//! memo's candidates; the index never decides validity.
//! Retention is bounded twice: by the table's count cap (a FIFO, cleared
//! with the memo) and by the process retention account, which charges
//! every kept summary for the bytes it holds and refuses to keep one under
//! pressure — the refusal is then returned, never sealed.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_session_query::retention::RetentionAdmission;

use crate::fact_signature_helpers::ReadSetSignatureExt as _;
use crate::project_semantic_dispatch::cost_receipt::{BudgetProfile, BudgetProfileSpec};
use crate::semantic_query::{
    CacheRead, PartialReasonSet, QueryResult, SemanticQueryKey, SemanticQueryValue,
};

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
    pub bytes: u64,
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
    /// The summary's charges against the process retention account, held
    /// for as long as the summary lives: its own bookkeeping, and shared
    /// holds on the carrier and on the allocations its read delivers
    /// (whose caller holds ride that read, so a delivery that outlives the
    /// summary keeps them charged). See [`super::retention::MemoRetention`].
    retention: Option<super::retention::MemoRetention>,
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

    /// The bytes of the summary's own bookkeeping: its block, its table
    /// slot and eviction-order record, and its request prefix's paid lists.
    /// Its read's delivery metadata charges itself; its carrier, self-roots
    /// and delivered allocations are shared allocations, held once through
    /// the store's registry; a wide
    /// carrier's evidence pages are claimed into this reservation, a page
    /// being charged once however many summaries and candidates share it
    /// ([`verter_session_query::facts::receipt::reserve_retained_with_evidence`]).
    fn own_retained_footprint(&self) -> verter_session_query::retention::Footprint {
        use verter_session_query::retention::allocation::{arc_block_bytes, AliasSet};
        let mut aliases = AliasSet::new();
        let bytes = arc_block_bytes(std::mem::size_of::<Self>())
            + verter_session_query::retention::ENTRY_OVERHEAD_BYTES
            + RefusalIndex::candidate_bytes(
                self.index_extras().count(),
                !self.carrier.facts.is_empty(),
            )
            + self
                .prefix
                .request
                .as_ref()
                .map_or(0, |spent| spent.owned_heap_bytes(&mut aliases));
        verter_session_query::retention::Footprint {
            bytes,
            handles: aliases.take_handles(),
        }
    }

    /// The canonicals the summary registers under besides its carrier: its
    /// strict self-roots and its dispatch fence.
    fn index_extras(&self) -> impl Iterator<Item = &str> + '_ {
        self.self_root_canonicals.iter().map(AsRef::as_ref).chain(
            self.read
                .dep_signature
                .iter()
                .map(|crate::semantic_query::DepEntry(canonical, _)| canonical.as_ref()),
        )
    }

    fn shared_parts(&self) -> super::retention::SharedParts<'_> {
        super::retention::SharedParts {
            lease: &self.read.lease,
            facts: &self.carrier.facts,
            self_roots: &self.self_root_canonicals,
            diagnostics: &self.read.walker_diagnostics,
            dep_signature: &self.read.dep_signature,
            result: &self.read.value,
        }
    }
}

type RefusalKey = (PreparedKeyHandle, BudgetProfile);

/// The refusal table's reverse index, keyed by refusal identity.
type RefusalIndex = super::reverse_index::ReceiptReverseIndex<RefusalKey, ()>;

/// The store's bounded refusal-summary table.
#[derive(Default)]
pub(crate) struct RefusalSummaries {
    entries: FxHashMap<RefusalKey, Arc<RefusalSummary>>,
    order: VecDeque<RefusalKey>,
    /// Every kept summary's registration, mutated under the table's lock
    /// in the same step as `entries`.
    index: RefusalIndex,
}

impl RefusalSummaries {
    fn register(&self, key: &RefusalKey, summary: &RefusalSummary) {
        self.index.register(
            key.clone(),
            (),
            &summary.carrier.facts,
            summary.index_extras(),
        );
    }
}

/// The store field: the table, how many summaries it holds (read without
/// its lock by every isolated root while it is empty), and how many times
/// it has been cleared.
#[derive(Default)]
pub(crate) struct RefusalSummaryTable {
    table: Mutex<RefusalSummaries>,
    len: AtomicUsize,
    /// Advanced under the table's lock by every clear: an evaluation that
    /// entered before a clear finishes after it, and its summary — decided
    /// against what the clear dropped — is never inserted.
    clears: AtomicU64,
}

impl RefusalSummaryTable {
    /// Bytes the table's reverse index's own storage is charged.
    pub(crate) fn index_storage_charge_bytes(&self) -> usize {
        self.table.lock().index.storage_charge_bytes()
    }

    /// An empty table whose reverse index is charged to `account`.
    pub(crate) fn charged(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            table: Mutex::new(RefusalSummaries {
                index: RefusalIndex::charged(account),
                ..RefusalSummaries::default()
            }),
            ..Self::default()
        }
    }
}

/// Where an isolated root entered the store: the project generation and
/// the refusal table's clear count it evaluated under. Its refusal is
/// sealed only while both still hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefusalEntryState {
    pub generation: u64,
    pub clears: u64,
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
            self.prepare_rooted_key(key.clone()),
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

    /// Where an isolated root of `ctx`'s request enters the store now.
    pub(crate) fn refusal_entry<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
    ) -> RefusalEntryState {
        RefusalEntryState {
            generation: ctx.request_flags().current_project_generation(),
            clears: self.refusal_summaries.clears.load(Ordering::Acquire),
        }
    }

    /// Seal `summary` as the refusal of the isolated root `key` under
    /// `profile`, entered at `entered`. Refused — nothing kept — for one
    /// the retention account declines to keep,
    /// and, decided under the table's lock in the same step as the
    /// insertion, a cancelled evaluation, one the project moved under, or
    /// one the table was cleared under: a torn or superseded evaluation
    /// never becomes a summary, however its completion interleaves with the
    /// cancellation or the clear. Its facts are revalidated at every
    /// delivery. A re-sealed refusal is the newest.
    pub(crate) fn seal_refusal<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: SemanticQueryKey,
        profile: BudgetProfile,
        entered: RefusalEntryState,
        mut summary: RefusalSummary,
    ) -> bool {
        let refuse = |refusal: verter_session_query::retention::RetentionRefusal| {
            crate::cache_runtime::admission::propagate_non_admission(
                refusal.non_admission_reason(),
            );
            false
        };
        let shared = match self.hold_shared_allocations(summary.shared_parts()) {
            Ok(shared) => shared,
            Err(refusal) => return refuse(refusal),
        };
        let own_footprint = summary.own_retained_footprint();
        match verter_session_query::facts::receipt::reserve_retained_with_evidence(
            self.retention_account(),
            own_footprint.bytes,
            &[&summary.carrier.facts],
        ) {
            RetentionAdmission::Admitted(own) => {
                let own = match own_footprint.handles.retained_with(own, self.charge_ctx()) {
                    Ok(own) => own,
                    Err(refusal) => return refuse(refusal),
                };
                let retention = shared.with_own(own);
                summary.retention = Some(retention);
            }
            RetentionAdmission::Refused(refusal) => return refuse(refusal),
        }
        let key = (self.prepare_rooted_key(key), profile);
        let flags = ctx.request_flags();
        let evicted = {
            let mut summaries = self.refusal_summaries.table.lock();
            if flags.is_cancelled()
                || flags.current_project_generation() != entered.generation
                || self.refusal_summaries.clears.load(Ordering::Acquire) != entered.clears
            {
                drop(summaries);
                // The summary and its reservation are released outside the
                // table's lock.
                drop(summary);
                return false;
            }
            summaries.register(&key, &summary);
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
                    summaries.index.unregister(&oldest);
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

    /// Whether a stored `result` may still be served: it names no retired
    /// signature-kernel epoch — the gate every warm read applies besides its
    /// fact rail. A stored result's graph operands are owned by its holder,
    /// so they cannot have been destroyed.
    pub(crate) fn serves_stored_value(&self, result: &QueryResult<SemanticQueryValue>) -> bool {
        !self.names_retired_kernel_epoch(result)
    }

    /// Drop every sealed refusal whose observed facts reach `canonical_id`,
    /// failed-prefix receipts included, answering how many were dropped.
    /// Validity stays read-side: this is the eager cleanup of an explicit
    /// invalidation, never a delivery decision.
    pub(crate) fn invalidate_refusal_summaries(&self, canonical_id: &str) -> usize {
        let dropped = {
            let mut summaries = self.refusal_summaries.table.lock();
            let reached = summaries.index.reached_by(canonical_id);
            if reached.is_empty() {
                return 0;
            }
            let mut dropped = Vec::with_capacity(reached.len());
            for (key, ()) in &reached {
                summaries.index.unregister(key);
                dropped.extend(summaries.entries.remove(key));
            }
            summaries
                .order
                .retain(|kept| reached.iter().all(|(key, ())| key != kept));
            self.refusal_summaries
                .len
                .store(summaries.entries.len(), Ordering::Release);
            dropped
        };
        let count = dropped.len();
        // Released outside the table's lock.
        drop(dropped);
        count
    }

    /// Drop every sealed refusal.
    pub(crate) fn clear_refusal_summaries(&self) {
        let cleared = {
            let mut summaries = self.refusal_summaries.table.lock();
            summaries.index.clear();
            summaries.order.clear();
            self.refusal_summaries.len.store(0, Ordering::Release);
            self.refusal_summaries.clears.fetch_add(1, Ordering::AcqRel);
            std::mem::take(&mut summaries.entries)
        };
        drop(cleared);
    }

    /// How many sealed refusals the store keeps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_summary_count_for_tests(&self) -> usize {
        self.refusal_summaries.table.lock().entries.len()
    }

    /// What the refusal table's reverse index holds now.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_index_occupancy_for_tests(&self) -> super::ReverseIndexOccupancy {
        self.refusal_summaries.table.lock().index.occupancy()
    }

    /// The validity rail of every sealed refusal the store keeps.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_carriers_for_tests(
        &self,
    ) -> Vec<verter_session_query::facts::fact_cache::ReadSetSignature> {
        self.refusal_summaries
            .table
            .lock()
            .entries
            .values()
            .map(|summary| summary.carrier.clone())
            .collect()
    }

    /// Where an isolated root of `ctx`'s request enters the store now.
    #[cfg(any(test, feature = "test-support"))]
    pub fn refusal_entry_for_tests<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
    ) -> RefusalEntryState {
        self.refusal_entry(ctx)
    }

    /// Seal a request-less refusal of `key` under `profile`, entered at
    /// `entered`, through the production seal.
    #[cfg(any(test, feature = "test-support"))]
    pub fn seal_refusal_for_tests<C: crate::resolver_core::ResolverCapabilities>(
        &self,
        ctx: &dyn crate::resolver_core::ResolverContext<C>,
        key: crate::semantic_query::SemanticQueryKey,
        profile: BudgetProfile,
        entered: RefusalEntryState,
        read: CacheRead<QueryResult<SemanticQueryValue>>,
        carrier: verter_session_query::facts::fact_cache::ReadSetSignature,
    ) -> bool {
        self.seal_refusal(
            ctx,
            key,
            profile,
            entered,
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
