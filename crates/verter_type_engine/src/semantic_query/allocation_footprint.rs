//! Allocation footprints of the semantic carriers a cache retains.
//!
//! Composes the declared conventions of
//! [`verter_session_query::retention::allocation`] over the engine's own
//! carriers: dependency rails, walker diagnostics and their errors,
//! materialised points, query values, request prefixes. Every figure is
//! the heap the carrier keeps reachable, by capacity, with each distinct
//! `Arc` allocation counted once per [`AliasSet`] — never a fixed per-item
//! proxy for a variable payload.
//!
//! What a carrier does NOT pay for here, and who does:
//!
//! * graph nodes (`SemanticNodeId` operands) — the node arena owns their
//!   storage; a carrier holds an ordinal and, through its lease, a root;
//! * evidence pages and cost receipts — each carries its own charge for as
//!   long as it lives;
//! * a signature epoch's tables — the epoch's owner;
//! * the inline bytes of a field — the block of the value embedding it.

use std::sync::Arc;

use verter_session_query::retention::allocation::{
    arc_block_bytes, arc_slice_block_bytes, arc_str_bytes, heap_block_bytes, AliasSet,
};

use super::demand::{MaterializedSet, ProjectionPath};
use super::{
    DeclIdentity, IndexKey, PathSegment, PropertyKey, QueryError, QueryResult, SemanticQueryValue,
};
use crate::project_semantic_dispatch::walk::ShallowDiagnostic;

/// Heap a value owns beyond its inline bytes.
pub(crate) trait OwnedHeap {
    /// Heap this value keeps reachable beyond its own inline bytes, each
    /// distinct `Arc` allocation counted once per `aliases`.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize;
}

impl OwnedHeap for Arc<str> {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        arc_str_bytes(self, aliases)
    }
}

impl OwnedHeap for String {
    fn owned_heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
        heap_block_bytes(self.capacity())
    }
}

impl<T: OwnedHeap> OwnedHeap for Arc<[T]> {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Some(block) = arc_slice_block_bytes(self, aliases) else {
            return 0;
        };
        block
            + self
                .iter()
                .map(|item| item.owned_heap_bytes(aliases))
                .sum::<usize>()
    }
}

impl<T: OwnedHeap> OwnedHeap for Option<T> {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.as_ref()
            .map_or(0, |value| value.owned_heap_bytes(aliases))
    }
}

impl<A: OwnedHeap, B: OwnedHeap> OwnedHeap for (A, B) {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.0.owned_heap_bytes(aliases) + self.1.owned_heap_bytes(aliases)
    }
}

/// Values whose every byte is inline.
macro_rules! inline_only {
    ($($t:ty),* $(,)?) => {
        $(impl OwnedHeap for $t {
            fn owned_heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
                0
            }
        })*
    };
}

inline_only!(
    super::SemanticNodeId,
    super::DepVersion,
    u32,
    super::SignatureRef,
    crate::semantic_query::BroadRuntimeKind,
);

impl OwnedHeap for DeclIdentity {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.canonical_id.owned_heap_bytes(aliases) + self.decl_name.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for verter_type_expr::facts::ValueDeclIdentityPart {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.canonical_id.owned_heap_bytes(aliases)
            + self.symbol.owned_heap_bytes(aliases)
            + self.member_path.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for PropertyKey {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            PropertyKey::String(text) => text.owned_heap_bytes(aliases),
            PropertyKey::Number(_) => 0,
            PropertyKey::UniqueSymbol(symbol) => symbol.owned_heap_bytes(aliases),
        }
    }
}

impl OwnedHeap for IndexKey {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            IndexKey::String(text) => text.owned_heap_bytes(aliases),
            IndexKey::Number(_) | IndexKey::Computed(_) => 0,
            IndexKey::UniqueSymbol(symbol) => symbol.owned_heap_bytes(aliases),
        }
    }
}

impl OwnedHeap for PathSegment {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            PathSegment::Member(key) => key.owned_heap_bytes(aliases),
            PathSegment::Index(key) => key.owned_heap_bytes(aliases),
        }
    }
}

impl OwnedHeap for ProjectionPath {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.as_arc().owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for MaterializedSet {
    /// The record slice and every record's path. A record is its demand
    /// point; the point's policy and facets are inline.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Some(block) = arc_slice_block_bytes(&self.0, aliases) else {
            return 0;
        };
        block
            + self
                .0
                .iter()
                .map(|point| point.path().owned_heap_bytes(aliases))
                .sum::<usize>()
    }
}

