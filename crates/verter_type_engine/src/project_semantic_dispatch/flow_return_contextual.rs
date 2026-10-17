//! A context-sensitive function argument typed under the contextual
//! signature its call hands it — the checker's second inference pass: the
//! arguments before it inferred the callee's type parameters, the
//! contextual type instantiated with those inferences types the function's
//! unannotated parameters, and its body return is read under them.

use super::{FlowDemandDriver, FlowEvaluator, Positional};
use crate::semantic_query::{
    IncompleteReason, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SignatureKind,
};
use verter_session_query::flow::slice::SliceExpr;

impl<C: crate::resolver_core::ResolverCapabilities> super::super::ProjectSemanticDispatch<'_, C> {
    /// Expand a contextual alias without erasing marker applications in its body.
    pub(in crate::project_semantic_dispatch) fn contextual_carrier_view(
        &self,
        node: SemanticNodeId,
    ) -> SemanticNodeId {
        let data = self.graph().node_data(node);
        let Some(SemanticNodeData::InstantiationRef { base, args }) = data.as_deref() else {
            return self.resolved_reduction_view(node);
        };
        let context =
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Skeleton,
            );
        match self.execute_type_node(crate::semantic_query::SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    base.canonical_id.clone(),
                    base.owner,
                    base.decl_name.clone(),
                ),
                std::sync::Arc::clone(args.as_arc()),
                self.instantiate_context_for(&base.canonical_id, context),
            ),
        )) {
            crate::semantic_query::QueryResult::Value(output) => output.value,
            _ => node,
        }
    }

    /// Find the global marker through contextual intersections and aliases,
    /// before its empty interface is reduced to a structural object.
    pub(in crate::project_semantic_dispatch) fn contextual_object_this_type(
        &self,
        contextual: SemanticNodeId,
        canonical: &str,
    ) -> Option<SemanticNodeId> {
        enum Work {
            Visit(SemanticNodeId),
            Join(SemanticNodeId, Vec<SemanticNodeId>, bool),
        }
        let mut pending = vec![Work::Visit(contextual)];
        let mut seen = rustc_hash::FxHashSet::default();
        let mut receivers = rustc_hash::FxHashMap::default();
        while let Some(work) = pending.pop() {
            let node = match work {
                Work::Visit(node) => node,
                Work::Join(node, children, union) => {
                    let found: Vec<_> = children
                        .iter()
                        .filter_map(|child| receivers.get(child).copied())
                        .collect();
                    if let Some(&first) = found.first() {
                        receivers.insert(
                            node,
                            if union {
                                self.intern_normalized_union_or_intersection(&found, true)
                            } else {
                                first
                            },
                        );
                    }
                    continue;
                }
            };
            if !seen.insert(node) {
                continue;
            }
            match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::InstantiationRef { base, args })
                    if base.decl_name.as_ref() == "ThisType" && args.len() == 1 =>
                {
                    if self
                        .first_global_declaration(canonical, "ThisType")
                        .is_some_and(|global| {
                            global.canonical_id == base.canonical_id && global.owner == base.owner
                        })
                    {
                        receivers.insert(node, args[0]);
                    }
                }
                Some(SemanticNodeData::Intersection(members)) => {
                    pending.push(Work::Join(node, members.to_vec(), false));
                    pending.extend(members.iter().rev().copied().map(Work::Visit));
                }
                Some(SemanticNodeData::Union(members)) => {
                    pending.push(Work::Join(node, members.to_vec(), true));
                    pending.extend(members.iter().rev().copied().map(Work::Visit));
                }
                _ => {
                    let resolved = self.contextual_carrier_view(node);
                    if resolved != node {
                        pending.push(Work::Join(node, vec![resolved], false));
                        pending.push(Work::Visit(resolved));
                    }
                }
            }
        }
        receivers.get(&contextual).copied()
    }
}

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
    pub(super) receiver: Option<SemanticNodeId>,
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

