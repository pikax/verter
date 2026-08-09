//! The sealed index-composed callable carrier.
//!
//! An index-composed callable is a same-file served function position
//! composed from the authoritative function-program index: its parameters
//! lower under the target's own binder environment, and its return is
//! owned by the `return_carrier` equation edge rather than being served
//! here (serving it would open a SECOND demand on the same target and
//! deposit it twice into the enclosing component's equation).
//!
//! Deferral is encoded in the TYPE, not in a sentinel: [`DeferredCallable`]
//! has no return-type slot at all, so a deliberately deferred return can
//! never be observed as a failed or missing one. A general
//! [`SemanticNodeData::Signature`] is produced only by
//! [`DeferredCallable::into_general_signature`], which requires the
//! resolved return node.
//!
//! The carrier's parts are private and readable only through a
//! [`DeferredCallableConsumer`] witness. The witness trait is sealed by a
//! private supertrait, so no consumer type can be defined outside this
//! module, and both witness types are crate-internal to mint: the parts
//! are structurally unreadable outside this crate. Inside it, the two
//! witnesses name the query boundaries that own the carrier's lifecycle —
//! the `ResolveOverloadSet` value boundary and the `ResolveCall`
//! applicability executor — and the instantiation / substitution paths
//! that rebuild the carrier present the executor's witness.

use std::sync::Arc;

use super::{
    FunctionParam, SemanticNodeData, SemanticNodeId, SignatureKind, SignatureNodeOccurrence,
    SignatureReturnCarrier, TypeParamDecl,
};

mod sealed {
    /// Private supertrait: only this module can implement it, so the
    /// deferred carrier's consumer set is closed here.
    pub trait Sealed {}
}

/// The closed set of deferred-callable consumers.
pub trait DeferredCallableConsumer: sealed::Sealed {}

/// The `ResolveOverloadSet` value boundary.
#[derive(Debug, Clone, Copy)]
pub struct ResolveOverloadSetConsumer(());

/// The `ResolveCall` applicability executor.
#[derive(Debug, Clone, Copy)]
pub struct ResolveCallConsumer(());

impl sealed::Sealed for ResolveOverloadSetConsumer {}
impl sealed::Sealed for ResolveCallConsumer {}
impl DeferredCallableConsumer for ResolveOverloadSetConsumer {}
impl DeferredCallableConsumer for ResolveCallConsumer {}

impl ResolveOverloadSetConsumer {
    /// The witness the `ResolveOverloadSet` value conversion presents.
    #[must_use]
    pub(crate) fn witness() -> Self {
        Self(())
    }
}

impl ResolveCallConsumer {
    /// The witness the `ResolveCall` applicability executor presents.
    #[must_use]
    pub(crate) fn witness() -> Self {
        Self(())
    }
}

/// One index-composed callable whose body-derived return is deliberately
/// deferred to its `return_carrier`. It carries NO return type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeferredCallable {
    kind: SignatureKind,
    params: Arc<[FunctionParam]>,
    type_parameters: Arc<[TypeParamDecl]>,
    occurrence: SignatureNodeOccurrence,
    return_carrier: SignatureReturnCarrier,
}

/// The composed parts of a [`DeferredCallable`], readable only by a
/// [`DeferredCallableConsumer`].
#[derive(Debug, Clone, Copy)]
pub struct DeferredCallableParts<'a> {
    /// Call or construct bucket.
    pub kind: SignatureKind,
    /// The composed formal parameters, in source order.
    pub params: &'a Arc<[FunctionParam]>,
    /// The position's own type parameters.
    pub type_parameters: &'a Arc<[TypeParamDecl]>,
    /// The env-free occurrence of the served position.
    pub occurrence: &'a SignatureNodeOccurrence,
    /// Where the deferred return comes from.
    pub return_carrier: &'a SignatureReturnCarrier,
}

impl DeferredCallable {
    /// Read the composed parts. Requires a sealed consumer witness.
    #[must_use]
    pub fn parts(&self, _consumer: &impl DeferredCallableConsumer) -> DeferredCallableParts<'_> {
        DeferredCallableParts {
            kind: self.kind,
            params: &self.params,
            type_parameters: &self.type_parameters,
            occurrence: &self.occurrence,
            return_carrier: &self.return_carrier,
        }
    }

    /// The canonical that declares this callable's served position.
    ///
    /// Witness-free on purpose: the seal protects the composed parameters
    /// and the return deferral (so a deliberately deferred return is never
    /// observed as a failed or missing one). A declaring canonical is
    /// neither — [`SemanticNodeData::Signature`] exposes the same
    /// content-free anchor through its public `occurrence` field.
    #[must_use]
    pub(crate) fn declaring_canonical(&self) -> &Arc<str> {
        &self.occurrence.function.anchor.canonical_id
    }

    /// Rebuild the carrier with substituted parameter / type-parameter
    /// nodes. Instantiation preserves the occurrence and the deferral.
    #[must_use]
    pub(crate) fn with_substituted(
        &self,
        params: Arc<[FunctionParam]>,
        type_parameters: Arc<[TypeParamDecl]>,
    ) -> Self {
        Self {
            kind: self.kind,
            params,
            type_parameters,
            occurrence: self.occurrence.clone(),
            return_carrier: self.return_carrier.clone(),
        }
    }

    /// The general signature this callable becomes ONCE its return is
    /// resolved. This is the only way a deferred callable turns into a
    /// [`SemanticNodeData::Signature`].
    #[must_use]
    pub(crate) fn into_general_signature(self, return_type: SemanticNodeId) -> SemanticNodeData {
        SemanticNodeData::Signature {
            kind: self.kind,
            params: self.params,
            return_type,
            type_parameters: self.type_parameters,
            occurrence: Some(self.occurrence),
            return_carrier: self.return_carrier,
            signature_span: None,
            return_type_span: None,
        }
    }
}
