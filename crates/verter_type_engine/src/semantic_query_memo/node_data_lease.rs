//! An internal payload read retains the node's declared graph operands.

use super::node_roots::NodeRoot;
use crate::semantic_query::SemanticNodeData;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct NodeDataLease {
    pub(super) data: Arc<SemanticNodeData>,
    pub(super) root: NodeRoot,
}

impl std::ops::Deref for NodeDataLease {
    type Target = SemanticNodeData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl NodeDataLease {
    pub fn root(&self) -> &NodeRoot {
        &self.root
    }
}
