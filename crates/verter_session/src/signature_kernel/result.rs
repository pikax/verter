//! Demand-driven signature results: the closed projection vocabulary and the
//! full memo identity of one result read.

use crate::semantic_query::{ResultEvaluationContextId, SemanticContextId};

use std::sync::Arc;

use crate::semantic_query::SemanticNodeId;

use super::records::{AppliedResultId, CallSubstitutionId, SignatureDescriptorId, SignatureSetRef};

/// Which half of a signature's result a read demands. A closed vocabulary,
/// not a license for a private body analyzer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultDemand {
    Return,
    Effects,
    Both,
}

impl ResultDemand {
    #[must_use]
    pub const fn reads_return(self) -> bool {
        matches!(self, Self::Return | Self::Both)
    }

    #[must_use]
    pub const fn reads_effects(self) -> bool {
        matches!(self, Self::Effects | Self::Both)
    }
}

/// The complete demand identity of one `ReadSignatureResult`: the
/// descriptor, the frozen call substitution over its residual binders, the
/// projection, the body/return evaluation context, and the semantic
/// context. No content version or edit counter is part of it — freshness is
/// dependency evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadSignatureResultKey {
    pub descriptor: SignatureDescriptorId,
    pub call_substitution: CallSubstitutionId,
    pub projection: ResultDemand,
    pub evaluation: ResultEvaluationContextId,
    pub semantic_context: SemanticContextId,
}

/// Value of a `SignaturesOfType` query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureSetValue {
    pub set: SignatureSetRef,
    /// Per candidate, in set order: the graph nodes THIS read's subject walk
    /// published it from. A descriptor is content-interned, so one
    /// descriptor outlives the file version that first published it; the
    /// node a consumer may read is the one this subject carries, which only
    /// the walk knows.
    pub nodes: Arc<[SignatureCandidateNodes]>,
}

/// The graph nodes one candidate of a [`SignatureSetValue`] was published
/// from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureCandidateNodes {
    /// The authored signature node of a leaf; `None` for a composite (it
    /// has constituents, not a node of its own).
    pub authored: Option<SemanticNodeId>,
    /// For a composite, the authored node of each constituent in
    /// constituent-sequence order; empty for a leaf, and empty when the walk
    /// never published one of them.
    pub constituents: Arc<[SemanticNodeId]>,
}

/// Value of a `ReadSignatureResult` query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureResultValue {
    pub result: AppliedResultId,
}
