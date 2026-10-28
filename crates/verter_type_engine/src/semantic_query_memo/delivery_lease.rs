//! Internal delivery ownership, shared by cache entries, flights and readers.

use super::{node_roots::NodeRootSet, SemanticGraphStore};
use crate::semantic_query::{QueryError, QueryResult, SemanticQueryValue};
use crate::signature_kernel::SignatureEpochLease;

#[derive(Debug, Default)]
pub struct SemanticDeliveryLease(Option<std::sync::Arc<DeliveryOwner>>);

impl Clone for SemanticDeliveryLease {
    fn clone(&self) -> Self {
        let copy = Self(self.0.clone());
        self.capture();
        copy
    }
}

#[derive(Debug, Default)]
struct DeliveryOwner {
    roots: NodeRootSet,
    signature: Option<SignatureEpochLease>,
    key_roots: NodeRootSet,
    /// This owner's own block, with its root sets inline in it, charged
    /// separately from the shared allocations it holds for exactly as long
    /// as the owner lives. The owner is its delivery's first holder and
    /// the delivery is in-flight work — published to no cache yet — so it
    /// is charged ACTIVE, never refused, until a cache retaining the
    /// owner (a memo candidate whose warm reads share it) adopts the
    /// charge into retention for as long as it does
    /// ([`SemanticDeliveryLease::adopt_metadata`]), or the delivery hands
    /// it to its caller
    /// ([`SemanticDeliveryLease::hand_off_in_flight`]).
    _own: Option<verter_session_query::retention::shared::CallerHold>,
}

impl DeliveryOwner {
    /// Charge this owner's own allocations against `account`: its `Arc`
    /// block, in `phase` — pinned for a delivery its caller keeps, in
    /// flight for one a publication may still adopt. A root set long
    /// enough to need a shared block names an owned allocation a caller can
    /// clone out of a delivered value, so the block charges itself, in the
    /// same phase, for the owner's readers. The rooted nodes are the
    /// arena's; the signature epoch is its
    /// owner's; the allocations the delivery hands out carry their own
    /// charges.
    fn charged(
        mut self,
        account: &std::sync::Arc<verter_session_query::retention::SemanticRetentionAccount>,
        phase: DeliveryPhase,
    ) -> std::sync::Arc<Self> {
        let footprint = self.own_footprint();
        let ctx = verter_session_query::retention::owned::ChargeCtx::new(account);
        let own = match phase {
            DeliveryPhase::Settled => {
                footprint.handles.pin(ctx);
                verter_session_query::retention::shared::CallerHold::pin(account, footprint.bytes)
            }
            DeliveryPhase::InFlight => {
                footprint.handles.charge_in_flight(ctx);
                verter_session_query::retention::shared::CallerHold::in_flight(
                    account,
                    footprint.bytes,
                )
            }
        };
        self._own = Some(own);
        std::sync::Arc::new(self)
    }

    /// This owner's own block and its root sets' shared blocks, as handles.
    fn own_footprint(&self) -> verter_session_query::retention::Footprint {
        use verter_session_query::retention::allocation::{arc_block_bytes, AliasSet};
        let mut aliases = AliasSet::new();
        let bytes = arc_block_bytes(std::mem::size_of::<Self>())
            + self.roots.owned_heap_bytes(&mut aliases)
            + self.key_roots.owned_heap_bytes(&mut aliases);
        verter_session_query::retention::Footprint {
            bytes,
            handles: aliases.take_handles(),
        }
    }
}

