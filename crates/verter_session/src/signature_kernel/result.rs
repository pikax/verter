//! Demand-driven signature results: the closed projection vocabulary and the
//! full memo identity of one result read.

use crate::semantic_query::{ResultEvaluationContextId, SemanticContextId};

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureSetValue {
    pub set: SignatureSetRef,
}

/// Value of a `ReadSignatureResult` query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureResultValue {
    pub result: AppliedResultId,
}
