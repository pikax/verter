//! Typed ownership topology of results, refusals and diagnostics.

use super::*;

impl QueryError {
    pub(crate) fn for_each_retained_node(&self, visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::RecursiveRef { name: _, args } => args.nodes().iter().copied().for_each(visit),
            Self::CheckerRecovery {
                diagnostic: _,
                basis: _,
                origin,
            } => origin.iter().copied().for_each(visit),
            Self::ExecutionAborted(_)
            | Self::Miss
            | Self::UnsupportedIntrinsic { name: _ }
            | Self::BudgetExceeded(_)
            | Self::Cancelled
            | Self::UnstableState { attempts: _ }
            | Self::ForeignSemanticOperand
            | Self::StaleSemanticOperand
            | Self::IncompleteSemanticOperand { reasons: _ }
            | Self::AliasCycle { chain: _ }
            | Self::Other(_)
            | Self::DeclPlaceholder {
                canonical_id: _,
                owner: _,
                name: _,
                whole_hash: _,
            }
            | Self::ValueDomainMismatch {
                expected: _,
                actual: _,
            }
            | Self::RaiseAliasCycle
            | Self::TypeParamCycle
            | Self::RaiseMiss
            | Self::UnrepresentableSurface
            | Self::UnrepresentableSurfaceMember
            | Self::OpenSurface
            | Self::UnmodeledPosition
            | Self::PermissiveWildcard => {}
        }
    }
}

impl SemanticQueryValue {
    pub(crate) fn for_each_retained_node(&self, mut visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::TypeNode(node) => visit(*node),
            Self::ObjectProjection(formula) => formula.for_each_retained_node(visit),
            Self::ProgramAnalysis(value) => visit(value.node),
            Self::DeclarationAnalysis(value) => value.contributors.iter().copied().for_each(visit),
            Self::OverloadSet(signatures) => {
                for signature in signatures.iter() {
                    visit(signature.node);
                    match &signature.return_carrier {
                        SignatureReturnCarrier::Declared(node) => visit(*node),
                        SignatureReturnCarrier::DeferredFlow(source)
                        | SignatureReturnCarrier::EvaluatedFlow(source) => {
                            for (_, image) in source.bindings() {
                                visit(*image);
                            }
                            source.receiver().into_iter().for_each(&mut visit);
                        }
                        SignatureReturnCarrier::Function(_) => {}
                    }
                }
            }
            Self::Relation(value) => {
                for binding in value.bindings.iter() {
                    visit(binding.param);
                    visit(binding.bound);
                }
            }
            Self::FlowReturn(value) => {
                visit(value.return_type());
                value
                    .fresh_literal_arms()
                    .iter()
                    .copied()
                    .for_each(&mut visit);
                if let Some(wrap) = value.function_wrap() {
                    wrap.yield_join.into_iter().for_each(&mut visit);
                    wrap.generator_head.into_iter().for_each(&mut visit);
                }
                value
                    .inferred_predicate()
                    .and_then(|predicate| predicate.ty)
                    .into_iter()
                    .for_each(visit);
            }
            Self::ResolveCall(value) => match value.as_ref() {
                ResolvedCallResult::Selected {
                    selected: _,
                    selected_signature,
                    substitution,
                    return_type,
                    fresh_literal_returns,
                    recovery_diagnostic: _,
                } => {
                    visit(*selected_signature);
                    for &(parameter, argument) in substitution.bindings() {
                        visit(parameter);
                        visit(argument);
                    }
                    visit(*return_type);
                    fresh_literal_returns.iter().copied().for_each(visit);
                }
                ResolvedCallResult::DynamicAny { return_type } => visit(*return_type),
            },
            Self::SignatureSet(value) => {
                for candidate in value.nodes.iter() {
                    candidate.authored.into_iter().for_each(&mut visit);
                    candidate.constituents.iter().copied().for_each(&mut visit);
                }
            }
            // Signature records retain graph tokens in their epoch; the
            // result delivery separately leases that epoch.
            Self::SignatureResult(_)
            | Self::BroadRuntime(_)
            | Self::MaterializationCycleGate(_)
            | Self::TruthinessDomain(_)
            | Self::DiagnosticAnalysis(_) => {}
        }
    }
}

impl QueryResult<SemanticQueryValue> {
    pub(crate) fn for_each_retained_node(&self, visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::Value(value) => value.for_each_retained_node(visit),
            Self::Recursive(node) => {
                let mut visit = visit;
                visit(*node);
            }
            Self::Error(error) => error.for_each_retained_node(visit),
        }
    }
}

impl crate::project_semantic_dispatch::walk::ShallowDiagnostic {
    pub(crate) fn for_each_retained_node(&self, mut visit: impl FnMut(SemanticNodeId)) {
        match self {
            Self::ProjectionWorkLimit { root }
            | Self::ConnectedQueryDepthLimit { root }
            | Self::ConnectedMemoryLimit { root }
            | Self::PathologicalInput { root } => visit(*root),
            Self::DuplicateArmShortCircuited { node }
            | Self::CycleShortCircuited { node }
            | Self::OpenConditional { node }
            | Self::OpenSpreadProgram { node } => visit(*node),
            Self::UnionArmEmpty {
                union_node,
                arm_index: _,
            } => visit(*union_node),
            Self::RelationTooComplex { source, target } => {
                visit(*source);
                visit(*target);
            }
            Self::InstantiationError { decl: _, error } => error.0.for_each_retained_node(visit),
            Self::CyclicInstantiation { decl: _ }
            | Self::UnresolvedSurfaceArm {
                name: _,
                owner_canonical: _,
                owner: _,
            }
            | Self::PendingFlowRoot { gap: _ } => {}
        }
    }
}
