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
//! A union subject distributes: the conditional evaluates arm by arm and the
//! answers re-form a union (the union's synthesized call signature is a
//! call-site construct, never an inference source), with
//! `OmitThisParameter` following its lib definition over the DISTRIBUTED
//! receiver union (`unknown extends ThisParameterType<T> ? T : …`). A type
//! parameter infers nothing: the conditional stays deferred, and its
//! constraint's signatures are the type parameter's apparent call surface,
//! not its inferred one.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_session_query::type_solver::arena::PrimitiveKind;

use crate::semantic_query::{
    ProjectionReductionContext, QueryError, QueryResult, SemanticContextId, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SemanticQueryValue, SignatureKind,
};

use super::ProjectSemanticDispatch;

/// The closed key of the signature-utility family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignatureUtility {
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

    /// Whether `head` is this utility's spelling. Answers for a resolved
    /// builtin head AND for a still-unresolved authored head: the carrier
    /// normalization that settles an authored head is shadowing-aware, so a
    /// userland declaration of the same name resolves to itself there and
    /// never reaches a builtin rail.
    pub(crate) fn spells(self, head: &str) -> bool {
        Self::from_builtin_name(head) == Some(self)
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
pub(crate) enum AuthoredSignatures {
    /// Every candidate is one authored signature node, in candidate order.
    Nodes(Vec<SemanticNodeId>),
    /// The subject did not settle, or a candidate is a composite with no
    /// single authored signature node.
    Undecided,
}

enum UtilityInference {
    Signature(SemanticNodeId),
    Absent,
    Incomplete,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// The subject's candidates of `kind`, read from `SignaturesOfType`,
    /// each as the authored signature node this subject carries it at — a
    /// composite candidate (a union or mixin synthesis) has representatives,
    /// not an authored node of its own, and reads `None`. The outer `None`
    /// is an incomplete read — never an empty set.
    fn authored_candidates_of(
        &self,
        subject: SemanticNodeId,
        kind: SignatureKind,
    ) -> Option<Vec<Option<SemanticNodeId>>> {
        match self
            .execute_via_cold_build_helper(SemanticQueryKey::SignaturesOfType {
                subject,
                kind,
                context: SemanticContextId::production(),
            })
            .value
        {
            QueryResult::Value(SemanticQueryValue::SignatureSet(value)) => {
                Some(value.nodes.iter().map(|nodes| nodes.authored).collect())
            }
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
    /// candidate of the shared signature list, in the node form every
    /// node-based consumer reads — the authored node of a leaf candidate,
    /// and the composed node of a composite one (a mixin construct
    /// intersection's candidate has no authored node of its own; its
    /// result is the intersection the kernel composed).
    pub fn utility_inference_signature(
        &self,
        subject: SemanticNodeId,
        kind: SignatureKind,
    ) -> Option<SemanticNodeId> {
        match self.utility_inference(subject, kind) {
            UtilityInference::Signature(node) => Some(node),
            UtilityInference::Absent | UtilityInference::Incomplete => None,
        }
    }

    fn utility_inference(&self, subject: SemanticNodeId, kind: SignatureKind) -> UtilityInference {
        if !self.subject_infers_a_signature(subject) {
            return UtilityInference::Incomplete;
        }
        let signature = match self.shared_signature_nodes(subject, kind) {
            super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => match nodes.last() {
                Some(node) => *node,
                None => return UtilityInference::Absent,
            },
            super::signature_discovery::SharedSignatureNodes::Incomplete(_) => {
                return UtilityInference::Incomplete
            }
        };
        match self.graph().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                kind: node_kind, ..
            }) if *node_kind == kind => UtilityInference::Signature(signature),
            Some(SemanticNodeData::DeferredCallable(callable))
                if callable
                    .parts(&crate::semantic_query::ResolveOverloadSetConsumer::witness())
                    .kind
                    == kind =>
            {
                UtilityInference::Signature(signature)
            }
            _ => UtilityInference::Incomplete,
        }
    }

    /// The arms of an already-settled union subject (through alias shells),
    /// `None` for every non-union shape.
    fn settled_union_arms(&self, subject: SemanticNodeId) -> Option<Arc<[SemanticNodeId]>> {
        let mut node = subject;
        let mut visited: FxHashSet<SemanticNodeId> = FxHashSet::default();
        while visited.insert(node) {
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Alias(target)) => node = *target,
                Some(SemanticNodeData::Union(members)) => return Some(members.members_arc()),
                _ => return None,
            }
        }
        None
    }

    /// Evaluate one signature utility over an already-settled `subject`
    /// under the caller's `context`. A union subject distributes (module
    /// docs): every arm settles through the same carrier demand point the
    /// subject did and answers on its own, and the answers re-form a
    /// canonical union. `None` is the utility's unanswerable shell (the
    /// caller publishes the typed miss) — one unanswerable arm leaves the
    /// whole union unanswered, never a partial union.
    pub fn resolve_signature_utility(
        &self,
        utility: SignatureUtility,
        subject: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        let Some(arms) = self.settled_union_arms(subject) else {
            return self.resolve_signature_utility_on_arm(utility, subject, context);
        };
        let mut settled_arms = Vec::with_capacity(arms.len());
        for &arm in arms.iter() {
            if self.cancellation.is_cancelled() {
                return None;
            }
            settled_arms.push(self.resolve_signature_source_carrier(arm, context));
        }
        let mut per_arm = Vec::with_capacity(settled_arms.len());
        match utility.projection() {
            // `OmitThisParameter<T>` is `unknown extends ThisParameterType<T>
            // ? T : (T extends (...args: infer A) => infer R ? (...args: A)
            // => R : T)`: the receiver check runs over the DISTRIBUTED
            // receiver union, so one receiver-less arm (its receiver is
            // `unknown`) returns the subject unchanged; otherwise the second
            // conditional distributes and every arm loses its receiver.
            SignatureUtilityProjection::WithoutReceiver => {
                for &arm in &settled_arms {
                    let receiver = self.resolve_signature_utility(
                        SignatureUtility::ThisParameterType,
                        arm,
                        context,
                    )?;
                    if matches!(
                        self.graph().node_data(receiver).as_deref(),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
                    ) {
                        return Some(subject);
                    }
                    per_arm.push(self.resolve_signature_utility(utility, arm, context)?);
                }
            }
            SignatureUtilityProjection::Result
            | SignatureUtilityProjection::ParameterTuple
            | SignatureUtilityProjection::Receiver => {
                for &arm in &settled_arms {
                    per_arm.push(self.resolve_signature_utility(utility, arm, context)?);
                }
            }
        }
        Some(self.intern_normalized_union_or_intersection(&per_arm, true))
    }

    /// One signature utility over a single settled, non-union arm.
    fn resolve_signature_utility_on_arm(
        &self,
        utility: SignatureUtility,
        subject: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        let inferred = match self.utility_inference(subject, utility.signature_kind()) {
            UtilityInference::Signature(node) => Some(node),
            UtilityInference::Absent => None,
            UtilityInference::Incomplete => return None,
        };
        let unknown = || {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
        };
        match utility.projection() {
            SignatureUtilityProjection::Result => {
                let inferred = inferred?;
                if context.mode == crate::semantic_query::ProjectionMode::Shallow
                    && crate::semantic_query::may_reduce_operator(context)
                {
                    if let Some(surface) =
                        self.signature_return_key_surface(utility, subject, inferred)
                    {
                        return Some(surface);
                    }
                }
                let signature = self.materialize_returned_callable(inferred);
                let (return_type, carrier) = match self.graph().node_data(signature).as_deref() {
                    Some(SemanticNodeData::Signature {
                        return_type,
                        return_carrier,
                        ..
                    }) => (*return_type, return_carrier.clone()),
                    _ => return None,
                };
                // A deferred source has no value slot. An evaluated flow slot
                // has a value, but its enclosing construction's adopted fact
                // does not travel in this utility's node-only argument. Demand
                // that same owned source here so its actual quality is adopted
                // by this result, instead of treating the stored slot as exact.
                let demand_source = matches!(
                    carrier,
                    crate::semantic_query::SignatureReturnCarrier::DeferredFlow(_)
                        | crate::semantic_query::SignatureReturnCarrier::EvaluatedFlow(_)
                ) || matches!(
                    self.graph().node_data(return_type).as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::Miss))
                );
                let return_type = match carrier.result_source() {
                    crate::semantic_query::SignatureResultSource::Function(
                        verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                    ) if demand_source => {
                        let canonical = Arc::clone(&identity.anchor.canonical_id);
                        match self.execute_function_return_source(
                            &verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                            canonical.as_ref(),
                        ) {
                            super::flow_return::FunctionReturnNode::Flow(result) => {
                                // The source consumer has already adopted either the whole
                                // fact or its independently certified selected value.
                                self.apply_signature_return_source(&carrier, result.return_type())
                            }
                            _ => return None,
                        }
                    }
                    _ => return_type,
                };
                // Free signature generics instantiate at their base
                // constraints: `ReturnType<typeof id>` over `id<T>(x: T): T`
                // is `unknown`, over `id<T extends string>(x: T): T` it is
                // `string`.
                let result =
                    self.instantiate_signature_params_at_base_constraints(signature, return_type);
                Some(result)
            }
            SignatureUtilityProjection::ParameterTuple => {
                let signature = inferred?;
                let tuple = self.intern_function_params_tuple(signature)?;
                Some(self.instantiate_signature_params_at_base_constraints(signature, tuple))
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

    /// A shallow result demand can use independently certified keys while
    /// each value remains its own deferred member demand. Key evidence cannot
    /// promote the stored values of an unfinished whole return.
    fn signature_return_key_surface(
        &self,
        utility: SignatureUtility,
        subject: SemanticNodeId,
        signature: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let data = self.graph().node_data(signature)?;
        let header =
            data.callable_header(&crate::semantic_query::ResolveOverloadSetConsumer::witness())?;
        let identity = header.return_carrier.flow_identity()?;
        let key = self.flow_return_key_for(identity);
        let read = self.observe_flow_return_key_domain(key);
        let usable = read.observations_are_complete()
            && matches!(read.value(), crate::semantic_query::FlowReturnStep::Complete(result) if result.exact_return_keys().is_some());
        if !usable {
            let _ = read.discard();
            return None;
        }
        let crate::semantic_query::FlowReturnStep::Complete(result) = read.adopt(self).ok()? else {
            return None;
        };
        let data = self.graph().node_data(result.return_type())?;
        let SemanticNodeData::Object(view) = data.as_ref() else {
            return None;
        };
        let source = self
            .graph()
            .intern_node(SemanticNodeData::InstantiationRef {
                base: crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::from("__builtin__"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: Default::default(),
                    decl_name: Arc::from(utility.builtin_name()),
                },
                args: Arc::from([subject]),
            });
        let entries = view
            .entries
            .iter()
            .map(|entry| match entry {
                crate::semantic_query::SurfaceEntry::Member(member) => {
                    let mut member = member.clone();
                    let key = member.key.cloned_known()?;
                    member.value = self.graph().intern_node(SemanticNodeData::IndexedAccess {
                        object: source,
                        index: crate::semantic_query::IndexKey::String(Arc::from(key.as_string()?)),
                    });
                    Some(crate::semantic_query::SurfaceEntry::Member(member))
                }
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?;
        Some(self.graph().intern_node(SemanticNodeData::Object(
            crate::semantic_query::SurfaceView::from_entries(entries, None, false),
        )))
    }

    fn signature_params(
        &self,
        signature: SemanticNodeId,
    ) -> Option<Arc<[crate::semantic_query::FunctionParam]>> {
        match self.graph().node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature { params, .. }) => Some(Arc::clone(params)),
            Some(SemanticNodeData::DeferredCallable(callable)) => Some(Arc::clone(
                callable
                    .parts(&crate::semantic_query::ResolveOverloadSetConsumer::witness())
                    .params,
            )),
            _ => None,
        }
    }
}
