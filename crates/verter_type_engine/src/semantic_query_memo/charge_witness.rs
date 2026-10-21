//! Allocation-specific charge observations of a held delivered answer.
//!
//! A [`HeldChargeWitness`] is read from a real held answer — a delivered
//! [`FlowReturnResult`] a public host request returned, or a [`CacheRead`] a
//! dispatch returned — and names, allocation by allocation, what that answer
//! keeps charged. It reads the allocations'
//! own charges through the retention account's observer
//! ([`verter_session_query::retention::witness`]); it estimates nothing,
//! keeps no ledger of its own and mints no root.
//!
//! # What it covers
//!
//! Three disjoint sections, each allocation in exactly one of them — the
//! first, in this order, that reaches it:
//!
//! * **Payload** — the self-charged carriers the answer hands its caller and
//!   every clone of it shares, with the owned children they answer for: a
//!   read's walker diagnostic list, dependency rail (its canonicals) and the
//!   owned names its value holds; a flow return's fresh-literal and
//!   checker-diagnostic slices (a diagnostic's printed arguments) and
//!   independence proofs. Each is charged
//!   by its own cell: pinned by the delivery that minted it, retained while
//!   a cache candidate holds it, pinned again for the readers that outlive
//!   every cache, released by its last handle.
//! * **Cost receipts** — a read's priced cost receipt and, once each, the
//!   receipts of its closure, with the owned names their charges hold. A
//!   receipt is reserved, retained, by the first store that retains a
//!   result it costs and keeps that reservation until its own last holder
//!   drops, so a receipt a reader outlives the cache with stays retained.
//! * **Root metadata** — the delivery owner's own bookkeeping, charged
//!   separately from what it delivers: the owner's block (a caller hold a
//!   retaining cache adopts into retention and hands back pinned) and the
//!   root-owner cells naming the nodes the read roots.
//!
//! Store backing is NOT covered: the rooted nodes' payloads are the arena's
//! storage, charged with the graph through its granular pins, and a
//! signature epoch's records are charged by the signature store. Neither is
//! an allocation the read owns, so neither appears in any section.
//!
//! The closure is the read's delivery stage — the allocations the read
//! holds at the moment of observation, not what a cache candidate keeps
//! beside them (its fact rail, self-roots and key roots), and not the
//! candidate's own owner. A copy of the read shares every allocation, so
//! its witness names the same allocations with the same charges.
//!
//! Reading a witness takes no reservation, adopts no pin, releases nothing
//! and keeps no handle: the account's classes and totals, and every
//! allocation's lifetime, are what they were before the read.

use verter_session_query::retention::owned::OwnedHandles;
use verter_session_query::retention::witness::{ChargeObservation, ChargeObserver};
use verter_session_query::retention::ChargeClass;

use crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt;
use crate::semantic_query::{CacheRead, FlowReturnResult, QueryResult, SemanticNodeId};

/// What a held read keeps charged. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldChargeWitness {
    payload: ChargeObservation,
    cost_receipts: ChargeObservation,
    root_metadata: ChargeObservation,
}

impl HeldChargeWitness {
    /// Observe what `read` keeps charged now.
    #[must_use]
    pub fn of_read(read: &CacheRead<QueryResult<SemanticNodeId>>) -> Self {
        use crate::semantic_query::allocation_footprint::OwnedHeap as _;
        Self::observe(
            |observer| {
                if let Some(diagnostics) = read.walker_diagnostics.owned() {
                    observer.owned(diagnostics);
                }
                if let Some(rail) = read.dep_signature.owned() {
                    observer.owned(rail);
                }
                observer.owned_handles(&OwnedHandles::of(|aliases| {
                    read.value.owned_heap_bytes(aliases)
                }));
            },
            read.receipt.priced(),
            &read.lease,
        )
    }

    /// Observe what a delivered flow return keeps charged now: its owned
    /// slices (fresh literal arms, checker diagnostics, independence
    /// proofs) as payload, no receipt, and its delivery's bookkeeping.
    #[must_use]
    pub fn of_flow_return(result: &FlowReturnResult) -> Self {
        use crate::semantic_query::allocation_footprint::OwnedHeap as _;
        Self::observe(
            |observer| {
                observer.owned_handles(&OwnedHandles::of(|aliases| {
                    result.owned_heap_bytes(aliases)
                }));
            },
            None,
            result.delivery(),
        )
    }

    fn observe(
        payload: impl FnOnce(&mut ChargeObserver),
        receipt: Option<&std::sync::Arc<DemandCostReceipt>>,
        lease: &super::delivery_lease::SemanticDeliveryLease,
    ) -> Self {
        let mut observer = ChargeObserver::new();
        payload(&mut observer);
        let payload = observer.finish();

        let mut observer = ChargeObserver::new();
        observer.exclude(&payload);
        if let Some(receipt) = receipt {
            receipt.observe_retention(&mut observer);
        }
        let cost_receipts = observer.finish();

        let mut observer = ChargeObserver::new();
        observer.exclude(&payload);
        observer.exclude(&cost_receipts);
        lease.observe_metadata(&mut observer);
        Self {
            payload,
            cost_receipts,
            root_metadata: observer.finish(),
        }
    }

    /// The self-charged carriers the read hands its caller.
    #[must_use]
    pub fn payload(&self) -> &ChargeObservation {
        &self.payload
    }

    /// The read's cost receipt closure.
    #[must_use]
    pub fn cost_receipts(&self) -> &ChargeObservation {
        &self.cost_receipts
    }

    /// The read's delivery-owner bookkeeping.
    #[must_use]
    pub fn root_metadata(&self) -> &ChargeObservation {
        &self.root_metadata
    }

    /// The three sections, in the order they are observed.
    #[must_use]
    pub fn sections(&self) -> [&ChargeObservation; 3] {
        [&self.payload, &self.cost_receipts, &self.root_metadata]
    }

    /// Bytes every section's charges of `class` hold together.
    #[must_use]
    pub fn charged_bytes_in(&self, class: ChargeClass) -> usize {
        self.sections()
            .iter()
            .map(|section| section.charged_bytes_in(class))
            .sum()
    }

    /// Whether every observed allocation is charged exactly what it
    /// measures.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.sections().iter().all(|section| section.is_exact())
    }
}