impl OwnedHeap for QueryError {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            QueryError::UnsupportedIntrinsic { name } | QueryError::Other(name) => {
                name.owned_heap_bytes(aliases)
            }
            QueryError::BudgetExceeded(failure) => failure.context.owned_heap_bytes(aliases),
            QueryError::AliasCycle { chain } => chain.owned_heap_bytes(aliases),
            QueryError::RecursiveRef { name, args } => {
                name.owned_heap_bytes(aliases)
                    + arc_slice_block_bytes(args.operands_arc(), aliases).unwrap_or(0)
            }
            QueryError::DeclPlaceholder {
                canonical_id, name, ..
            } => canonical_id.owned_heap_bytes(aliases) + name.owned_heap_bytes(aliases),
            QueryError::ExecutionAborted(_)
            | QueryError::Miss
            | QueryError::Cancelled
            | QueryError::UnstableState { .. }
            | QueryError::ForeignSemanticOperand
            | QueryError::StaleSemanticOperand
            | QueryError::IncompleteSemanticOperand { .. }
            | QueryError::ValueDomainMismatch { .. }
            | QueryError::RaiseAliasCycle
            | QueryError::TypeParamCycle
            | QueryError::RaiseMiss
            | QueryError::UnrepresentableSurface
            | QueryError::UnrepresentableSurfaceMember
            | QueryError::OpenSurface
            | QueryError::UnmodeledPosition
            | QueryError::PermissiveWildcard
            | QueryError::CheckerRecovery { .. } => 0,
        }
    }
}

impl OwnedHeap for ShallowDiagnostic {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            ShallowDiagnostic::CyclicInstantiation { decl } => decl.owned_heap_bytes(aliases),
            ShallowDiagnostic::InstantiationError { decl, error } => {
                decl.owned_heap_bytes(aliases) + error.0.owned_heap_bytes(aliases)
            }
            ShallowDiagnostic::UnresolvedSurfaceArm {
                name,
                owner_canonical,
                ..
            } => name.owned_heap_bytes(aliases) + owner_canonical.owned_heap_bytes(aliases),
            ShallowDiagnostic::ProjectionWorkLimit { .. }
            | ShallowDiagnostic::ConnectedQueryDepthLimit { .. }
            | ShallowDiagnostic::ConnectedMemoryLimit { .. }
            | ShallowDiagnostic::DuplicateArmShortCircuited { .. }
            | ShallowDiagnostic::CycleShortCircuited { .. }
            | ShallowDiagnostic::OpenConditional { .. }
            | ShallowDiagnostic::PathologicalInput { .. }
            | ShallowDiagnostic::UnionArmEmpty { .. }
            | ShallowDiagnostic::OpenSpreadProgram { .. }
            | ShallowDiagnostic::PendingFlowRoot { .. }
            | ShallowDiagnostic::RelationTooComplex { .. } => 0,
        }
    }
}

impl OwnedHeap for super::InferBinding {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.name.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for SemanticQueryValue {
    /// The heap a value keeps reachable outside the graph. A node-valued
    /// result is an ordinal: the arena owns the node.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            SemanticQueryValue::TypeNode(_)
            | SemanticQueryValue::ProgramAnalysis(_)
            | SemanticQueryValue::MaterializationCycleGate(_)
            | SemanticQueryValue::TruthinessDomain(_)
            | SemanticQueryValue::DiagnosticAnalysis(_) => 0,
            SemanticQueryValue::ObjectProjection(formula) => formula.owned_heap_bytes(aliases),
            SemanticQueryValue::DeclarationAnalysis(value) => {
                value.contributors.owned_heap_bytes(aliases)
            }
            SemanticQueryValue::OverloadSet(signatures) => signatures.owned_heap_bytes(aliases),
            SemanticQueryValue::Relation(payload) => payload.bindings.owned_heap_bytes(aliases),
            SemanticQueryValue::BroadRuntime(classification) => {
                classification.kinds_arc().owned_heap_bytes(aliases)
            }
            SemanticQueryValue::FlowReturn(result) => sized_arc_bytes(result, aliases),
            SemanticQueryValue::ResolveCall(result) => sized_arc_bytes(result, aliases),
            SemanticQueryValue::SignatureSet(value) => value.owned_heap_bytes(aliases),
            // The applied result lives in the signature epoch's tables.
            SemanticQueryValue::SignatureResult(_) => 0,
        }
    }
}

impl<T: OwnedHeap> OwnedHeap for QueryResult<T> {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            QueryResult::Value(value) => value.owned_heap_bytes(aliases),
            QueryResult::Recursive(_) => 0,
            QueryResult::Error(error) => error.owned_heap_bytes(aliases),
        }
    }
}

/// An `Arc<T>`'s block, once per `aliases`.
pub(crate) fn sized_arc_bytes<T>(value: &Arc<T>, aliases: &mut AliasSet) -> usize {
    if aliases.first(value) {
        arc_block_bytes(std::mem::size_of::<T>())
    } else {
        0
    }
}

#[cfg(test)]
#[path = "allocation_footprint_tests.rs"]
mod tests;
