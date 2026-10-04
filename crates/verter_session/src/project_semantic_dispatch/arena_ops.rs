//! Static arena operations selected by the sole query driver. Evaluators can
//! read and construct nodes, but cannot acquire or publish query results.

use super::{NodeScopeId, SemanticGraphStore, SemanticNodeData, SemanticNodeId};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) struct ArenaOps<'a> {
    graph: &'a SemanticGraphStore,
}
impl<'a> ArenaOps<'a> {
    pub(super) fn new(graph: &'a SemanticGraphStore) -> Self {
        Self { graph }
    }
    pub(super) fn node_data(&self, id: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.graph.node_data(id)
    }
    pub(super) fn node_reaches_unresolved(&self, node: SemanticNodeId) -> bool {
        self.graph.node_reaches_unresolved(node)
    }

    pub(super) fn intern_node(&self, data: SemanticNodeData) -> SemanticNodeId {
        self.graph.intern_node(data)
    }
    pub(super) fn intern_node_with_scope(
        &self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.graph.intern_node_with_scope(data, scope)
    }
}

impl crate::semantic_query::NodeRead for ArenaOps<'_> {
    fn node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        self.graph.node_data(node)
    }
}
impl crate::semantic_query::NodeRead for SemanticGraphStore {
    fn node_data(&self, node: SemanticNodeId) -> Option<Arc<SemanticNodeData>> {
        SemanticGraphStore::node_data(self, node)
    }
}
