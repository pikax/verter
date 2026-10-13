//! The sealed source-owned callable header.
//!
//! A callable whose return demand is deferred retains its formal parameters,
//! lexical images, source occurrence and metadata here. Index composition and
//! ordinary signature lowering use the same carrier. Its return is owned by
//! the `return_carrier` source rather than a provisional value slot.
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
    occurrence: Option<SignatureNodeOccurrence>,
    return_carrier: SignatureReturnCarrier,
    signature_span: Option<verter_span::Span>,
    return_type_span: Option<verter_span::Span>,
    predicate: Option<super::SignaturePredicate>,
    is_abstract: bool,
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
    pub occurrence: Option<&'a SignatureNodeOccurrence>,
    /// Where the deferred return comes from.
    pub return_carrier: &'a SignatureReturnCarrier,
    pub signature_span: Option<verter_span::Span>,
    pub return_type_span: Option<verter_span::Span>,
    pub predicate: Option<super::SignaturePredicate>,
    pub is_abstract: bool,
}

impl DeferredCallable {
    pub(crate) fn with_occurrence(mut self, occurrence: SignatureNodeOccurrence) -> Self {
        self.occurrence = Some(occurrence);
        self
    }

    /// Read the composed parts. Requires a sealed consumer witness.
    #[must_use]
    pub fn parts(&self, _consumer: &impl DeferredCallableConsumer) -> DeferredCallableParts<'_> {
        DeferredCallableParts {
            kind: self.kind,
            params: &self.params,
            type_parameters: &self.type_parameters,
            occurrence: self.occurrence.as_ref(),
            return_carrier: &self.return_carrier,
            signature_span: self.signature_span,
            return_type_span: self.return_type_span,
            predicate: self.predicate,
            is_abstract: self.is_abstract,
        }
    }

    /// The parts as identity inputs for the representation-only stable key,
    /// the same fields the derived `Eq` / `Hash` read. Visible to the
    /// `semantic_query` layer only, never to a consumer: a decision that
    /// reads the parts still presents a sealed witness through
    /// [`Self::parts`].
    #[must_use]
    pub(super) fn stable_identity_parts(&self) -> DeferredCallableParts<'_> {
        DeferredCallableParts {
            kind: self.kind,
            params: &self.params,
            type_parameters: &self.type_parameters,
            occurrence: self.occurrence.as_ref(),
            return_carrier: &self.return_carrier,
            signature_span: self.signature_span,
            return_type_span: self.return_type_span,
            predicate: self.predicate,
            is_abstract: self.is_abstract,
        }
    }

    /// Visit every child node the carrier holds (parameter types, binder
    /// declarations and their bounds, a declared return carrier). Topology
    /// only, for structural invariants such as the arena's: no decision
    /// reads the parts through it.
    pub(crate) fn for_each_child_node(&self, mut visit: impl FnMut(SemanticNodeId)) {
        self.params.iter().for_each(|param| visit(param.ty));
        for decl in self.type_parameters.iter() {
            visit(decl.param);
            decl.constraint.into_iter().for_each(&mut visit);
            decl.default.into_iter().for_each(&mut visit);
        }
        if let Some(predicate) = self.predicate {
            predicate.ty.into_iter().for_each(&mut visit);
        }
        if let SignatureReturnCarrier::DeferredFlow(source) = &self.return_carrier {
            for (_, image) in source.bindings() {
                visit(*image);
            }
            source.receiver().into_iter().for_each(&mut visit);
        }
        if let SignatureReturnCarrier::Declared(node) = &self.return_carrier {
            visit(*node);
        }
    }

    /// A carrier from explicit parts, for this crate's unit tests only.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_parts_for_tests(
        kind: SignatureKind,
        params: Arc<[FunctionParam]>,
        type_parameters: Arc<[TypeParamDecl]>,
        occurrence: SignatureNodeOccurrence,
        return_carrier: SignatureReturnCarrier,
    ) -> Self {
        Self {
            kind,
            params,
            type_parameters,
            occurrence: Some(occurrence),
            return_carrier,
            signature_span: None,
            return_type_span: None,
            predicate: None,
            is_abstract: false,
        }
    }

    /// Convert a source-owned deferred header before it enters the graph.
    /// The resulting callable has no return-value slot to read prematurely.
    pub(crate) fn seal_signature_header(header: SemanticNodeData) -> SemanticNodeData {
        let SemanticNodeData::Signature {
            kind,
            params,
            type_parameters,
            occurrence,
            return_carrier: SignatureReturnCarrier::DeferredFlow(source),
            signature_span,
            return_type_span,
            predicate,
            is_abstract,
            ..
        } = &header
        else {
            return header;
        };
        SemanticNodeData::DeferredCallable(Self {
            kind: *kind,
            params: Arc::clone(params),
            type_parameters: Arc::clone(type_parameters),
            occurrence: occurrence.clone(),
            return_carrier: SignatureReturnCarrier::DeferredFlow(source.clone()),
            signature_span: *signature_span,
            return_type_span: *return_type_span,
            predicate: *predicate,
            is_abstract: *is_abstract,
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn return_carrier_for_tests(&self) -> &SignatureReturnCarrier {
        &self.return_carrier
    }

    /// Map lexical images and predicate targets without exposing a return slot.
    pub(crate) fn map_source_values(
        &self,
        mut map: impl FnMut(SemanticNodeId) -> SemanticNodeId,
    ) -> Self {
        let mut mapped = self.clone();
        if let SignatureReturnCarrier::DeferredFlow(source) = &self.return_carrier {
            mapped.return_carrier =
                SignatureReturnCarrier::DeferredFlow(source.map_images(&mut map));
        }
        mapped.predicate = self.predicate.map(|predicate| predicate.map_type(map));
        mapped
    }

    /// Retain the result substitution after explicit instantiation consumed
    /// this header's own clause. Lexical capture remains a separate producer step.
    pub(crate) fn with_instantiated_clause(
        &self,
        bindings: impl IntoIterator<Item = (Arc<str>, SemanticNodeId)>,
    ) -> Self {
        let mut specialized = self.clone();
        if let SignatureReturnCarrier::DeferredFlow(source) = &self.return_carrier {
            specialized.return_carrier = SignatureReturnCarrier::DeferredFlow(
                source.clone().with_instantiated_clause(bindings),
            );
        }
        specialized
    }

    pub(crate) fn with_receiver(&self, receiver: SemanticNodeId) -> Self {
        let mut mapped = self.clone();
        if let SignatureReturnCarrier::DeferredFlow(source) = &self.return_carrier {
            mapped.return_carrier =
                SignatureReturnCarrier::DeferredFlow(source.clone().with_receiver(receiver));
        }
        mapped
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
        self.return_carrier
            .flow_identity()
            .map(|identity| &identity.anchor.canonical_id)
            .or_else(|| {
                self.occurrence
                    .as_ref()
                    .map(|occurrence| &occurrence.function.anchor.canonical_id)
            })
            .expect("a deferred callable retains its source or served occurrence")
    }

    pub(crate) fn without_receiver(&self, ordinary: &[FunctionParam]) -> Self {
        let mut rebuilt = self.clone();
        rebuilt.params = Arc::from(ordinary);
        rebuilt.predicate = None;
        rebuilt
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
            signature_span: self.signature_span,
            return_type_span: self.return_type_span,
            predicate: self.predicate,
            is_abstract: self.is_abstract,
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
            occurrence: self.occurrence,
            return_carrier: match self.return_carrier {
                SignatureReturnCarrier::DeferredFlow(source) => {
                    SignatureReturnCarrier::EvaluatedFlow(source)
                }
                other => other,
            },
            signature_span: self.signature_span,
            return_type_span: self.return_type_span,
            predicate: self.predicate,
            is_abstract: self.is_abstract,
        }
    }
}

/// Formal callable facts shared by evaluated and source-owned headers.
/// Return values are a separate demand and are intentionally absent here.
pub(crate) struct CallableHeaderParts<'a> {
    pub kind: SignatureKind,
    pub params: &'a Arc<[FunctionParam]>,
    pub return_carrier: &'a SignatureReturnCarrier,
    pub type_parameters: &'a Arc<[TypeParamDecl]>,
    pub return_type_span: Option<verter_span::Span>,
}

impl SemanticNodeData {
    pub(crate) fn callable_header(
        &self,
        consumer: &impl DeferredCallableConsumer,
    ) -> Option<CallableHeaderParts<'_>> {
        match self {
            Self::Signature {
                kind,
                params,
                return_carrier,
                type_parameters,
                return_type_span,
                ..
            } => Some(CallableHeaderParts {
                kind: *kind,
                params,
                return_carrier,
                type_parameters,
                return_type_span: *return_type_span,
            }),
            Self::DeferredCallable(callable) => {
                let parts = callable.parts(consumer);
                Some(CallableHeaderParts {
                    kind: parts.kind,
                    params: parts.params,
                    return_carrier: parts.return_carrier,
                    type_parameters: parts.type_parameters,
                    return_type_span: parts.return_type_span,
                })
            }
            _ => None,
        }
    }
}