impl SemanticDeliveryLease {
    fn capture(&self) {
        if self.0.is_none() {
            return;
        }
        let arena = self.0.as_ref().and_then(|owner| {
            owner
                .roots
                .arena_identity()
                .or_else(|| owner.key_roots.arena_identity())
        });
        super::node_roots::NodeRootScope::capture_delivery(Self(self.0.clone()), arena);
    }
    /// Inspect a delivery's arena after its store is dropped in fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub fn live_nodes_for_tests(&self) -> usize {
        self.0.as_ref().map_or(0, |owner| owner.roots.live_nodes())
    }
    pub fn roots(&self) -> &[crate::semantic_query::SemanticNodeId] {
        self.0.as_ref().map_or(&[], |owner| owner.roots.operands())
    }
    /// Borrow one node this delivery roots.
    pub fn node_ref(
        &self,
        id: crate::semantic_query::SemanticNodeId,
    ) -> Result<super::node_roots::NodeRef<'_>, QueryError> {
        self.0
            .as_ref()
            .ok_or(QueryError::ForeignSemanticOperand)?
            .roots
            .node_ref(id)
    }
    pub fn signature(&self) -> Option<&SignatureEpochLease> {
        self.0.as_ref().and_then(|owner| owner.signature.as_ref())
    }
    /// Another handle on this delivery that no construction owner captures:
    /// the owner taking it keeps it itself.
    pub(super) fn share(&self) -> Self {
        Self(self.0.clone())
    }
    /// The shared owner every clone of this delivery names, or `None` for
    /// an empty delivery.
    pub(super) fn owner_identity(&self) -> Option<usize> {
        self.0
            .as_ref()
            .map(|owner| std::sync::Arc::as_ptr(owner) as usize)
    }
    /// The bytes this delivery's own metadata is charged for: its owner's
    /// block and the root sets' shared blocks it names, each by its own
    /// cell.
    #[cfg(any(test, feature = "test-support"))]
    pub fn own_charge_bytes_for_tests(&self) -> usize {
        self.0.as_ref().map_or(0, |owner| {
            owner._own.as_ref().map_or(
                0,
                verter_session_query::retention::shared::CallerHold::bytes,
            ) + owner.own_footprint().handles.charged_bytes()
        })
    }

    /// Observe this delivery's own metadata charges: its owner's block hold,
    /// against the footprint the owner measures now, and the root sets'
    /// shared blocks it names. The rooted nodes are the arena's storage and the
    /// signature epoch its store's, so neither is observed here.
    #[cfg(any(test, feature = "test-support", feature = "soak-support"))]
    pub(crate) fn observe_metadata(
        &self,
        observer: &mut verter_session_query::retention::witness::ChargeObserver,
    ) {
        let Some(owner) = self.0.as_ref() else {
            return;
        };
        let footprint = owner.own_footprint();
        if let Some(own) = owner._own.as_ref() {
            observer.caller_hold(own, footprint.bytes);
        }
        observer.owned_handles(&footprint.handles);
    }

    /// A cache's hold on this delivery's metadata charge, for a cache that
    /// retains the delivery (its warm reads share the owner): retained while
    /// the cache holds it, handed back to the owner's readers when it drains.
    /// `None` for an empty delivery.
    ///
    /// # Errors
    ///
    /// The account's refusal: the cache must not retain the delivery, and
    /// the metadata stays pinned to its readers.
    pub(super) fn adopt_metadata(
        &self,
    ) -> Result<
        Option<verter_session_query::retention::shared::CacheHold>,
        verter_session_query::retention::RetentionRefusal,
    > {
        self.0
            .as_ref()
            .and_then(|owner| owner._own.as_ref())
            .map(verter_session_query::retention::shared::CallerHold::adopt)
            .transpose()
    }

    /// Settle this delivery's still-in-flight charges at its caller
    /// handoff: the delivery is complete and its callers hold it, so the
    /// owner's own hold and the root sets' shared blocks its metadata names
    /// transfer from ACTIVE to a pin — never released, never charged
    /// twice. A publication that already adopted them keeps them
    /// retained, and a hold that was never in flight keeps its class.
    pub(super) fn hand_off_in_flight(&self) {
        if let Some(owner) = self.0.as_ref() {
            let footprint = owner.own_footprint();
            footprint.handles.hand_off_in_flight();
            if let Some(own) = owner._own.as_ref() {
                own.hand_off_in_flight();
            }
        }
    }

    /// The root sets' shared blocks this delivery's metadata names, for a
    /// cache that
    /// holds them together with the other allocations it retains.
    pub(super) fn metadata_handles(&self) -> verter_session_query::retention::owned::OwnedHandles {
        self.0
            .as_ref()
            .map(|owner| owner.own_footprint().handles)
            .unwrap_or_default()
    }

    /// A delivery rooting `roots` and holding no shared allocation, its own
    /// metadata pinned against `account`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn from_roots(
        roots: NodeRootSet,
        account: &std::sync::Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self(Some(
            DeliveryOwner {
                roots,
                ..Default::default()
            }
            .charged(account, DeliveryPhase::Settled),
        ))
    }
}

