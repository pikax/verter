//! The signature utilities: `ReturnType`, `InstanceType`, `Parameters`,
//! `ConstructorParameters`, `ThisParameterType`, `OmitThisParameter`.
//!
//! All six are one keyed read. [`SignatureUtility`] is the key — the builtin
//! spelling is classified once, at [`SignatureUtility::from_builtin_name`] —
//! and the subject's signatures come from the shared `SignaturesOfType`
//! authority, never from a private walk of the subject's shape.
//!
//! Inference mode: a signature utility is a conditional type that infers
//! from a signature position, and conditional inference against a subject
//! carrying several signatures reads the LAST one. That is the one selection
//! rule here ([`ProjectSemanticDispatch::utility_inference_signature`]);
//! argument-driven overload resolution belongs to call resolution.
//!
//! Two subject shapes deliberately infer nothing: a union (the conditional
//! distributes over its arms; the union's synthesized call signature is a
//! call-site construct, not an inference source) and a type parameter (the
//! conditional stays deferred; its constraint's signatures are the type
//! parameter's apparent call surface, not its inferred one).

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_solver::arena::PrimitiveKind;

use crate::semantic_query::{
    QueryResult, SemanticContextId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, SignatureKind,
};

use super::ProjectSemanticDispatch;

/// The closed key of the signature-utility family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum SignatureUtility {
    ReturnType,
    InstanceType,
    Parameters,
    ConstructorParameters,
    ThisParameterType,
    OmitThisParameter,
}

/// What a signature utility reads off its inferred signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SignatureUtilityProjection {
    /// The signature's result.
    Result,
    /// The ordinary (receiver-free) parameters as a tuple.
    ParameterTuple,
    /// The authored `this` receiver.
    Receiver,
    /// The signature republished without its `this` receiver.
    WithoutReceiver,
}

impl SignatureUtility {
    /// Every member of the family, in the builtin registry's order.
    pub(crate) const ALL: [Self; 6] = [
        Self::ReturnType,
        Self::Parameters,
        Self::ConstructorParameters,
        Self::InstanceType,
        Self::ThisParameterType,
        Self::OmitThisParameter,
    ];

    /// The ONE spelling boundary: a builtin utility head that belongs to the
    /// signature family. The head has already been resolved to the builtin
    /// (a userland shadow never reaches here).
    pub(crate) fn from_builtin_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|utility| utility.builtin_name() == name)
    }

    /// The builtin head this key is spelled as.
    pub(crate) const fn builtin_name(self) -> &'static str {
        match self {
            Self::ReturnType => "ReturnType",
            Self::InstanceType => "InstanceType",
            Self::Parameters => "Parameters",
            Self::ConstructorParameters => "ConstructorParameters",
            Self::ThisParameterType => "ThisParameterType",
            Self::OmitThisParameter => "OmitThisParameter",
        }
    }

    /// Which signature list the utility infers from.
    pub(crate) const fn signature_kind(self) -> SignatureKind {
        match self {
            Self::ReturnType
            | Self::Parameters
            | Self::ThisParameterType
            | Self::OmitThisParameter => SignatureKind::Call,
            Self::InstanceType | Self::ConstructorParameters => SignatureKind::Construct,
        }
    }

    /// What the utility reads off the inferred signature.
    pub(crate) const fn projection(self) -> SignatureUtilityProjection {
        match self {
            Self::ReturnType | Self::InstanceType => SignatureUtilityProjection::Result,
            Self::Parameters | Self::ConstructorParameters => {
                SignatureUtilityProjection::ParameterTuple
            }
            Self::ThisParameterType => SignatureUtilityProjection::Receiver,
            Self::OmitThisParameter => SignatureUtilityProjection::WithoutReceiver,
        }
    }
}

/// The ordered authored signatures of one kind a settled subject carries.
pub(super) enum AuthoredSignatures {
    /// Every candidate is one authored signature node, in candidate order.
    Nodes(Vec<SemanticNodeId>),
    /// The subject did not settle, or a candidate is a composite with no
    /// single authored signature node.
    Undecided,
}

