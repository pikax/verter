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
mod rebind;
mod records;
mod result;
mod storage;
mod substitution;

#[allow(unused_imports)]
pub use discovery::{
    append_signatures, heritage_signatures, intersection_signatures, merged_declaration_signatures,
    publish_signature, resolution_order, set_from_candidates, signatures_identical,
    union_signatures, BinderInput, DiscoveryError, DiscoveryTypes, ForcedResult, MatchOptions,
    ParamInput, RestInput, ResultInput, SignatureInput,
};
#[allow(unused_imports)]
pub use lifetime::{
    EpochBinding, SignatureEpochLease, SignatureStore, StoreError, EPOCH_RECORD_CAP,
};
#[allow(unused_imports)]
pub use positional::{
    tuple_inference_plan, MinArityFlags, PositionalMode, PositionalShape, ProjectedElement,
    ProjectedKind, ProjectedTuple, SlotTypeFacts, TupleElementKind, TupleInferenceStep, TypeAt,
    VariadicArityFacts,
};
#[allow(unused_imports)]
pub use provenance::{
    ArmIdentity, ConstituentSequence, DeclarationGroupId, DeclarationParentId, MappedConstituent,
    OriginRelation, OverloadOrder, SignatureProvenance, SourceLocatorId,
};
#[allow(unused_imports)]
pub use read_view::{BorrowedSet, ReadError, SemanticReadView};
#[allow(unused_imports)]
pub use rebind::EpochRebind;
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
    SignatureSetValue, SignatureVersion,
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
mod rebind_tests;
#[cfg(test)]
mod records_tests;
#[cfg(test)]
mod storage_tests;
#[cfg(test)]
mod substitution_tests;

// A binding is one computation's epoch: moved with the computation that
// holds it, never shared by copy, so two holders never disagree about which
// epoch their records belong to. A rebind pass lives inside one call.
static_assertions::assert_impl_all!(EpochBinding: Send);
static_assertions::assert_not_impl_any!(EpochBinding: Clone, Copy);
static_assertions::assert_not_impl_any!(EpochRebind<'static>: Clone, Copy);
