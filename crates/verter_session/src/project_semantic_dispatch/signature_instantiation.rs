//! The checker's `instantiateSignatureInContextOf` for a relation: a
//! generic source signature relates to a target signature once its type
//! parameters are inferred from the target's parameter types, and, for a
//! type parameter no parameter infers, from the target's return type.

use std::sync::Arc;

use super::dispatch_txn::{InferenceInfoSetup, InferenceSessionSetup, RelationStep};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, InferenceCandidatePriority, InferencePassKind,
    NoInferMask, PrimitiveKind, SemanticNodeData, SemanticNodeId, SignatureKind,
    SignatureReturnCarrier, TypeParamDecl, VariancePhase,
};

impl ProjectSemanticDispatch<'_> {
    /// `source` instantiated in the context of `target` when `source` is a
    /// generic signature `target` does not share its type parameters with:
    /// each type parameter takes the combined candidates the target's
    /// parameter types give the source's parameter types, else those the
    /// target's return gives the source's return, else `unknown`, and a
    /// binding its constraint does not accept is the constraint. `None`
    /// when `source` is no such signature or an inference does not settle.
    pub(super) fn instantiate_signature_in_context_of(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        kind: SignatureKind,
    ) -> Option<SemanticNodeId> {
        let graph = self.graph();
        let (source_return, source_tps) = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            }) if !type_parameters.is_empty() => (*return_type, Arc::clone(type_parameters)),
            _ => return None,
        };
        let target_return = match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            }) if type_parameters
                .iter()
                .map(|tp| tp.param)
                .ne(source_tps.iter().map(|tp| tp.param)) =>
            {
                *return_type
            }
            _ => return None,
        };
        let plan = self
            .signature_comparison_plan(source, target, kind, false)
            .ok()?;
        let pairs: Vec<(SemanticNodeId, SemanticNodeId)> = plan
            .positions
            .iter()
            .map(|&(source_param, target_param)| (target_param, source_param))
            .collect();
        let from_params = self.infer_signature_bindings(&source_tps, &pairs)?;
        let mut bindings: Vec<Option<SemanticNodeId>> = from_params;
        if bindings.iter().any(Option::is_none) {
            let from_return =
                self.infer_signature_bindings(&source_tps, &[(target_return, source_return)])?;
            for (binding, returned) in bindings.iter_mut().zip(from_return) {
                if binding.is_none() {
                    *binding = returned;
                }
            }
        }
        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let mut instantiated = source;
        let mut substituted: Vec<(SemanticNodeId, SemanticNodeId)> = Vec::new();
        for (decl, binding) in source_tps.iter().zip(bindings) {
            let mut bound = binding.unwrap_or(unknown);
            if let Some(constraint) = decl.constraint {
                let constraint = substituted.iter().fold(constraint, |node, (param, arg)| {
                    self.substitute_semantic_type_param(node, *param, *arg)
                });
                match self.execute_relate_pair(bound, constraint) {
                    RelationStep::Assignable { .. } => {}
                    RelationStep::NotAssignable => bound = constraint,
                    _ => return None,
                }
            }
            substituted.push((decl.param, bound));
            instantiated = self.substitute_semantic_type_param(instantiated, decl.param, bound);
        }
        Some(self.signature_without_type_parameters(instantiated))
    }

    /// Each of `type_parameters`' combined candidates from relating every
    /// `(from, into)` pair under one collecting session, `None` for a type
    /// parameter no pair infers; `None` overall when a relation does not
    /// settle.
    fn infer_signature_bindings(
        &self,
        type_parameters: &[TypeParamDecl],
        pairs: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<Vec<Option<SemanticNodeId>>> {
        let infer_params: Arc<[InferenceInfoSetup]> = type_parameters
            .iter()
            .map(|decl| {
                InferenceInfoSetup::for_call(
                    decl.param,
                    Arc::clone(&decl.name),
                    ConstParamPolicy::NonConst,
                    decl.constraint.is_some(),
                )
            })
            .collect();
        let setup = InferenceSessionSetup::new(
            infer_params,
            VariancePhase::Covariant,
            InferencePassKind::CallApplicability,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        );
        let session_id = self
            .dispatch_txn
            .borrow_mut()
            .push_collecting_session(setup, None);
        let mut settled = true;
        for &(from, into) in pairs {
            match self.execute_relate_pair(from, into) {
                RelationStep::Assignable { .. } | RelationStep::NotAssignable => {}
                _ => {
                    settled = false;
                    break;
                }
            }
        }
        let inputs = {
            let txn = self.dispatch_txn.borrow();
            txn.relation
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .and_then(|session| session.fixation_inputs())
        };
        if let Some(session) = self
            .dispatch_txn
            .borrow_mut()
            .relation
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.abandon();
        }
        if !settled {
            return None;
        }
        Some(
            inputs?
                .into_iter()
                .map(|input| {
                    (!input.candidates.is_empty()).then(|| {
                        self.relation_combine_candidates(&input.candidates, input.variance)
                    })
                })
                .collect(),
        )
    }

    /// `signature` declaring no type parameters: an instantiated generic
    /// signature, whose parameters were all substituted.
    fn signature_without_type_parameters(&self, signature: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(signature) else {
            return signature;
        };
        let SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            occurrence,
            return_carrier,
            signature_span,
            return_type_span,
            predicate,
            is_abstract,
            ..
        } = data.as_ref()
        else {
            return signature;
        };
        graph.intern_preserving_scope(
            signature,
            SemanticNodeData::Signature {
                kind: *kind,
                params: Arc::clone(params),
                return_type: *return_type,
                type_parameters: Arc::from(Vec::new().into_boxed_slice()),
                occurrence: occurrence.clone(),
                return_carrier: match return_carrier {
                    SignatureReturnCarrier::Declared(_) => {
                        SignatureReturnCarrier::Declared(*return_type)
                    }
                    other => other.clone(),
                },
                signature_span: *signature_span,
                return_type_span: *return_type_span,
                predicate: *predicate,
                is_abstract: *is_abstract,
            },
        )
    }
}
