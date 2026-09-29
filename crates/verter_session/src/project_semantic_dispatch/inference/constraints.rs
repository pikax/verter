//! An `infer X extends C` declaration's constraint: the capture a template
//! literal hole converts by it (`inferToTemplateLiteralType`) and the
//! constraint a fixed inference falls back to (`getInferredType`).

use std::sync::Arc;

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

impl ProjectSemanticDispatch<'_> {
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
            .into_complete_node()?;
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
                    self.dispatch_txn.borrow_mut().begin_binding_disabled();
                    let step = self.execute_relate_pair(slice, member);
                    self.dispatch_txn.borrow_mut().end_binding_disabled();
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

    /// `bindings` with each constrained `infer` the checker's fixed type
    /// (`getInferredType`): its inference when it is assignable to its
    /// constraint, else that constraint. A constraint reads no `infer` of its
    /// pattern (the checker resolves it where the conditional is written).
    /// `None` when a constraint relation is undecided.
    pub(in crate::project_semantic_dispatch) fn infer_bindings_within_constraints(
        &self,
        bindings: &Arc<[InferBinding]>,
    ) -> Option<Arc<[InferBinding]>> {
        let mut fixed: Vec<InferBinding> = bindings.to_vec();
        for binding in fixed.iter_mut() {
            let Some(constraint) = self.infer_constraint(binding.param) else {
                continue;
            };
            self.dispatch_txn.borrow_mut().begin_binding_disabled();
            let step = self.execute_relate_pair(binding.bound, constraint);
            self.dispatch_txn.borrow_mut().end_binding_disabled();
            match step {
                RelationStep::Assignable { .. } => {}
                RelationStep::NotAssignable => binding.bound = constraint,
                _ => return None,
            }
        }
        Some(Arc::from(fixed.into_boxed_slice()))
    }

    /// Whether `pattern` declares an `infer` with a constraint.
    pub(in crate::project_semantic_dispatch) fn pattern_constrains_an_infer(
        &self,
        pattern: SemanticNodeId,
    ) -> bool {
        self.infer_scan(pattern, true)
            .infers
            .iter()
            .any(|infer| self.infer_constraint(*infer).is_some())
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
