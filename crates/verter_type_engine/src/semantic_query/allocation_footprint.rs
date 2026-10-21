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
    arc_slice_block_bytes, arc_str_bytes, heap_block_bytes, AliasSet,
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

impl<T: OwnedHeap> OwnedHeap for Box<[T]> {
    /// An exclusively owned list: its backing and its items' heap.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        heap_block_bytes(std::mem::size_of_val::<[T]>(self))
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

/// Charge an owned allocation of each listed carrier by its [`OwnedHeap`].
macro_rules! measured_by_owned_heap {
    ($($t:ty),* $(,)?) => {
        $(impl verter_session_query::retention::owned::Measured for $t {
            fn heap_bytes(&self, aliases: &mut AliasSet) -> usize {
                OwnedHeap::owned_heap_bytes(self, aliases)
            }
        })*
    };
}

measured_by_owned_heap!(
    super::PathSegment,
    super::SemanticNodeId,
    super::SignatureRef,
    super::BroadRuntimeKind,
    super::InferBinding,
    super::FlowReturnResult,
    super::ResolvedCallResult,
    super::RecursionIdentities,
    super::MaterializationCycleGateFallbackReason,
    super::object_spread_projection::ObjectProjectionAlternative,
    super::CheckerDiagnostic,
    super::CheckerDiagnosticOccurrence,
    crate::project_semantic_dispatch::flow_return::ExactMemberValue,
    crate::project_semantic_dispatch::flow_return::ExactReturnKeyDomain,
);

impl OwnedHeap for super::CheckerDiagnosticOccurrence {
    /// The printed message arguments.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.heap_bytes(aliases)
    }
}

impl OwnedHeap for crate::project_semantic_dispatch::walk::WalkerDiagnostics {
    /// The list's owned allocation; the static empty list is never charged.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.owned().map_or(0, |list| list.offer(aliases))
    }
}

impl OwnedHeap for super::DepSignature {
    /// The rail's owned allocation; the static empty rail is never charged.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.owned().map_or(0, |rail| rail.offer(aliases))
    }
}

impl<T: verter_session_query::retention::owned::Measured> OwnedHeap
    for verter_session_query::retention::owned::Owned<T>
{
    /// A nested owned allocation: charged by its own cell.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

impl<T: verter_session_query::retention::owned::Measured> OwnedHeap
    for verter_session_query::retention::owned::OwnedSlice<T>
{
    /// A nested owned slice: charged by its own cell.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
    }
}

impl OwnedHeap for verter_session_query::retention::owned::OwnedStr {
    /// A nested owned string: charged by its own cell.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.offer(aliases)
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
    super::MaterializationCycleGateFallbackReason,
    super::CheckerDiagnostic,
    super::CheckerDiagnosticRecipe,
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
        self.as_owned().owned_heap_bytes(aliases)
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
            QueryError::BudgetExceeded(failure) => failure.context.offer(aliases),
            QueryError::AliasCycle { chain } => chain.owned_heap_bytes(aliases),
            QueryError::RecursiveRef { name, args } => {
                name.owned_heap_bytes(aliases) + args.operands_owned().offer(aliases)
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
            } => name.offer(aliases) + owner_canonical.offer(aliases),
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
        let Self {
            param: _,
            name,
            bound: _,
        } = self;
        name.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for SemanticQueryValue {
    /// The heap a value keeps reachable outside the graph. A node-valued
    /// result is an ordinal: the arena owns the node.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            SemanticQueryValue::TypeNode(_)
            | SemanticQueryValue::ProgramAnalysis(super::ProgramAnalysisValue { node: _ })
            | SemanticQueryValue::DiagnosticAnalysis(super::DiagnosticAnalysisShell) => 0,
            SemanticQueryValue::TruthinessDomain(domain) => copy_inline(domain),
            SemanticQueryValue::MaterializationCycleGate(outcome) => {
                outcome.owned_heap_bytes(aliases)
            }
            SemanticQueryValue::ObjectProjection(formula) => formula.owned_heap_bytes(aliases),
            SemanticQueryValue::DeclarationAnalysis(super::DeclarationAnalysisValue {
                contributors,
            }) => contributors.owned_heap_bytes(aliases),
            SemanticQueryValue::OverloadSet(signatures) => signatures.owned_heap_bytes(aliases),
            SemanticQueryValue::Relation(payload) => payload.owned_heap_bytes(aliases),
            SemanticQueryValue::BroadRuntime(classification) => {
                classification.owned_heap_bytes(aliases)
            }
            SemanticQueryValue::FlowReturn(result) => result.offer(aliases),
            SemanticQueryValue::ResolveCall(result) => result.offer(aliases),
            SemanticQueryValue::SignatureSet(value) => value.owned_heap_bytes(aliases),
            // The epoch lease and the applied result live in the signature
            // epoch's tables.
            SemanticQueryValue::SignatureResult(
                crate::signature_kernel::SignatureResultValue {
                    version: _,
                    result: _,
                },
            ) => 0,
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

/// A field whose type is `Copy` owns no heap: the bound is the proof, so a
/// field that later gains an allocation stops compiling here.
pub(crate) const fn copy_inline<T: Copy>(_: &T) -> usize {
    0
}

#[cfg(test)]
#[path = "allocation_footprint_tests.rs"]
mod tests;

// ---------------------------------------------------------------------------
// Sealed query-key components.
//
// These keys keep their node sets and substitution frames private; the memo
// family that embeds them measures them here, where the fields are visible.
// Each counts its `Arc` slice block and its root attachment's owner, once
// per `aliases`; the node ids inside are the arena's.
// ---------------------------------------------------------------------------

/// The node-set block and root attachment of a sealed node-set key.
macro_rules! node_set_key_heap_bytes {
    ($($key:ty),* $(,)?) => {
        $(impl $key {
            pub(crate) fn key_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
                arc_slice_block_bytes(&self.0, aliases).unwrap_or(0)
                    + self.1 .0.owned_heap_bytes(aliases)
            }
        })*
    };
}

node_set_key_heap_bytes!(
    super::FlowNarrowingKey,
    super::ContextualTypingKey,
    super::InferableParamSetId,
);

impl super::CanonicalTypeSubstitution {
    /// The pair list — an owned allocation charged by its own cell — and
    /// its root attachment's owner.
    pub(crate) fn key_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        self.0.offer(aliases) + self.1 .0.owned_heap_bytes(aliases)
    }
}
