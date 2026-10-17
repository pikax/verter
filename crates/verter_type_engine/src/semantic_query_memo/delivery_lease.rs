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

#[derive(Clone, Debug, Default)]
struct DeliveryOwner {
    roots: NodeRootSet,
    signature: Option<SignatureEpochLease>,
    key_roots: NodeRootSet,
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
    /// The shared owner every clone of this delivery names, or `None` for
    /// an empty delivery.
    pub(super) fn owner_identity(&self) -> Option<usize> {
        self.0
            .as_ref()
            .map(|owner| std::sync::Arc::as_ptr(owner) as usize)
    }
    pub(crate) fn from_roots(roots: NodeRootSet) -> Self {
        Self(Some(std::sync::Arc::new(DeliveryOwner {
            roots,
            ..Default::default()
        })))
    }
}

impl SemanticGraphStore {
    pub(crate) fn lease_delivery(
        &self,
        result: &QueryResult<SemanticQueryValue>,
        diagnostics: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    ) -> Result<SemanticDeliveryLease, QueryError> {
        let mut nodes = super::node_roots::RootIds::new();
        result.for_each_retained_node(|node| nodes.push(node));
        for diagnostic in diagnostics {
            diagnostic.for_each_retained_node(|node| nodes.push(node));
        }
        nodes.retain(|node| node.0 < super::UNALLOCATABLE_ID_FLOOR);
        let roots = self.root_existing_nodes(nodes)?;
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
        let lease = SemanticDeliveryLease(Some(std::sync::Arc::new(DeliveryOwner {
            roots,
            signature,
            key_roots: Default::default(),
        })));
        lease.capture();
        Ok(lease)
    }

    /// `lease` itself when it already roots, in this graph's arena, every
    /// node `result` and `diagnostics` deliver (a lease the result was
    /// stored or produced with), handed to this thread's construction owner
    /// as a fresh lease would be; otherwise a fresh delivery lease. A value
    /// that carries a signature kernel epoch is always leased afresh, so its
    /// epoch lease is the one this read names.
    pub(crate) fn redeliver(
        &self,
        lease: SemanticDeliveryLease,
        result: &QueryResult<SemanticQueryValue>,
        diagnostics: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    ) -> Result<SemanticDeliveryLease, QueryError> {
        if self.lease_covers(&lease, result, diagnostics) {
            lease.capture();
            return Ok(lease);
        }
        self.lease_delivery(result, diagnostics)
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
        let mut check = |node: crate::semantic_query::SemanticNodeId| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR && !roots.contains(&node) {
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
        result: &QueryResult<SemanticQueryValue>,
        diagnostics: &[crate::project_semantic_dispatch::walk::ShallowDiagnostic],
    ) -> Result<SemanticDeliveryLease, QueryError> {
        let mut lease = self.lease_delivery(result, diagnostics)?;
        let mut nodes = super::node_roots::RootIds::new();
        family.for_each_node_id(|node| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR {
                nodes.push(node);
            }
        });
        std::sync::Arc::make_mut(lease.0.as_mut().unwrap()).key_roots =
            self.root_existing_nodes(nodes)?;
        Ok(lease)
    }
}
