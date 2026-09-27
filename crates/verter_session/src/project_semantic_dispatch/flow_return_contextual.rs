//! A context-sensitive function argument typed under the contextual
//! signature its call hands it — the checker's second inference pass: the
//! arguments before it inferred the callee's type parameters, the
//! contextual type instantiated with those inferences types the function's
//! unannotated parameters, and its body return is read under them.

use super::{FlowEvaluator, Positional};
use crate::flow_slice_content::SliceExpr;
use crate::semantic_query::{SemanticNodeData, SemanticNodeId, SignatureKind};

/// A function-value argument to type under a contextual signature: the
/// nested function's demand, and what its outcome is read against.
pub(super) struct FunctionArgumentRequest {
    pub(super) demand: super::nested::NestedDemand,
    pub(super) mark: FunctionArgumentMark,
}

/// The frame's holds and degradation before a function-value argument
/// evaluates.
pub(super) struct FunctionArgumentMark {
    holds_before: usize,
    degradation_before: Option<crate::semantic_query::FlowReturnDegradation>,
}

/// The contextual signature a function value is checked under: the type at
/// each parameter position and the contextual return type.
#[derive(Clone)]
pub(super) struct ContextualSignature {
    /// The contextual signature's non-rest parameter types, in order.
    parameters: Vec<SemanticNodeId>,
    /// The element type of its rest parameter, when it has one.
    rest_element: Option<SemanticNodeId>,
    /// The contextual return type.
    pub(super) return_type: SemanticNodeId,
}

impl ContextualSignature {
    /// The contextual type at parameter `position`
    /// (`getTypeAtPosition`): the parameter there, else the rest element;
    /// `None` past the end of a signature without a rest parameter.
    pub(super) fn parameter_at(&self, position: usize) -> Option<SemanticNodeId> {
        self.parameters.get(position).copied().or(self.rest_element)
    }
}

