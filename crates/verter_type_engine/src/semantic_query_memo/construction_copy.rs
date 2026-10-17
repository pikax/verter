//! Copies of graph carriers made by a semantic builder while it constructs a
//! new payload. A builder outside the engine (a component-meta projector, a
//! framework surface producer) assembles new surfaces from members it read;
//! the copy is taken only inside a construction scope of the graph, which
//! roots every operand the copy names until the scope closes. Outside such
//! a scope there is no copy: a consumer borrows the carrier from the lease
//! that owns it.

use super::node_roots::{NodeRoot, NodeRootScope, NodeRootSet};
use super::SemanticGraphStore;
use crate::project_semantic_dispatch::walk::ShallowDiagnostic;
use crate::semantic_query::{
    authored_property_key_child, IndexSignature, SemanticNodeData, SemanticNodeId, SurfaceEntry,
    SurfaceMember, SurfaceView,
};
use std::sync::Arc;

mod sealed {
    /// Unnameable outside this module, so only the store can request a copy.
    pub struct CopyToken(pub(super) ());
    pub trait Sealed {}
}

/// A graph carrier a builder may copy inside a construction scope.
pub trait ConstructionCopy: sealed::Sealed + Sized {
    /// The graph operands the carrier names, with multiplicity.
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId));
    #[doc(hidden)]
    fn copy_with(&self, token: sealed::CopyToken) -> Self;
}

fn member_operands(member: &SurfaceMember, visit: &mut dyn FnMut(SemanticNodeId)) {
    authored_property_key_child(&member.key)
        .into_iter()
        .for_each(&mut *visit);
    visit(member.value);
}

fn index_operands(signature: &IndexSignature, visit: &mut dyn FnMut(SemanticNodeId)) {
    visit(signature.key_type);
    visit(signature.value_type);
}

impl sealed::Sealed for SurfaceMember {}
impl ConstructionCopy for SurfaceMember {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        member_operands(self, visit);
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for IndexSignature {}
impl ConstructionCopy for IndexSignature {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        index_operands(self, visit);
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for SurfaceEntry {}
impl ConstructionCopy for SurfaceEntry {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        match self {
            Self::Member(member) => member_operands(member, visit),
            Self::CallSignature(node) | Self::ConstructSignature(node) => visit(*node),
            Self::IndexSignature(signature) => index_operands(signature, visit),
        }
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for SurfaceView {}
impl ConstructionCopy for SurfaceView {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        for entry in self.entries() {
            entry.for_each_operand(visit);
        }
        self.keyspace().into_iter().for_each(visit);
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for SemanticNodeData {}
impl ConstructionCopy for SemanticNodeData {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.for_each_retained_child(visit);
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for ShallowDiagnostic {}
impl ConstructionCopy for ShallowDiagnostic {
    fn for_each_operand(&self, visit: &mut dyn FnMut(SemanticNodeId)) {
        self.for_each_retained_node(visit);
    }
    fn copy_with(&self, _: sealed::CopyToken) -> Self {
        self.clone()
    }
}

impl SemanticGraphStore {
    /// Copy a carrier while this thread holds a construction scope of this
    /// graph. The scope roots every operand the copy names, so the copy is
    /// usable for exactly as long as the builder that took it. Without such
    /// a scope there is no copy.
    pub fn copy_in_scope<T: ConstructionCopy>(&self, value: &T) -> Option<T> {
        if !NodeRootScope::holds(&self.arena.core) {
            return None;
        }
        let mut operands = super::node_roots::RootIds::new();
        value.for_each_operand(&mut |node| {
            if node.0 < super::UNALLOCATABLE_ID_FLOOR {
                operands.push(node);
            }
        });
        if let Some(&first) = operands.first() {
            let roots =
                NodeRootSet::acquire(Arc::clone(&self.arena.core), operands.clone()).ok()?;
            NodeRootScope::capture(NodeRoot::from_roots(first, roots));
        }
        Some(value.copy_with(sealed::CopyToken(())))
    }
}
