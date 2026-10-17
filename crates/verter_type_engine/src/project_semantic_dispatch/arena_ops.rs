//! Static arena operations selected by the sole query driver. Evaluators can
//! read and construct nodes, but cannot acquire or publish query results.

use super::{NodeScopeId, SemanticGraphStore, SemanticNodeData, SemanticNodeId};

#[derive(Clone, Copy)]
pub(super) struct ArenaOps<'a> {
    graph: &'a SemanticGraphStore,
}
impl<'a> ArenaOps<'a> {
    pub(super) fn new(graph: &'a SemanticGraphStore) -> Self {
        Self { graph }
    }
    pub(super) fn node_data(
        &self,
        id: SemanticNodeId,
    ) -> Option<crate::semantic_query_memo::NodeData> {
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
    /// A rebuilt `origin` under the lexical scope `origin` carries.
    pub(super) fn intern_preserving_scope(
        &self,
        origin: SemanticNodeId,
        data: SemanticNodeData,
    ) -> SemanticNodeId {
        self.graph.intern_preserving_scope(origin, data)
    }
}

impl crate::semantic_query::NodeRead for ArenaOps<'_> {
    fn node_data(&self, node: SemanticNodeId) -> Option<crate::semantic_query_memo::NodeData> {
        self.graph.node_data(node)
    }
}
impl crate::semantic_query::NodeRead for SemanticGraphStore {
    fn node_data(&self, node: SemanticNodeId) -> Option<crate::semantic_query_memo::NodeData> {
        SemanticGraphStore::node_data(self, node)
    }
}
