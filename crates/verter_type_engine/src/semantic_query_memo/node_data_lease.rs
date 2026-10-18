//! An owning read of a graph payload. The payload's root retains the
//! node's declared graph operands; its children are reachable only through
//! borrowed capabilities bound to this lease.

use super::node_roots::{NodeRef, NodeRoot};
use crate::semantic_query::{QueryError, SemanticNodeData, SemanticNodeId};

/// Clones share the immutable payload and its owning root. There is no
/// conversion to an unleased `crate::semantic_query_memo::NodeData` or an owned payload
/// copy: a consumer borrows the payload through `Deref`.
#[derive(Clone, Debug)]
pub struct NodeDataLease {
    pub(super) data: crate::semantic_query_memo::NodeData,
    pub(super) root: NodeRoot,
}

impl std::ops::Deref for NodeDataLease {
    type Target = SemanticNodeData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl AsRef<SemanticNodeData> for NodeDataLease {
    fn as_ref(&self) -> &SemanticNodeData {
        &self.data
    }
}

impl NodeDataLease {
    pub fn root(&self) -> &NodeRoot {
        &self.root
    }

    /// The leased node itself as a read capability.
    pub fn node_ref(&self) -> Result<NodeRef<'_>, QueryError> {
        self.root.node_ref()
    }

    /// Borrow one retained child of this payload. The parent's root keeps
    /// the child's edge alive for the borrow; an ordinal the payload does
    /// not retain is foreign.
    pub fn child(&self, id: SemanticNodeId) -> Result<NodeRef<'_>, QueryError> {
        let mut retained = false;
        self.data
            .for_each_retained_child(|child| retained |= child == id);
        let parent = self.root.node_ref()?;
        if !retained {
            return Err(QueryError::ForeignSemanticOperand);
        }
        Ok(NodeRef::new(id, parent.arena()))
    }

    /// Whether both reads share the same immutable payload allocation.
    #[must_use]
    pub fn ptr_eq(left: &Self, right: &Self) -> bool {
        super::NodeData::ptr_eq(&left.data, &right.data)
    }
}

/// An owning read of an `Object` payload: the one-level surface, borrowed
/// through `Deref`, and the node it was read from. Clones share the read.
#[derive(Clone, Debug)]
pub struct SurfaceLease(NodeDataLease);

impl SurfaceLease {
    /// The surface of an `Object` payload; any other payload has none.
    #[must_use]
    pub fn from_lease(lease: NodeDataLease) -> Option<Self> {
        matches!(&*lease.data, SemanticNodeData::Object(_)).then_some(Self(lease))
    }

    /// The owning read the surface is borrowed from.
    #[must_use]
    pub fn lease(&self) -> &NodeDataLease {
        &self.0
    }

    /// The ordinal of the `Object` node: an identity label, not a capability.
    #[must_use]
    pub fn node(&self) -> SemanticNodeId {
        self.0.root.id()
    }
}

impl std::ops::Deref for SurfaceLease {
    type Target = crate::semantic_query::SurfaceView;
    fn deref(&self) -> &Self::Target {
        match &*self.0.data {
            SemanticNodeData::Object(view) => view,
            _ => unreachable!("a surface lease is minted only over an Object payload"),
        }
    }
}