/// The shared allocations a delivery hands its caller: its walker
/// diagnostics, its dependency rail and its value's outermost allocation.
pub(crate) struct DeliveredAllocations<'a> {
    pub(crate) diagnostics: Option<&'a crate::project_semantic_dispatch::walk::WalkerDiagnostics>,
    pub(crate) dep_signature: Option<&'a crate::semantic_query::DepSignature>,
    pub(crate) result: &'a QueryResult<SemanticQueryValue>,
}

impl<'a> DeliveredAllocations<'a> {
    /// A delivery of `result` alone.
    pub(crate) fn value(result: &'a QueryResult<SemanticQueryValue>) -> Self {
        Self {
            diagnostics: None,
            dep_signature: None,
            result,
        }
    }

    /// A delivery of `result` with its diagnostics and dependency rail.
    pub(crate) fn read(
        result: &'a QueryResult<SemanticQueryValue>,
        diagnostics: &'a crate::project_semantic_dispatch::walk::WalkerDiagnostics,
        dep_signature: &'a crate::semantic_query::DepSignature,
    ) -> Self {
        Self {
            diagnostics: Some(diagnostics),
            dep_signature: Some(dep_signature),
            result,
        }
    }
}

/// How a fresh delivery's allocations are first charged.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DeliveryPhase {
    /// The delivery hands its value straight to its caller: its
    /// allocations are pinned for that caller, exactly as they always
    /// were.
    Settled,
    /// The delivery names in-flight work a publication may still adopt:
    /// its allocations are charged ACTIVE until the publication or the
    /// caller handoff settles them.
    InFlight,
}

impl SemanticGraphStore {
    pub(crate) fn lease_delivery(
        &self,
        delivered: DeliveredAllocations<'_>,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        self.mint_delivery(delivered, NodeRootSet::default(), DeliveryPhase::Settled)
    }

    /// Mint a delivery of IN-FLIGHT work: a cold producer's settled value,
    /// or a candidate a publication is about to stage. The delivery's
    /// payload is charged ACTIVE — unconditionally, never refused — until
    /// the publication adopts it into retention or the delivery reaches
    /// its caller handoff
    /// ([`SemanticGraphStore::hand_off_delivered_allocations`]).
    pub(crate) fn lease_delivery_in_flight(
        &self,
        delivered: DeliveredAllocations<'_>,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        self.mint_delivery(delivered, NodeRootSet::default(), DeliveryPhase::InFlight)
    }

