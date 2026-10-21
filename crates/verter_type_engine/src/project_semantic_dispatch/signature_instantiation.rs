//! The checker's `instantiateSignatureInContextOf` for a relation: a
//! generic source signature's type parameters are inferred from the target's
//! parameter types, and, for a type parameter no parameter infers, from the
//! target's return type.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::conditional_decision::{exact_reading, ConditionalReading};
use super::dispatch_txn::RelationStep;
use super::inference::session::{FixationInput, InferenceInfoSetup, InferenceSessionSetup};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, InferenceCandidatePriority, InferencePassKind,
    NoInferMask, SemanticNodeData, SemanticNodeId, SignatureKind, SignatureReturnCarrier,
    TypeParamDecl, VariancePhase,
};

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
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
            }) if !type_parameters.is_empty() => {
                (*return_type, Arc::clone(type_parameters.as_arc()))
            }
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
            self.graph(),
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
        let substitution =
            crate::semantic_query::CanonicalTypeSubstitution::new(self.graph(), clamped);
        Some(self.substitute_canonical(
            self.signature_without_type_parameters(source),
            &substitution,
        ))
    }

    /// The fixation inputs of `type_parameters` after relating every
    /// `(from, into)` pair under one collecting session; `None` when a
    /// relation stops at its budget or a conditional reading does not
    /// settle.
    pub(super) fn signature_inference_inputs(
        &self,
        type_parameters: &[TypeParamDecl],
        pairs: &[(SemanticNodeId, SemanticNodeId)],
    ) -> Option<Vec<FixationInput>> {
        let infer_params: Arc<[InferenceInfoSetup]> = type_parameters
            .iter()
            .map(|decl| {
                InferenceInfoSetup::for_call(
                    decl.param,
                    Arc::<str>::from(&decl.name),
                    ConstParamPolicy::NonConst,
                    decl.constraint,
                )
            })
            .collect();
        let setup = InferenceSessionSetup::new(
            self.graph(),
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
            .push_collecting_session(setup);
        // Inference between conditional types visits corresponding operands
        // independently (`inferFromTypes`). A failed extends comparison must
        // not roll back the check position's candidate. The conditional
        // authority supplies branches with their pending substitutions applied.
        let settled = (|| {
            let mut pending: Vec<_> = pairs.iter().rev().copied().collect();
            let mut seen = FxHashSet::default();
            while let Some((from, into)) = pending.pop() {
                if !seen.insert((from, into)) {
                    continue;
                }
                match (
                    exact_reading(self.conditional_outcome(from))?,
                    exact_reading(self.conditional_outcome(into))?,
                ) {
                    (ConditionalReading::Reduced(from), _) => pending.push((from, into)),
                    (_, ConditionalReading::Reduced(into)) => pending.push((from, into)),
                    (ConditionalReading::Deferred(from), ConditionalReading::Deferred(into)) => {
                        pending.extend([
                            (from.false_branch, into.false_branch),
                            (from.true_branch, into.true_branch),
                            (from.extends, into.extends),
                            (from.check, into.check),
                        ]);
                    }
                    // Inference reads what the pair deposits, whatever its
                    // verdict (`inferTypes` never fails): a pair the session
                    // cannot decide — an outer `infer` or call parameter in
                    // the target's positions — still infers the source's own
                    // parameters. Only a budget stop leaves the inference
                    // incomplete.
                    _ => {
                        if let RelationStep::BudgetExceeded(_) = self.relate_collecting(from, into)
                        {
                            return None;
                        }
                    }
                }
            }
            Some(())
        })()
        .is_some();
        let inputs = {
            let txn = self.dispatch_txn.borrow();
            txn.relation
                .session(session_id)
                .and_then(|session| session.fixation_inputs())
        };
        if let Some(session) = self
            .dispatch_txn
            .borrow_mut()
            .relation
            .session_mut(session_id)
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
    pub(super) fn signature_without_type_parameters(
        &self,
        signature: SemanticNodeId,
    ) -> SemanticNodeId {
        self.signature_with_type_parameters(signature, Vec::new())
    }

    /// `signature` declaring `type_parameters` in place of its own.
    pub(super) fn signature_with_type_parameters(
        &self,
        signature: SemanticNodeId,
        type_parameters: Vec<TypeParamDecl>,
    ) -> SemanticNodeId {
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
                params: params.share(),
                return_type: *return_type,
                type_parameters: crate::semantic_query::PayloadArray::from(
                    type_parameters.into_boxed_slice(),
                ),
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