/// The sole callee signature's contextual inputs, prepared once for a call
/// route. Sibling callbacks share discovery and constraint substitution.
pub(super) struct CalleeParameters {
    parameters: std::sync::Arc<[crate::semantic_query::FunctionParam]>,
    fixed_count: usize,
    rest_element: Option<SemanticNodeId>,
    constraints: Vec<(SemanticNodeId, SemanticNodeId)>,
    substituted: rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
}

impl CalleeParameters {
    pub(super) fn at<D: FlowDemandDriver>(
        &mut self,
        dispatch: &D,
        ordinal: usize,
    ) -> Option<SemanticNodeId> {
        super::super::call_resolve::record_argument_work(1);
        let parameter = if ordinal < self.fixed_count {
            self.parameters.get(ordinal).map(|param| param.ty)
        } else {
            self.rest_element
        }?;
        Some(*self.substituted.entry(parameter).or_insert_with(|| {
            self.constraints
                .iter()
                .fold(parameter, |parameter, &(param, constrained)| {
                    dispatch.substitute_semantic_type_param(parameter, param, constrained)
                })
        }))
    }
}

impl<D: FlowDemandDriver> FlowEvaluator<'_, '_, D> {
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
        let (receiver, params) = crate::semantic_query::split_this_receiver(params);
        let mut parameters = Vec::with_capacity(params.len());
        let mut rest_element = None;
        for param in params.iter() {
            if param.rest {
                rest_element = match graph.node_data(param.ty).as_deref() {
                    Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                    Some(SemanticNodeData::Tuple { elements, .. })
                        if elements
                            .iter()
                            .all(|element| !element.optional && !element.rest) =>
                    {
                        parameters.extend(elements.iter().map(|element| element.value));
                        None
                    }
                    _ => return None,
                };
                break;
            }
            parameters.push(param.ty);
        }
        Some(ContextualSignature {
            receiver: receiver.map(|param| param.ty),
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
        parameter: Option<SemanticNodeId>,
    ) -> Option<FunctionArgumentRequest> {
        if !matches!(expr, SliceExpr::NestedFunctionValue { gap: None, .. }) {
            return None;
        }
        let signature = parameter.and_then(|parameter| self.contextual_signature(parameter));
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

    /// The types the callee's one call signature declares for its arguments,
    /// with each of the signature's type parameters read with its
    /// constraint, as a contextual type reads an unfixed type parameter.
    /// `None` for a callee with no call signature or several.
    pub(super) fn callee_parameter_types(
        &self,
        callee: SemanticNodeId,
    ) -> Result<Option<CalleeParameters>, IncompleteReason> {
        let signatures = match self
            .dispatch
            .shared_signature_nodes(callee, SignatureKind::Call)
        {
            super::super::signature_discovery::SharedSignatureNodes::Nodes(signatures) => {
                signatures
            }
            super::super::signature_discovery::SharedSignatureNodes::Incomplete(reason) => {
                return Err(reason)
            }
        };
        let [signature] = signatures.as_slice() else {
            return Ok(None);
        };
        let graph = self.dispatch.graph();
        let data = graph
            .node_data(*signature)
            .ok_or(IncompleteReason::UnsettledInput)?;
        let header = data
            .callable_header(&crate::semantic_query::ResolveOverloadSetConsumer::witness())
            .ok_or(IncompleteReason::UnsettledInput)?;
        let (params, type_parameters) = (header.params, header.type_parameters);
        let constraints: Vec<_> = type_parameters
            .iter()
            .filter_map(|decl| {
                let constraint = decl.constraint?;
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
                    _ => return None,
                };
                Some((decl.param, constrained))
            })
            .collect();
        let mut fixed_count = params.len();
        let mut rest_element = None;
        for (position, param) in params.iter().enumerate() {
            super::super::call_resolve::record_argument_work(1);
            if param.rest {
                fixed_count = position;
                rest_element = match graph.node_data(param.ty).as_deref() {
                    Some(SemanticNodeData::Array { element, .. }) => Some(*element),
                    _ => None,
                };
                break;
            }
        }
        Ok(Some(CalleeParameters {
            parameters: std::sync::Arc::clone(params),
            fixed_count,
            rest_element,
            constraints,
            substituted: rustc_hash::FxHashMap::default(),
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