    /// Mint a delivery: root its operands, attach its signature-version
    /// lease, and charge the allocations it hands out — in flight for a
    /// delivery the producer protocol has not handed off yet, pinned for
    /// one that goes straight to its caller — together with its own
    /// metadata in the same phase.
    fn mint_delivery(
        &self,
        delivered: DeliveredAllocations<'_>,
        key_roots: NodeRootSet,
        phase: DeliveryPhase,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        let result = delivered.result;
        let mut nodes = super::node_roots::RootIds::new();
        result.for_each_retained_node(|node| nodes.push(node));
        for diagnostic in delivered.diagnostics.map_or(&[][..], |d| &d[..]) {
            diagnostic.for_each_retained_node(|node| nodes.push(node));
        }
        nodes.retain(|node| node.0 < super::UNALLOCATABLE_ID_FLOOR);
        let roots = self.root_nodes(nodes)?;
        let attached = match result {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => value.version.0.clone(),
            QueryResult::Value(SemanticQueryValue::SignatureResult(value)) => {
                value.version.0.clone()
            }
            _ => None,
        };
        let signature = if let Some(attached) = attached {
            Some(attached)
        } else {
            super::signature_epoch::value_kernel_epoch(result)
                .map(|epoch| {
                    self.signature_store()
                        .lease_epoch(epoch)
                        .map_err(|_| QueryError::StaleSemanticOperand)
                })
                .transpose()?
        };
        match phase {
            DeliveryPhase::Settled => self.pin_delivered_allocations(&delivered),
            DeliveryPhase::InFlight => self.charge_delivered_in_flight(&delivered),
        }
        let account = self.retention_account();
        if key_roots.operands().is_empty() {
            let lease = SemanticDeliveryLease(Some(
                DeliveryOwner {
                    roots,
                    signature,
                    key_roots,
                    _own: None,
                }
                .charged(account, phase),
            ));
            lease.capture();
            return Ok(lease);
        }
        // A cache candidate's key operands are the cache's to keep, never
        // the reader's: this thread's construction owner receives a
        // delivery of the value alone, and the candidate keeps its own
        // owner rooting both.
        SemanticDeliveryLease(Some(
            DeliveryOwner {
                roots: roots.clone(),
                signature: signature.clone(),
                key_roots: NodeRootSet::default(),
                _own: None,
            }
            .charged(account, phase),
        ))
        .capture();
        Ok(SemanticDeliveryLease(Some(
            DeliveryOwner {
                roots,
                signature,
                key_roots,
                _own: None,
            }
            .charged(account, phase),
        )))
    }

    /// `lease` itself when it already roots, in this graph's arena, every
    /// node `result` and `diagnostics` deliver (a lease the result was
    /// stored or produced with), handed to this thread's construction owner
    /// as a fresh lease would be; otherwise a fresh delivery lease. A value
    /// that carries a signature kernel epoch is always leased afresh, so its
    /// epoch lease is the one this read names.
    ///
    /// A reused lease still charges the allocations the delivery hands its
    /// caller, exactly as a fresh lease would.
    pub(crate) fn redeliver(
        &self,
        lease: SemanticDeliveryLease,
        delivered: DeliveredAllocations<'_>,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        let diagnostics = delivered.diagnostics.map_or(&[][..], |d| &d[..]);
        if self.lease_covers(&lease, delivered.result, diagnostics) {
            self.pin_delivered_allocations(&delivered);
            lease.capture();
            return Ok(lease);
        }
        self.lease_delivery(delivered)
    }

    /// Whether `lease` roots, in this graph's arena, every allocatable node
    /// `result` and `diagnostics` retain, and `result` names no kernel epoch.
    fn lease_covers(
        &self,
        lease: &SemanticDeliveryLease,
        result: &QueryResult<SemanticQueryValue>,
        diagnostics: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    ) -> bool {
        let Some(owner) = lease.0.as_ref() else {
            return false;
        };
        if !owner.roots.belongs_to(&self.arena.core)
            || matches!(
                result,
                QueryResult::Value(
                    SemanticQueryValue::SignatureSet(_) | SemanticQueryValue::SignatureResult(_)
                )
            )
        {
            return false;
        }
        let roots = owner.roots.operands();
        let mut covered = true;
        // A lease delivered from this result roots its nodes in the order the
        // result visits them, so the next unmatched operand is checked first
        // and a covering lease is confirmed in one pass; any other order
        // falls back to a search of the whole set.
        let mut next = 0;
        let mut check = |node: crate::semantic_query::SemanticNodeId| {
            if node.0 >= super::UNALLOCATABLE_ID_FLOOR {
                return;
            }
            if roots.get(next) == Some(&node) {
                next += 1;
            } else if !roots.contains(&node) {
                covered = false;
            }
        };
        result.for_each_retained_node(&mut check);
        for diagnostic in diagnostics {
            diagnostic.for_each_retained_node(&mut check);
        }
        covered
    }

    pub(super) fn lease_candidate(
        &self,
        family: &super::family::FamilyKey,
        delivered: DeliveredAllocations<'_>,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        let mut nodes = super::node_roots::RootIds::new();
        family.for_each_node_id(|node| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR {
                nodes.push(node);
            }
        });
        let key_roots = self.root_nodes(nodes)?;
        self.mint_delivery(delivered, key_roots, DeliveryPhase::InFlight)
    }
}
