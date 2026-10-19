//! Copies of graph carriers made by a semantic builder while it constructs a
//! new payload. A builder outside the engine (a component-meta projector, a
//! framework surface producer) assembles new surfaces from members it read;
//! the copy is taken only inside a construction scope of the graph, which
//! roots every operand the copy names until the scope closes. Outside such
//! a scope there is no copy: a consumer borrows the carrier from the lease
//! that owns it.
//!
//! A copy allocates nothing of its own: every name it carries is an owned
//! handle it shares with the carrier it copies, so it keeps that one charge
//! — never a second, never none — for as long as it lives. Only member
//! records copy; a whole payload or surface, whose lists and text the
//! payload owns, has no copy.

use super::node_roots::{NodeRoot, NodeRootScope, NodeRootSet};
use super::SemanticGraphStore;
use crate::semantic_query::node_payload_footprint::{PayloadHeap, PayloadHeapVisitor};
use crate::semantic_query::{
    authored_property_key_child, IndexSignature, SemanticNodeId, SurfaceEntry, SurfaceMember,
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
    /// Heap the carrier reaches outside its owned handles.
    #[doc(hidden)]
    fn unowned_heap_bytes(&self, token: &sealed::CopyToken) -> usize;
}

/// Heap a copy would allocate or alias outside its owned handles.
struct UnownedHeap(usize);

fn unowned_heap(value: &impl PayloadHeap) -> usize {
    let mut unowned = UnownedHeap(0);
    value.visit_payload_heap(&mut unowned);
    unowned.0
}

impl PayloadHeapVisitor for UnownedHeap {
    fn owned(&mut self, bytes: usize) {
        self.0 += bytes;
    }
    fn shared(&mut self, _key: usize, bytes: usize) -> bool {
        self.0 += bytes;
        true
    }
    fn handle(
        &mut self,
        _offer: &mut dyn FnMut(&mut verter_session_query::retention::allocation::AliasSet) -> usize,
    ) {
    }
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
    fn unowned_heap_bytes(&self, _: &sealed::CopyToken) -> usize {
        unowned_heap(self)
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
    fn unowned_heap_bytes(&self, _: &sealed::CopyToken) -> usize {
        unowned_heap(self)
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
    fn unowned_heap_bytes(&self, _: &sealed::CopyToken) -> usize {
        unowned_heap(self)
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
        let copy = value.copy_with(sealed::CopyToken(()));
        verter_debug_assert::verter_debug_assert_eq!(
            copy.unowned_heap_bytes(&sealed::CopyToken(())),
            0,
            "a construction copy shares only owned handles"
        );
        Some(copy)
    }
}