impl ProjectSemanticDispatch<'_> {
    /// The subject's candidates of `kind`, read from `SignaturesOfType`,
    /// each as the authored signature node this subject carries it at — a
    /// composite candidate (a union or mixin synthesis) has representatives,
    /// not an authored node of its own, and reads `None`. The outer `None`
    /// is an incomplete read — never an empty set.
    fn authored_candidates_of(
        &self,
        subject: SemanticNodeId,
        kind: SignatureKind,
    ) -> Option<Arc<[Option<SemanticNodeId>]>> {
        match self
            .execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
                subject,
                kind,
                context: SemanticContextId::production(),
            })
            .value
        {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => Some(value.authored),
            _ => None,
        }
    }

    /// The subject's authored signatures of `kind`, in candidate order.
    pub(super) fn authored_signatures_of(
        &self,
        subject: SemanticNodeId,
        kind: SignatureKind,
    ) -> AuthoredSignatures {
        match self.authored_candidates_of(subject, kind) {
            Some(candidates) => candidates
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .map_or(AuthoredSignatures::Undecided, AuthoredSignatures::Nodes),
            None => AuthoredSignatures::Undecided,
        }
    }

    /// Whether conditional inference reads a signature off `subject` at all:
    /// a union distributes and a type parameter defers (module docs).
    fn subject_infers_a_signature(&self, subject: SemanticNodeId) -> bool {
        let mut node = subject;
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        while visited.insert(node) {
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Alias(target)) => node = *target,
                Some(SemanticNodeData::Union(_) | SemanticNodeData::TypeParam { .. }) | None => {
                    return false
                }
                Some(_) => return true,
            }
        }
        false
    }

    /// The signature a utility of `kind` infers from `subject`: the LAST
    /// candidate of the shared signature list.
    pub(super) fn utility_inference_signature(
        &self,
        subject: SemanticNodeId,
        kind: SignatureKind,
    ) -> Option<SemanticNodeId> {
        if !self.subject_infers_a_signature(subject) {
            return None;
        }
        let signature = (*self.authored_candidates_of(subject, kind)?.last()?)?;
        match self.graph().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                kind: node_kind, ..
            }) if *node_kind == kind => Some(signature),
            _ => None,
        }
    }

    /// Evaluate one signature utility over an already-settled `subject`.
    /// `None` is the utility's unanswerable shell (the caller publishes the
    /// typed miss).
    pub(super) fn resolve_signature_utility(
        &self,
        utility: SignatureUtility,
        subject: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let inferred = self.utility_inference_signature(subject, utility.signature_kind());
        let unknown = || {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
        };
        match utility.projection() {
            SignatureUtilityProjection::Result => {
                let signature = inferred?;
                let return_type = match self.graph().node_data(signature).as_deref() {
                    Some(SemanticNodeData::Signature { return_type, .. }) => *return_type,
                    _ => return None,
                };
                // Free signature generics instantiate at `unknown`:
                // `ReturnType<typeof id>` over `id<T>(x: T): T` is `unknown`.
                Some(self.instantiate_free_signature_params_at_unknown(signature, return_type))
            }
            SignatureUtilityProjection::ParameterTuple => {
                let signature = inferred?;
                let tuple = self.intern_function_params_tuple(signature)?;
                Some(self.instantiate_free_signature_params_at_unknown(signature, tuple))
            }
            // A signature with no authored `this` has none, which is
            // `unknown`; so does a subject with no signature to infer from.
            SignatureUtilityProjection::Receiver => Some(match inferred {
                Some(signature) => self
                    .signature_params(signature)
                    .and_then(|params| {
                        crate::semantic_query::split_this_receiver(&params)
                            .0
                            .map(|receiver| receiver.ty)
                    })
                    .unwrap_or_else(unknown),
                None => unknown(),
            }),
            // The identity on a subject with no receiver to remove.
            SignatureUtilityProjection::WithoutReceiver => Some(match inferred {
                Some(signature) => match self.signature_params(signature) {
                    Some(params) => match crate::semantic_query::split_this_receiver(&params) {
                        (Some(_), ordinary) => {
                            self.intern_signature_without_receiver(signature, ordinary)
                        }
                        (None, _) => subject,
                    },
                    None => subject,
                },
                None => subject,
            }),
        }
    }

    fn signature_params(
        &self,
        signature: SemanticNodeId,
    ) -> Option<Arc<[crate::semantic_query::FunctionParam]>> {
        match self.graph().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature { params, .. }) => Some(Arc::clone(params)),
            _ => None,
        }
    }
}
