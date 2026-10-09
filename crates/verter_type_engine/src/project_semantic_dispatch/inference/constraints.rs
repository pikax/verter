//! An `infer X extends C` declaration's constraint: the capture a template
//! literal hole converts by it (`inferToTemplateLiteralType`) and the
//! constraint a fixed inference falls back to (`getInferredType`).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::dispatch_txn::RelationStep;
use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{
    InferBinding, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};

/// Where a constraint member stands in the checker's preference among the
/// members a template capture may convert to, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CaptureTier {
    TemplateOrMapping,
    StringLiteral,
    Number,
    NumberLiteral,
    BigInt,
    BigIntLiteral,
    BooleanLiteral,
    Undefined,
    Null,
}

impl<C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'_, C> {
    pub(in crate::project_semantic_dispatch) fn template_capture(
        &self,
        slice: SemanticNodeId,
        hole: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        super::InferenceTxn::new(self).template_capture(slice, hole)
    }
    pub(in crate::project_semantic_dispatch) fn infer_constraints_in(
        &self,
        pattern: SemanticNodeId,
    ) -> FxHashMap<SemanticNodeId, SemanticNodeId> {
        super::InferenceTxn::new(self).infer_constraints_in(pattern)
    }
    pub(in crate::project_semantic_dispatch) fn infer_bindings_within_constraints(
        &self,
        bindings: &Arc<[InferBinding]>,
        constraints: &FxHashMap<SemanticNodeId, SemanticNodeId>,
    ) -> Option<Arc<[InferBinding]>> {
        super::InferenceTxn::new(self).infer_bindings_within_constraints(bindings, constraints)
    }
}
impl<D: super::InferenceDemandDriver> super::InferenceTxn<'_, D> {
    /// The constraint an `infer` declaration writes, if any.
    pub(in crate::project_semantic_dispatch) fn infer_constraint(
        &self,
        infer: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        match self.graph().node_data(infer).as_deref() {
            Some(SemanticNodeData::Infer { constraint, .. }) => *constraint,
            _ => None,
        }
    }

    /// The candidate a template literal hole's `infer` takes from the
    /// string literal `slice` it captures (`inferToTemplateLiteralType`).
    /// Under a constraint holding no `string`, the capture converts to the
    /// member the checker prefers: a template or string mapping it matches
    /// or a string literal it spells keeps the string; a `number` member
    /// takes the numeric literal the string round-trips to (`"42"` is `42`,
    /// `"0x10"` is not one); a `bigint` member the bigint literal; a
    /// boolean, `undefined` or `null` literal the one the string names.
    /// With none, or without a constraint, the capture is the string.
    /// `None` when whether a member matches is undecided.
    pub(in crate::project_semantic_dispatch) fn template_capture(
        &self,
        slice: SemanticNodeId,
        hole: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let Some(constraint) = self.infer_constraint(hole) else {
            return Some(slice);
        };
        let text = match self.graph().node_data(slice).as_deref() {
            Some(SemanticNodeData::Literal(LiteralValue::String(text))) => text.clone(),
            _ => return Some(slice),
        };
        let resolved = self
            .normalize_node_for_structural_fact_demand(
                constraint,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .into_usable_node()?;
        let graph = self.graph();
        let mut members: Vec<SemanticNodeId> = match graph.node_data(resolved).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.members_arc().to_vec(),
            _ => vec![resolved],
        };
        // `boolean` is the union of its two literals.
        let boolean =
            |value| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(value)));
        members = members
            .into_iter()
            .flat_map(|member| match graph.node_data(member).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean)) => {
                    vec![boolean(false), boolean(true)]
                }
                _ => vec![member],
            })
            .collect();
        let kind = |member: SemanticNodeId| graph.node_data(member).as_deref().cloned();
        if members.iter().any(|member| {
            matches!(
                kind(*member),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::String | PrimitiveKind::Any
                ))
            )
        }) {
            return Some(slice);
        }
        let number = round_trip_number(&text);
        let bigint = round_trip_bigint(&text);
        let mut best: Option<(CaptureTier, SemanticNodeId)> = None;
        for member in members {
            let candidate = match kind(member) {
                Some(SemanticNodeData::TemplateLiteral { .. })
                | Some(SemanticNodeData::IntrinsicApplication { .. }) => {
                    let step = self.execute_relate_pair(slice, member);
                    match step {
                        RelationStep::Assignable { .. } => {
                            Some((CaptureTier::TemplateOrMapping, slice))
                        }
                        RelationStep::NotAssignable => None,
                        _ => return None,
                    }
                }
                Some(SemanticNodeData::Literal(LiteralValue::String(value))) => {
                    (value == text).then_some((CaptureTier::StringLiteral, member))
                }
                Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => number.map(|value| {
                    (
                        CaptureTier::Number,
                        graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(value))),
                    )
                }),
                Some(SemanticNodeData::Literal(LiteralValue::Number(value))) => number
                    .filter(|number| number.to_bits() == value.to_bits())
                    .map(|_| (CaptureTier::NumberLiteral, member)),
                Some(SemanticNodeData::Primitive(PrimitiveKind::BigInt)) => {
                    bigint.as_ref().map(|value| {
                        (
                            CaptureTier::BigInt,
                            graph.intern_node(SemanticNodeData::Literal(LiteralValue::BigInt(
                                value.clone(),
                            ))),
                        )
                    })
                }
                Some(SemanticNodeData::Literal(LiteralValue::BigInt(value))) => bigint
                    .as_ref()
                    .filter(|bigint| **bigint == value)
                    .map(|_| (CaptureTier::BigIntLiteral, member)),
                Some(SemanticNodeData::Literal(LiteralValue::Boolean(value))) => (text
                    == if value { "true" } else { "false" })
                .then_some((CaptureTier::BooleanLiteral, member)),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined)) => {
                    (text == "undefined").then_some((CaptureTier::Undefined, member))
                }
                Some(SemanticNodeData::Primitive(PrimitiveKind::Null)) => {
                    (text == "null").then_some((CaptureTier::Null, member))
                }
                _ => None,
            };
            if let Some((tier, converted)) = candidate {
                if best.is_none_or(|(kept, _)| tier < kept) {
                    best = Some((tier, converted));
                }
            }
        }
        Some(best.map_or(slice, |(_, converted)| converted))
    }

    /// The constraint each `infer` of `pattern` fixes within: the one it
    /// declares (`infer X extends C`), else the one its position implies
    /// (`getInferredTypeParameterConstraint`) — a reference's type argument
    /// takes the constraint of the reference's type parameter there,
    /// instantiated with the reference's type arguments; a rest element or
    /// rest parameter takes `unknown[]`; a template literal hole takes
    /// `string`; several positions take the intersection of theirs. One
    /// walk over the pattern.
    pub(in crate::project_semantic_dispatch) fn infer_constraints_in(
        &self,
        pattern: SemanticNodeId,
    ) -> FxHashMap<SemanticNodeId, SemanticNodeId> {
        let graph = self.graph();
        let is_infer = |node: SemanticNodeId| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Infer { .. })
            )
        };
        let mut implied: FxHashMap<SemanticNodeId, Vec<SemanticNodeId>> = FxHashMap::default();
        let mut declared: FxHashMap<SemanticNodeId, SemanticNodeId> = FxHashMap::default();
        let unknown_array = || {
            let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
            graph.intern_node(SemanticNodeData::Array {
                element: unknown,
                readonly: false,
            })
        };
        let mut seen: FxHashSet<SemanticNodeId> = FxHashSet::default();
        let mut stack = vec![pattern];
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::Infer {
                    constraint: Some(constraint),
                    ..
                } => {
                    declared.insert(node, *constraint);
                }
                SemanticNodeData::InstantiationRef { base, args }
                    if base.canonical_id.as_ref() != "__builtin__" =>
                {
                    for (index, arg) in args.iter().enumerate() {
                        if is_infer(*arg) {
                            if let Some(constraint) =
                                self.declared_parameter_constraint(base, index, args)
                            {
                                if constraint != *arg {
                                    implied.entry(*arg).or_default().push(constraint);
                                }
                            }
                        }
                    }
                }
                SemanticNodeData::Tuple { elements, .. } => {
                    for element in elements.iter() {
                        if element.rest && is_infer(element.value) {
                            implied
                                .entry(element.value)
                                .or_default()
                                .push(unknown_array());
                        }
                    }
                }
                SemanticNodeData::Signature { params, .. } => {
                    for param in params.iter() {
                        if param.rest && is_infer(param.ty) {
                            implied.entry(param.ty).or_default().push(unknown_array());
                        }
                    }
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    for hole in expressions.iter() {
                        if is_infer(*hole) {
                            let string = graph
                                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
                            implied.entry(*hole).or_default().push(string);
                        }
                    }
                }
                _ => {}
            }
            let _ = data.for_each_child(|child| stack.push(child));
        }
        let mut constraints = declared;
        for (infer, positions) in implied {
            if constraints.contains_key(&infer) {
                continue;
            }
            let constraint = match positions.as_slice() {
                [one] => *one,
                many => self.intern_normalized_union_or_intersection(many, false),
            };
            constraints.insert(infer, constraint);
        }
        constraints
    }

    /// The constraint of type parameter `index` of the generic declaration
    /// `base`, instantiated with `args`; `None` when it declares none or the
    /// declaration cannot be read.
    fn declared_parameter_constraint(
        &self,
        base: &crate::semantic_query::DeclIdentity,
        index: usize,
        args: &[SemanticNodeId],
    ) -> Option<SemanticNodeId> {
        let prepared = self.source.prepared_type_decl_return_only(
            base.canonical_id.as_ref(),
            base.owner,
            base.decl_name.as_ref(),
        )?;
        prepared.type_parameters.get(index)?.constraint.as_ref()?;
        let binders = self.locator_binder_frame_from_narrow_params(
            &super::super::relation_variance::declaration_scope(base),
            &base.decl_name,
            &prepared.type_parameters,
        );
        let (_, binder) = binders.get(index)?;
        let constraint = match self.graph().node_data(*binder).as_deref() {
            Some(SemanticNodeData::TypeParam {
                constraint: Some(constraint),
                ..
            }) => *constraint,
            _ => return None,
        };
        Some(
            binders
                .iter()
                .zip(args)
                .fold(constraint, |node, ((_, binder), arg)| {
                    self.substitute_semantic_type_param(node, *binder, *arg)
                }),
        )
    }

    /// `bindings` with each `infer` that `constraints` constrains the
    /// checker's fixed type (`getInferredType`): its inference when it is
    /// assignable to its constraint, else that constraint. `None` when a
    /// constraint relation is undecided.
    pub(in crate::project_semantic_dispatch) fn infer_bindings_within_constraints(
        &self,
        bindings: &Arc<[InferBinding]>,
        constraints: &FxHashMap<SemanticNodeId, SemanticNodeId>,
    ) -> Option<Arc<[InferBinding]>> {
        let mut fixed: Vec<InferBinding> = bindings.to_vec();
        for binding in fixed.iter_mut() {
            let Some(&constraint) = constraints.get(&binding.param) else {
                continue;
            };
            let step = self.execute_relate_pair(binding.bound, constraint);
            match step {
                RelationStep::Assignable { .. } => {}
                RelationStep::NotAssignable => binding.bound = constraint,
                _ => return None,
            }
        }
        Some(Arc::from(fixed.into_boxed_slice()))
    }
}

/// The number `text` spells when JavaScript's `+text` round-trips to it
/// (`isValidNumberString(text, true)`).
fn round_trip_number(text: &str) -> Option<f64> {
    let value = text.parse::<f64>().ok().filter(|value| value.is_finite())?;
    (crate::semantic_query::index_key::js_number_to_string(value) == text).then_some(value)
}

/// The bigint `text` spells when it round-trips (`isValidBigIntString(text,
/// true)`): an optional `-` and decimal digits without a leading zero.
fn round_trip_bigint(text: &str) -> Option<String> {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let canonical = !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && text != "-0";
    canonical.then(|| text.to_string())
}
