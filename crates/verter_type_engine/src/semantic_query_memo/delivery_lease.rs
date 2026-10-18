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
    /// This owner's own block and root-owner metadata, charged
    /// separately from the shared allocations it holds for exactly as long
    /// as the owner lives: the owner is its caller-first holder, so it is
    /// pinned while only readers keep it, and a cache retaining the owner
    /// (a memo candidate whose warm reads share it) adopts the same charge
    /// into retention for as long as it does ([`SemanticDeliveryLease::adopt_metadata`]).
    _own: Option<verter_session_query::retention::shared::CallerHold>,
}

impl DeliveryOwner {
    /// Pin this owner's own allocations against `account`: its `Arc` block.
    /// Its two root owners are owned allocations a caller can clone out of
    /// a delivered value, so each charges itself, pinned here for the
    /// owner's readers. The rooted nodes are the arena's; the signature
    /// epoch is its owner's; the allocations the delivery hands out carry
    /// their own charges.
    fn charged(
        mut self,
        account: &std::sync::Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> std::sync::Arc<Self> {
        let footprint = self.own_footprint();
        footprint
            .handles
            .pin(verter_session_query::retention::owned::ChargeCtx::new(
                account,
            ));
        self._own = Some(verter_session_query::retention::shared::CallerHold::pin(
            account,
            footprint.bytes,
        ));
        std::sync::Arc::new(self)
    }

    /// This owner's own block and the root owners' handles.
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
    /// block and the root owners it names, each by its own cell.
    #[cfg(any(test, feature = "test-support"))]
    pub fn own_charge_bytes_for_tests(&self) -> usize {
        self.0.as_ref().map_or(0, |owner| {
            owner._own.as_ref().map_or(
                0,
                verter_session_query::retention::shared::CallerHold::bytes,
            ) + owner.own_footprint().handles.charged_bytes()
        })
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

    /// A cache's holds on the root owners this delivery's metadata names,
    /// retained beside [`Self::adopt_metadata`]'s hold for as long as the
    /// cache keeps the delivery.
    ///
    /// # Errors
    ///
    /// The account's refusal: nothing is held.
    pub(super) fn hold_metadata_handles(
        &self,
        ctx: verter_session_query::retention::owned::ChargeCtx<'_>,
    ) -> Result<
        verter_session_query::retention::owned::HeldHandles,
        verter_session_query::retention::RetentionRefusal,
    > {
        self.metadata_handles().hold(ctx)
    }

    /// The root owners this delivery's metadata names, for a cache that
    /// holds them together with the other allocations it retains.
    pub(super) fn metadata_handles(&self) -> verter_session_query::retention::owned::OwnedHandles {
        self.0
            .as_ref()
            .map(|owner| owner.own_footprint().handles)
            .unwrap_or_default()
    }

    /// A delivery rooting `roots` and holding no shared allocation, its own
    /// metadata pinned against `account`.
    pub(crate) fn from_roots(
        roots: NodeRootSet,
        account: &std::sync::Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self(Some(
            DeliveryOwner {
                roots,
                ..Default::default()
            }
            .charged(account),
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

impl SemanticGraphStore {
    pub(crate) fn lease_delivery(
        &self,
        delivered: DeliveredAllocations<'_>,
    ) -> Result<SemanticDeliveryLease, QueryError> {
        self.mint_delivery(delivered, NodeRootSet::default())
    }

    /// Mint a delivery: root its operands, attach its signature-version
    /// lease, charge the allocations it hands out to its caller (pinned
    /// until a cache retains them), and pin its own metadata.
    fn mint_delivery(
        &self,
        delivered: DeliveredAllocations<'_>,
        key_roots: NodeRootSet,
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
        self.pin_delivered_allocations(&delivered);
        let account = self.retention_account();
        if key_roots.operands().is_empty() {
            let lease = SemanticDeliveryLease(Some(
                DeliveryOwner {
                    roots,
                    signature,
                    key_roots,
                    _own: None,
                }
                .charged(account),
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
            .charged(account),
        ))
        .capture();
        Ok(SemanticDeliveryLease(Some(
            DeliveryOwner {
                roots,
                signature,
                key_roots,
                _own: None,
            }
            .charged(account),
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
        self.mint_delivery(delivered, key_roots)
    }
}
