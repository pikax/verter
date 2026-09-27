//! The checker's `instantiateSignatureInContextOf` for a relation: a
//! generic source signature relates to a target signature once its type
//! parameters are inferred from the target's parameter types, and, for a
//! type parameter no parameter infers, from the target's return type.

use std::sync::Arc;

use super::dispatch_txn::{InferenceInfoSetup, InferenceSessionSetup, RelationStep};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, InferenceCandidatePriority, InferencePassKind,
    NoInferMask, SemanticNodeData, SemanticNodeId, SignatureKind, SignatureReturnCarrier,
    TypeParamDecl, VariancePhase,
};

impl ProjectSemanticDispatch<'_> {
    /// `source` instantiated in the context of `target` when `source` is a
    /// generic signature `target` does not share its type parameters with:
    /// each type parameter fixes, as a call's does, from the candidates the
    /// target's parameter types give the source's parameter types, else
    /// those the target's return gives the source's return, else its
    /// default, else `unknown`, and an inference its constraint does not
    /// accept is the constraint. `None` when `source` is no such signature
    /// or an inference does not settle.
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
        // Candidates from the parameters first; a type parameter they leave
        // uninferred takes the return's (the checker's lower-priority
        // return-type inference).
        let mut inputs = self.signature_inference_inputs(&source_tps, &pairs)?;
        if inputs.iter().any(|input| input.candidates.is_empty()) {
            let returned =
                self.signature_inference_inputs(&source_tps, &[(target_return, source_return)])?;
            for (input, returned) in inputs.iter_mut().zip(returned) {
                if input.candidates.is_empty() {
                    input.candidates = returned.candidates;
                    input.variance = returned.variance;
                }
            }
        }
        // The one inference fixation a call takes: the common supertype of
        // the candidates, widened, else the default, else `unknown`.
        let (fixed, uninferred) = self
            .fix_inference_inputs(inputs, &source_tps, |this, from, to| {
                Ok(settled_relation(this.execute_relate_pair(from, to)))
            })
            .ok()?;
        let substitution = crate::semantic_query::CanonicalTypeSubstitution::new(
            fixed
                .iter()
                .map(|binding| (binding.param, binding.bound))
                .collect(),
        );
        // An inference its constraint refuses is the constraint
        // (`getInferredType`); an uninferred parameter's fixation already
        // settled its default against the constraint.
        let mut clamped = Vec::with_capacity(fixed.len());
        for (position, (decl, binding)) in source_tps.iter().zip(&fixed).enumerate() {
            let mut bound = binding.bound;
            if let Some(constraint) = decl.constraint.filter(|_| !uninferred.contains(&position)) {
                let constraint = self.substitute_canonical(constraint, &substitution);
                if !settled_relation(self.execute_relate_pair(bound, constraint))? {
                    bound = constraint;
                }
            }
            clamped.push((decl.param, bound));
        }
        // The signature sheds its own clause before its positions take the
        // bindings: substituted through the clause, a parameter whose bound
        // names an earlier one would re-intern as another binder, and its
        // occurrences would no longer take their own binding.
        let substitution = crate::semantic_query::CanonicalTypeSubstitution::new(clamped);
        Some(self.substitute_canonical(
            self.signature_without_type_parameters(source),
            &substitution,
        ))
    }

    /// The fixation inputs of `type_parameters` after relating every
    /// `(from, into)` pair under one collecting session; `None` when a
    /// relation does not settle.
    fn signature_inference_inputs(
        &self,
        type_parameters: &[TypeParamDecl],
        pairs: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<Vec<super::dispatch_txn::FixationInput>> {
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
        let settled = pairs
            .iter()
            .all(|&(from, into)| settled_relation(self.execute_relate_pair(from, into)).is_some());
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
        inputs
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

/// A decided relation's verdict; `None` for one that does not settle.
fn settled_relation(step: RelationStep) -> Option<bool> {
    match step {
        RelationStep::Assignable { .. } => Some(true),
        RelationStep::NotAssignable => Some(false),
        _ => None,
    }
}
