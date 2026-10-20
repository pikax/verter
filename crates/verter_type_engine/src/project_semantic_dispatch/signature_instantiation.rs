//! The checker's `instantiateSignatureInContextOf` for a relation: a
//! generic source signature relates to a target signature once its type
//! parameters are inferred from the target's parameter types, and, for a
//! type parameter no parameter infers, from the target's return type.

use std::sync::Arc;

use super::dispatch_txn::RelationStep;
use super::inference::session::{FixationInput, InferenceInfoSetup, InferenceSessionSetup};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, InferenceCandidatePriority, InferencePassKind,
    NoInferMask, SemanticNodeData, SemanticNodeId, SignatureKind, SignatureReturnCarrier,
    TypeParamDecl, VariancePhase,
};

/// What instantiating a signature in the context of another gives
/// ([`ProjectSemanticDispatch::in_context_instantiation`]).
pub(super) enum InContextInstantiation {
    /// The source is not generic, or shares its type parameters with the
    /// target: it relates as it is.
    NotApplicable,
    /// The source instantiated in the target's context.
    Instantiated(SemanticNodeId),
    /// An inference the instantiation needs does not settle.
    Unsettled,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    /// [`Self::instantiate_signature_in_context_of`], telling a source it
    /// does not apply to from one whose inference does not settle.
    pub(super) fn in_context_instantiation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        kind: SignatureKind,
    ) -> InContextInstantiation {
        let graph = self.graph();
        let type_parameters =
            |signature: SemanticNodeId| match graph.node_data(signature).as_deref() {
                Some(SemanticNodeData::Signature {
                    type_parameters, ..
                }) => Some(
                    type_parameters
                        .iter()
                        .map(|tp| tp.param)
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            };
        let applies = match (type_parameters(source), type_parameters(target)) {
            (Some(source_tps), Some(target_tps)) => {
                !source_tps.is_empty() && source_tps != target_tps
            }
            _ => false,
        };
        if !applies {
            return InContextInstantiation::NotApplicable;
        }
        match self.instantiate_signature_in_context_of(source, target, kind) {
            Some(instantiated) => InContextInstantiation::Instantiated(instantiated),
            None => InContextInstantiation::Unsettled,
        }
    }

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
        let (target_return, target_params) = match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Signature {
                return_type,
                type_parameters,
                ..
            }) if type_parameters
                .iter()
                .map(|tp| tp.param)
                .ne(source_tps.iter().map(|tp| tp.param)) =>
            {
                (
                    *return_type,
                    type_parameters
                        .iter()
                        .map(|tp| tp.param)
                        .collect::<rustc_hash::FxHashSet<_>>(),
                )
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
        // return-type inference). A parameter the target's clause declares
        // too binds to itself below, so it asks the return for nothing.
        let mut inputs = self.signature_inference_inputs(&source_tps, &pairs)?;
        if inputs.iter().zip(source_tps.iter()).any(|(input, decl)| {
            input.candidates.is_empty() && !target_params.contains(&decl.param)
        }) {
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
        let (mut fixed, uninferred) = self
            .fix_inference_inputs(inputs, &source_tps, |this, from, to| {
                Ok(settled_relation(this.execute_relate_pair(from, to)))
            })
            .ok()?;
        // A parameter the target's clause declares too is the target's own
        // (a method parameter whose bounds no instantiation moved): the
        // checker's source names a fresh copy of it, which infers exactly
        // the target's parameter. Its occurrences on both sides are one
        // node, so it binds to itself, never to its default.
        for (decl, binding) in source_tps.iter().zip(fixed.iter_mut()) {
            if target_params.contains(&decl.param) {
                binding.bound = decl.param;
            }
        }
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
    /// relation does not settle.
    fn signature_inference_inputs(
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
                    decl.constraint.is_some(),
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
            .push_collecting_session(setup, None);
        let settled = pairs
            .iter()
            .all(|&(from, into)| settled_relation(self.relate_collecting(from, into)).is_some());
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

    /// The target a generic source signature is instantiated in the context
    /// of and then related to (the checker's `getCanonicalSignature`): each
    /// type parameter without a constraint is its declaration's own binder,
    /// whatever default the instantiation that produced the signature gave
    /// it. Instantiating a generic interface re-interns a method's
    /// parameter whose default names the interface's own (`then<R = T>` of
    /// `Box<T>`), so each level of a recursive relation (`Box<R>`'s `then`
    /// returns `Box<R'>`) would otherwise name a fresh parameter and never
    /// meet the pair it started from. A default has no part in a relation
    /// of the target: only the source's parameters fix. `signature` itself
    /// when no parameter's identity moves.
    pub(super) fn canonical_signature(&self, signature: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let type_parameters = match graph.node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                type_parameters, ..
            }) if !type_parameters.is_empty() => Arc::clone(type_parameters.as_arc()),
            _ => return signature,
        };
        let mut renamed = Vec::new();
        let canonical: Vec<TypeParamDecl> = type_parameters
            .iter()
            .map(|decl| {
                let binder = match graph.node_data(decl.param).as_deref() {
                    Some(SemanticNodeData::TypeParam {
                        decl: declaration,
                        param_index,
                        constraint: None,
                        default: Some(_),
                        display_name,
                    }) if decl.constraint.is_none() => graph.intern_preserving_scope(
                        decl.param,
                        SemanticNodeData::TypeParam {
                            decl: declaration.clone(),
                            param_index: *param_index,
                            constraint: None,
                            default: None,
                            display_name: display_name.clone(),
                        },
                    ),
                    _ => decl.param,
                };
                if binder == decl.param {
                    return decl.copy();
                }
                renamed.push((decl.param, binder));
                TypeParamDecl {
                    name: decl.name.clone(),
                    param: binder,
                    constraint: None,
                    default: None,
                    is_const: decl.is_const,
                }
            })
            .collect();
        if renamed.is_empty() {
            return signature;
        }
        // The clause is shed before the positions take the canonical
        // binders, as an instantiation sheds it, so no binder is rewritten
        // through its own clause.
        let substitution = crate::semantic_query::CanonicalTypeSubstitution::new(graph, renamed);
        let positions = self.substitute_canonical(
            self.signature_without_type_parameters(signature),
            &substitution,
        );
        self.signature_with_type_parameters(positions, canonical)
    }

    /// `signature` declaring no type parameters: an instantiated generic
    /// signature, whose parameters were all substituted.
    fn signature_without_type_parameters(&self, signature: SemanticNodeId) -> SemanticNodeId {
        self.signature_with_type_parameters(signature, Vec::new())
    }

    /// `signature` declaring `type_parameters` in place of its own clause.
    fn signature_with_type_parameters(
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
