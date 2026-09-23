//! Signature records, substitutions, and epoch-safe interned storage.
//!
//! `ProjectSemanticDispatch::signature_discovery` is the consumer: it discovers,
//! matches and instantiates signature sets through these records. Handles are
//! epoch-qualified; published records are fully initialized before a handle is
//! returned. Warm positional reads borrow through a request-pinned view: no
//! per-candidate `Arc` clone and no intern-shard lock.

#![allow(dead_code)]

mod discovery;
mod lifetime;
mod positional;
mod provenance;
mod read_view;
mod records;
mod result;
mod storage;
mod substitution;

#[allow(unused_imports)]
pub use discovery::{
    append_signatures, heritage_signatures, intersection_signatures, publish_signature,
    set_from_candidates, signatures_identical, union_signatures, BinderInput, DiscoveryError,
    DiscoveryTypes, MatchOptions, ParamInput, RestInput, ResultInput, SignatureInput,
};
#[allow(unused_imports)]
pub use lifetime::{SignatureStore, StoreError, EPOCH_RECORD_CAP};
#[allow(unused_imports)]
pub use positional::{
    MinArityFlags, PositionalMode, PositionalShape, ProjectedElement, ProjectedKind,
    ProjectedTuple, SlotTypeFacts, TypeAt,
};
#[allow(unused_imports)]
pub use provenance::{
    ArmIdentity, ConstituentSequence, DeclarationGroupId, DeclarationParentId, MappedConstituent,
    OriginRelation, OverloadOrder, SignatureProvenance, SourceLocatorId,
};
#[allow(unused_imports)]
pub use read_view::{BorrowedSet, ReadError, SemanticReadView};
#[allow(unused_imports)]
pub use records::{
    AppliedResult, AppliedResultId, BinderDeclaration, BinderSpace, BinderSpaceId, BodyLocatorId,
    CallSubstitutionId, DeclarationInstantiationId, GraphEpoch, ParameterLayout, ParameterLayoutId,
    ParameterOptionality, ParameterSlot, ParameterSlotId, PredicateEffect, RestKind, RestSlot,
    ReturnObligationKey, SignatureCandidate, SignatureDescriptor, SignatureDescriptorId,
    SignatureInputShape, SignatureInputShapeId, SignatureKind, SignatureProvenanceId,
    SignatureResultRecipe, SignatureResultRecipeId, SignatureSemanticFlags, SignatureSetId,
    SignatureSetRef, SignatureTemplate, SignatureTemplateId, SpellingId, TypeToken,
    LAYOUT_QUERY_OUTCOME_SET, LAYOUT_READY_SET, LAYOUT_SIGNATURE_CANDIDATE,
    LAYOUT_SIGNATURE_SET_REF,
};
#[allow(unused_imports)]
pub use result::{
    ReadSignatureResultKey, ResultDemand, SignatureCandidateNodes, SignatureResultValue,
    SignatureSetValue,
};
#[allow(unused_imports)]
pub use substitution::{
    compose_canonical, CallSubstitution, SubstError, SubstTerm, MAX_SUBSTITUTION_CHAIN_DEPTH,
};

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod lifetime_tests;
#[cfg(test)]
mod positional_tests;
#[cfg(test)]
mod provenance_tests;
#[cfg(test)]
mod read_view_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod substitution_tests;