impl FlowEvaluator<'_, '_> {
    /// The contextual signature of `contextual`: its one call signature,
    /// read as the checker reads a contextual signature. `None` when it has
    /// none, several, or a generic one.
    pub(super) fn contextual_signature(
        &self,
        contextual: SemanticNodeId,
    ) -> Option<ContextualSignature> {
        let signatures = match self
            .dispatch
            .shared_signature_nodes(contextual, SignatureKind::Call)
        {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(signatures) => {
                signatures
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => return None,
        };
        let [signature] = signatures.as_slice() else {
            return None;
        };
        let graph = self.dispatch.graph();
        let data = graph.node_data(*signature)?;
        let SemanticNodeData::Signature {
            params,
            return_type,
            type_parameters,
            ..
        } = &*data
        else {
            return None;
        };
        if !type_parameters.is_empty() {
            return None;
        }
        let mut parameters = Vec::with_capacity(params.len());
        let mut rest_element = None;
        for param in params.iter() {
            if param.rest {
                rest_element = match graph.node_data(param.ty).as_deref() {
                    Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                    _ => return None,
                };
                break;
            }
            parameters.push(param.ty);
        }
        Some(ContextualSignature {
            parameters,
            rest_element,
            return_type: *return_type,
        })
    }

    /// A function-value argument typed under `contextual`, the type its
    /// parameter position takes with the inferences before it applied.
    /// `None` when the argument is no nested function value, the contextual
    /// type has no one non-generic call signature, or the function's
    /// evaluation does not finish here — the call then stays undecided.
    pub(super) fn eval_function_argument_in_context(
        &mut self,
        expr: &SliceExpr,
        contextual: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        if !matches!(expr, SliceExpr::NestedFunctionValue { gap: None, .. }) {
            return None;
        }
        let signature = self.contextual_signature(contextual)?;
        let request = self.function_argument_request(expr, Some(signature))?;
        let node = self.eval_nested_demand(request.demand);
        self.function_argument_value(request.mark, Positional::Value(node))
    }

    /// The request to type a function-value argument that is not context
    /// sensitive with the call's first pass (`checkExpressionWithContextualType`): its
    /// parameters are its own, and its body return widens a lone fresh
    /// literal unless the parameter's contextual return type is a literal
    /// context for it — the type the callee's one call signature declares
    /// at position `ordinal`, its type parameters unfixed (`run(() => 1)`
    /// over `run<R>(cb: () => R): R` passes `() => number`, over `R extends
    /// number` `() => 1`). A callee with several signatures gives no
    /// contextual return, so the literal widens. `None` when the argument is
    /// no nested function value; its evaluation is read by
    /// [`Self::function_argument_value`].
    pub(super) fn function_argument_request_under_parameter(
        &mut self,
        expr: &SliceExpr,
        callee: SemanticNodeId,
        ordinal: usize,
    ) -> Option<FunctionArgumentRequest> {
        if !matches!(expr, SliceExpr::NestedFunctionValue { gap: None, .. }) {
            return None;
        }
        let signature = self
            .callee_parameter_type(callee, ordinal)
            .and_then(|parameter| self.contextual_signature(parameter));
        self.function_argument_request(expr, signature)
    }

    /// The request to type the function-value argument `expr` (no flow
    /// gap) under `signature`: its demand, and the frame's holds and
    /// degradation before it evaluates.
    pub(super) fn function_argument_request(
        &mut self,
        expr: &SliceExpr,
        signature: Option<ContextualSignature>,
    ) -> Option<FunctionArgumentRequest> {
        let (demand, gap) = super::nested::NestedDemand::of(expr, signature)?;
        if gap.is_some() {
            return None;
        }
        Some(FunctionArgumentRequest {
            demand,
            mark: FunctionArgumentMark {
                holds_before: self.holds.len(),
                degradation_before: self.degradation,
            },
        })
    }

    /// A function-value argument's type from its evaluated signature
    /// `value`: `None` when its evaluation did not finish (it held or
    /// degraded the frame, which is then restored) or is no signature.
    pub(super) fn function_argument_value(
        &mut self,
        mark: FunctionArgumentMark,
        value: Positional<SemanticNodeId>,
    ) -> Option<SemanticNodeId> {
        let FunctionArgumentMark {
            holds_before,
            degradation_before,
        } = mark;
        let finished = self.holds.len() == holds_before && self.degradation == degradation_before;
        self.holds.truncate(holds_before);
        if !finished {
            self.degradation = degradation_before;
            return None;
        }
        let Positional::Value(node) = value else {
            return None;
        };
        match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Signature { .. }) => Some(node),
            _ => None,
        }
    }

    /// The type the callee's one call signature declares for the argument
    /// at `ordinal` — the parameter there, or its rest parameter's element —
    /// with each of the signature's type parameters read with its
    /// constraint, as a contextual type reads an unfixed type parameter.
    /// `None` for a callee with no call signature or several.
    fn callee_parameter_type(
        &self,
        callee: SemanticNodeId,
        ordinal: usize,
    ) -> Option<SemanticNodeId> {
        let signatures = match self
            .dispatch
            .shared_signature_nodes(callee, SignatureKind::Call)
        {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(signatures) => {
                signatures
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(_) => return None,
        };
        let [signature] = signatures.as_slice() else {
            return None;
        };
        let graph = self.dispatch.graph();
        let data = graph.node_data(*signature)?;
        let SemanticNodeData::Signature {
            params,
            type_parameters,
            ..
        } = &*data
        else {
            return None;
        };
        let mut parameter = None;
        for (position, param) in params.iter().enumerate() {
            if param.rest {
                parameter = match graph.node_data(param.ty).as_deref() {
                    Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                    _ => None,
                };
                break;
            }
            if position == ordinal {
                parameter = Some(param.ty);
                break;
            }
        }
        let parameter = parameter?;
        Some(type_parameters.iter().fold(parameter, |parameter, decl| {
            let Some(constraint) = decl.constraint else {
                return parameter;
            };
            let constrained = match graph.node_data(decl.param).as_deref() {
                Some(SemanticNodeData::TypeParam {
                    decl: identity,
                    param_index,
                    default,
                    display_name,
                    constraint: None,
                }) => graph.intern_node(SemanticNodeData::TypeParam {
                    decl: identity.clone(),
                    param_index: *param_index,
                    constraint: Some(constraint),
                    default: *default,
                    display_name: std::sync::Arc::clone(display_name),
                }),
                _ => return parameter,
            };
            self.dispatch
                .substitute_semantic_type_param(parameter, decl.param, constrained)
        }))
    }

    /// Whether `step` asks for a context-sensitive argument to be typed
    /// under a contextual type: the argument's position and that type.
    pub(super) fn contextual_argument_request(
        step: &Positional<super::super::call_resolve::ResolveCallStep>,
    ) -> Option<(usize, SemanticNodeId)> {
        match step {
            Positional::Value(super::super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::ContextSensitiveInference {
                    contextual: Some((position, contextual)),
                },
            )) => Some((usize::try_from(*position).ok()?, *contextual)),
            _ => None,
        }
    }
}
