//! Operator evaluation: the flow evaluator's typing of arithmetic, bitwise
//! and string-concatenating operators, element accesses and update
//! expressions over their operands' TYPES — the checker's
//! `checkBinaryLikeExpression`, `checkPrefixUnaryExpression` /
//! `getUnaryResultType` and element-access rules, measured on 7.0.2.

use std::sync::Arc;

use super::super::dispatch_txn::RelationStep;
use super::{FlowEvaluator, FlowProductSubject, Positional};
use crate::flow_slice_content::{
    SliceArithmetic, SliceExpr, SliceGuard, SliceLogical, SliceNarrowRoot, SliceNarrowSubject,
};
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, LiteralValue, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};

/// The primitive family an `isTypeAssignableToKind` / `maybeTypeOfKind`
/// question names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperandKind {
    Number,
    BigInt,
    String,
}

impl OperandKind {
    fn primitive(self) -> PrimitiveKind {
        match self {
            Self::Number => PrimitiveKind::Number,
            Self::BigInt => PrimitiveKind::BigInt,
            Self::String => PrimitiveKind::String,
        }
    }

    /// Whether a primitive or literal node is of this family by its own
    /// flags (`source.flags & kind`); `None` for every other node.
    fn of(self, data: &SemanticNodeData) -> Option<bool> {
        match data {
            SemanticNodeData::Primitive(primitive) => Some(*primitive == self.primitive()),
            SemanticNodeData::Literal(literal) => Some(matches!(
                (self, literal),
                (Self::Number, LiteralValue::Number(_))
                    | (Self::BigInt, LiteralValue::BigInt(_))
                    | (Self::String, LiteralValue::String(_))
            )),
            SemanticNodeData::TemplateLiteral { .. } => Some(self == Self::String),
            _ => None,
        }
    }
}

impl FlowEvaluator<'_, '_> {
    /// [`SliceExpr::Arithmetic`]: every operand evaluates in order, then the
    /// operator's result follows the checker's rule over their types. A
    /// relation the shared authority cannot decide leaves the typed
    /// relation gap at the position.
    pub(super) fn eval_arithmetic(
        &mut self,
        operator: SliceArithmetic,
        operands: &[SliceExpr],
    ) -> Positional<SemanticNodeId> {
        let mut types: Vec<SemanticNodeId> = Vec::with_capacity(operands.len());
        for operand in operands {
            match self.eval_expr(operand) {
                Positional::Value(node) => types.push(node),
                Positional::Hold => return Positional::Hold,
                Positional::Unmodeled => return Positional::Unmodeled,
            }
        }
        let result = match (operator, types.as_slice()) {
            (SliceArithmetic::Plus, [_]) => Some(self.primitive(PrimitiveKind::Number)),
            (SliceArithmetic::Negate, [operand]) => self.unary_numeric_result(*operand),
            (SliceArithmetic::Numeric, [left, right]) => self.numeric_result(*left, *right),
            (SliceArithmetic::Add, [left, right]) => self.addition_result(*left, *right),
            _ => None,
        };
        self.decided(result)
    }

    /// [`SliceExpr::ElementAccess`]: the checker's indexed access of an
    /// array's, a tuple's or a string's type by a numeric key's type — unless
    /// the key names the member reference a narrowing stands on. An `any`
    /// object reads `any`.
    pub(super) fn eval_element_access(
        &mut self,
        object: &SliceExpr,
        index: &SliceExpr,
        reference: Option<(
            &SliceNarrowSubject,
            &crate::flow_slice_content::SliceElementKey,
        )>,
    ) -> Positional<SemanticNodeId> {
        let object = match self.eval_expr(object) {
            Positional::Value(node) => node,
            other => return other,
        };
        let index = match self.eval_expr(index) {
            Positional::Value(node) => node,
            other => return other,
        };
        if let Some((reference, key)) = reference {
            if let Some(segment) = self.element_segment(index, key) {
                let mut path = reference.path.to_vec();
                path.push(segment);
                let subject = SliceNarrowSubject {
                    root: reference.root.clone(),
                    path: Arc::from(path.into_boxed_slice()),
                };
                if let Some(narrowed) = self.narrowed_read(&subject) {
                    return Positional::Value(narrowed);
                }
            }
        }
        let value = self.element_type(object, index);
        match value {
            Some(node)
                if !matches!(
                    self.dispatch.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Opaque(_))
                ) =>
            {
                Positional::Value(node)
            }
            _ => {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    FlowGap::UnmodeledExpression,
                ));
                Positional::Unmodeled
            }
        }
    }

    /// The segment an element-access key spells under its object's
    /// reference ([`crate::flow_slice_content::SliceElementKey`]): a
    /// `const` key of one string or numeric literal type names that
    /// member, any other unassigned key its identity segment.
    pub(super) fn element_segment(
        &self,
        index: SemanticNodeId,
        key: &crate::flow_slice_content::SliceElementKey,
    ) -> Option<Arc<str>> {
        if key.constant {
            let name: Option<Arc<str>> = match self.dispatch.graph().node_data(index).as_deref() {
                Some(SemanticNodeData::Literal(LiteralValue::String(value)))
                    if !value.starts_with('\u{0}') =>
                {
                    Some(Arc::from(value.as_str()))
                }
                Some(SemanticNodeData::Literal(LiteralValue::Number(value))) => Some(Arc::from(
                    crate::semantic_query::index_key::js_number_to_string(*value).as_str(),
                )),
                _ => None,
            };
            if name.is_some() {
                return name;
            }
        }
        key.identity.clone()
    }

    /// A computed write `o[k] = v` whose key reads a frame binding: the
    /// written reference is the object's extended by the key's segment,
    /// reduced against the checker's indexed access of the object's type
    /// by the key's type. A key spelling no segment (a key binding some
    /// write reaches) writes no reference: the right-hand side still runs.
    pub(super) fn apply_keyed_member_write(
        &mut self,
        target: &SliceNarrowSubject,
        key: &crate::flow_slice_content::SliceWriteKey,
        write: &crate::flow_slice_content::SliceMemberWrite,
    ) {
        let index = match self.eval_expr(&key.value) {
            Positional::Value(node) => Some(node),
            Positional::Hold | Positional::Unmodeled => None,
        };
        let segment = index.and_then(|index| self.element_segment(index, &key.key));
        let Some(segment) = segment else {
            if let crate::flow_slice_content::SliceMemberWrite::Assign { value, .. } = write {
                self.prescan_statement_value_writes(Some(value));
                let holds_before = self.holds.len();
                let _ = self.eval_expr(value);
                self.holds.truncate(holds_before);
            }
            if index.is_none() {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    FlowGap::UnmodeledExpression,
                ));
            }
            return;
        };
        let declared = match (self.reference_node(target, true), index) {
            (Some(object), Some(index)) => self.element_type(object, index),
            _ => None,
        };
        let mut path = target.path.to_vec();
        path.push(segment);
        let written = SliceNarrowSubject {
            root: target.root.clone(),
            path: Arc::from(path.into_boxed_slice()),
        };
        self.apply_member_write_declared(&written, write, declared);
    }

    /// The checker's indexed access of `object` by `index` where this lane
    /// reads it: `any` for an `any` object, a numeric position of an
    /// array, a tuple or a string, and the named members a key of string or
    /// numeric literal types spells. `None` for any other access (an index
    /// signature, a mapped type).
    fn element_type(
        &mut self,
        object: SemanticNodeId,
        index: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        if self.is_any_node(object) {
            return Some(object);
        }
        // A numeric position of an array, a tuple or a string: the checker's
        // indexed access by the key's type.
        let positional = self.operand_arms(object).into_iter().all(|arm| {
            matches!(
                self.dispatch
                    .graph()
                    .node_data(self.dispatch.resolved_reduction_view(arm))
                    .as_deref(),
                Some(
                    SemanticNodeData::Array { .. }
                        | SemanticNodeData::Tuple { .. }
                        | SemanticNodeData::Primitive(PrimitiveKind::String)
                        | SemanticNodeData::Literal(LiteralValue::String(_))
                )
            )
        }) && self.is_numeric_key(index);
        if positional {
            return self.positional_element(object, index);
        }
        // A key of string or numeric literal types reads the members it
        // names; a key of any other type (an index-signature read) is not
        // read here.
        let mut members = Vec::new();
        for key in self.operand_arms(index) {
            let name: Arc<str> = match self.dispatch.graph().node_data(key).as_deref() {
                Some(SemanticNodeData::Literal(LiteralValue::String(value))) => {
                    Arc::from(value.as_str())
                }
                Some(SemanticNodeData::Literal(LiteralValue::Number(value))) => Arc::from(
                    crate::semantic_query::index_key::js_number_to_string(*value).as_str(),
                ),
                _ => return None,
            };
            members.push(self.project_member_path(object, std::slice::from_ref(&name))?);
        }
        (!members.is_empty()).then(|| self.union(&members))
    }

    /// [`SliceExpr::Update`]: the value is the unary numeric result over
    /// the target's current type; the write then retypes the target to the
    /// base type of the literal type it held, exactly as the statement
    /// twin does.
    pub(super) fn eval_update(
        &mut self,
        target: &SliceNarrowSubject,
    ) -> Positional<SemanticNodeId> {
        let Some(current) = self.subject_current_node(target) else {
            let marker = super::super::flow_return_callee::unmodeled_position_marker(self.dispatch);
            self.bind_written(target, marker, true, None, false);
            return Positional::Unmodeled;
        };
        let result = self.unary_numeric_result(current);
        let written = self.base_type_of_literal(current);
        let definition = match &target.root {
            SliceNarrowRoot::Param { binding, .. }
            | SliceNarrowRoot::Local {
                binding: FlowProductSubject::Local(binding),
                ..
            } => Some(self.flow_graph.binding_node(*binding)),
            SliceNarrowRoot::Local { .. } => None,
        };
        self.bind_written(target, written, false, definition, false);
        self.decided(result)
    }

    /// The element a numeric `index` reads off an array (its element type),
    /// a string (`string`) or a tuple (the checker's indexed access), over
    /// every member of `object`.
    fn positional_element(
        &mut self,
        object: SemanticNodeId,
        index: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let mut elements = Vec::new();
        for arm in self.operand_arms(object) {
            let resolved = self.dispatch.resolved_reduction_view(arm);
            let data = self.dispatch.graph().node_data(resolved);
            let element = match data.as_deref()? {
                SemanticNodeData::Array { element, .. } => *element,
                SemanticNodeData::Primitive(PrimitiveKind::String)
                | SemanticNodeData::Literal(LiteralValue::String(_)) => {
                    self.primitive(PrimitiveKind::String)
                }
                _ => {
                    drop(data);
                    self.indexed_access(resolved, index)?
                }
            };
            elements.push(element);
        }
        Some(match elements.as_slice() {
            [single] => *single,
            _ => self.union(&elements),
        })
    }

    fn is_any_node(&self, node: SemanticNodeId) -> bool {
        matches!(
            self.dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
        )
    }

    /// Whether an index type reads a numeric position: `number`, a numeric
    /// literal, a union of those, or `any`.
    fn is_numeric_key(&self, index: SemanticNodeId) -> bool {
        self.operand_arms(index).into_iter().all(|arm| {
            matches!(
                self.dispatch.graph().node_data(arm).as_deref(),
                Some(
                    SemanticNodeData::Primitive(PrimitiveKind::Number | PrimitiveKind::Any)
                        | SemanticNodeData::Literal(LiteralValue::Number(_))
                )
            )
        })
    }

    /// A decided result, or the typed relation gap at the position.
    fn decided(&mut self, result: Option<SemanticNodeId>) -> Positional<SemanticNodeId> {
        match result {
            Some(node) => Positional::Value(node),
            None => {
                self.record_degradation(FlowReturnDegradation::FlowGap(FlowGap::NominalRelation));
                Positional::Unmodeled
            }
        }
    }

    fn primitive(&self, kind: PrimitiveKind) -> SemanticNodeId {
        self.dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(kind))
    }

    /// The members of a union, or the node itself.
    fn operand_arms(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        self.dispatch
            .union_arms_of(node)
            .map_or_else(|| vec![node], |arms| arms.to_vec())
    }

    /// The operand of an arithmetic operator after `checkNonNullType`: its
    /// non-nullable part, or the checker's error type (`any`) when
    /// nothing else is left (tsc 7.0.2: `null + i` over `i: number` is
    /// `any` with and without `strictNullChecks`).
    fn checked_arithmetic_operand(&self, node: SemanticNodeId) -> SemanticNodeId {
        let checked = self.non_nullable_operand(node);
        if checked != node && self.is_primitive(checked, &[PrimitiveKind::Never]) {
            return self.primitive(PrimitiveKind::Any);
        }
        checked
    }

    /// `getNonNullableType`: the operand without its `null`, `undefined`
    /// and `void` members.
    fn non_nullable_operand(&self, node: SemanticNodeId) -> SemanticNodeId {
        let arms = self.operand_arms(node);
        let kept: Vec<SemanticNodeId> = arms
            .iter()
            .copied()
            .filter(|arm| {
                !matches!(
                    self.dispatch.graph().node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Primitive(
                        PrimitiveKind::Null | PrimitiveKind::Undefined | PrimitiveKind::Void
                    ))
                )
            })
            .collect();
        match kept.as_slice() {
            _ if kept.len() == arms.len() => node,
            [] => self.primitive(PrimitiveKind::Never),
            [single] => *single,
            _ => self.union(&kept),
        }
    }

    fn is_primitive(&self, node: SemanticNodeId, kinds: &[PrimitiveKind]) -> bool {
        matches!(
            self.dispatch.graph().node_data(node).as_deref(),
            Some(SemanticNodeData::Primitive(primitive)) if kinds.contains(primitive)
        )
    }

    /// `isTypeAssignableToKind(node, kind, strict)`: the node's own flags,
    /// then (`strict` refusing `any` / `unknown` / `void` / `undefined` /
    /// `null` themselves) assignability to the family's primitive through
    /// the shared relation authority. `None` when that relation is
    /// undecided.
    fn assignable_to_kind(
        &mut self,
        node: SemanticNodeId,
        kind: OperandKind,
        strict: bool,
    ) -> Option<bool> {
        if let Some(true) = self
            .dispatch
            .graph()
            .node_data(node)
            .as_deref()
            .and_then(|data| kind.of(data))
        {
            return Some(true);
        }
        if strict
            && self.is_primitive(
                node,
                &[
                    PrimitiveKind::Any,
                    PrimitiveKind::Unknown,
                    PrimitiveKind::Void,
                    PrimitiveKind::Undefined,
                    PrimitiveKind::Null,
                ],
            )
        {
            return Some(false);
        }
        // A type parameter is assignable to the family exactly when its
        // constraint is (none constrains it to `unknown`, which is not).
        let constraint = match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::TypeParam { constraint, .. }) => Some(*constraint),
            _ => None,
        };
        if let Some(constraint) = constraint {
            return match constraint {
                Some(constraint) => self.assignable_to_kind(constraint, kind, false),
                None => Some(false),
            };
        }
        let target = self.primitive(kind.primitive());
        match self.dispatch.execute_relate_pair(node, target) {
            RelationStep::Assignable { .. } => Some(true),
            RelationStep::NotAssignable => Some(false),
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed(_) => {
                None
            }
        }
    }

    /// `maybeTypeOfKind(node, kind)`: whether some member of the node —
    /// through unions and intersections — is of the family by its own
    /// flags. A type parameter is not, whatever its constraint (measured:
    /// `a * a` over `a: T extends bigint` is `number`), and `any` /
    /// `unknown` are of no family.
    fn maybe_of_kind(&self, node: SemanticNodeId, kind: OperandKind) -> bool {
        let mut pending = vec![node];
        let mut seen: Vec<SemanticNodeId> = Vec::new();
        while let Some(node) = pending.pop() {
            if seen.contains(&node) {
                continue;
            }
            seen.push(node);
            let data = self.dispatch.graph().node_data(node);
            match data.as_deref() {
                Some(SemanticNodeData::Union(members)) => pending.extend(members.to_vec()),
                Some(SemanticNodeData::Intersection(members)) => pending.extend(members.to_vec()),
                Some(data) if kind.of(data) == Some(true) => return true,
                _ => {}
            }
        }
        false
    }

    /// `getUnaryResultType` over the non-nullable operand (unary `-` / `~`,
    /// `++` / `--`): `bigint` for a `bigint` operand, `number | bigint`
    /// when it may also be a number or is `any` / `unknown`, else `number`.
    fn unary_numeric_result(&mut self, operand: SemanticNodeId) -> Option<SemanticNodeId> {
        let operand = self.checked_arithmetic_operand(operand);
        if !self.maybe_of_kind(operand, OperandKind::BigInt) {
            return Some(self.primitive(PrimitiveKind::Number));
        }
        let bigint = self.primitive(PrimitiveKind::BigInt);
        if self.is_primitive(operand, &[PrimitiveKind::Any, PrimitiveKind::Unknown])
            || self.maybe_of_kind(operand, OperandKind::Number)
        {
            let number = self.primitive(PrimitiveKind::Number);
            return Some(self.union(&[number, bigint]));
        }
        Some(bigint)
    }

    /// The arithmetic / bitwise operators' result: `number` when both
    /// operands are `any` / `unknown` or neither may be a `bigint`,
    /// `bigint` when both are, and the checker's error type (`any`)
    /// otherwise.
    fn numeric_result(
        &mut self,
        left: SemanticNodeId,
        right: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let left = self.checked_arithmetic_operand(left);
        let right = self.checked_arithmetic_operand(right);
        let any_or_unknown = [PrimitiveKind::Any, PrimitiveKind::Unknown];
        if (self.is_primitive(left, &any_or_unknown) && self.is_primitive(right, &any_or_unknown))
            || !(self.maybe_of_kind(left, OperandKind::BigInt)
                || self.maybe_of_kind(right, OperandKind::BigInt))
        {
            return Some(self.primitive(PrimitiveKind::Number));
        }
        if self.assignable_to_kind(left, OperandKind::BigInt, false)?
            && self.assignable_to_kind(right, OperandKind::BigInt, false)?
        {
            return Some(self.primitive(PrimitiveKind::BigInt));
        }
        Some(self.primitive(PrimitiveKind::Any))
    }

    /// The `+` operator's result: `number` when both operands are numbers,
    /// `bigint` when both are bigints, `string` when either is a string,
    /// `any` when either is `any`, and the checker's error type (`any`)
    /// otherwise. The operands lose `null` / `undefined` unless one of
    /// them is a string.
    fn addition_result(
        &mut self,
        left: SemanticNodeId,
        right: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let (left, right) = if !self.assignable_to_kind(left, OperandKind::String, true)?
            && !self.assignable_to_kind(right, OperandKind::String, true)?
        {
            (
                self.checked_arithmetic_operand(left),
                self.checked_arithmetic_operand(right),
            )
        } else {
            (left, right)
        };
        for (kind, primitive) in [
            (OperandKind::Number, PrimitiveKind::Number),
            (OperandKind::BigInt, PrimitiveKind::BigInt),
        ] {
            if self.assignable_to_kind(left, kind, true)?
                && self.assignable_to_kind(right, kind, true)?
            {
                return Some(self.primitive(primitive));
            }
        }
        if self.assignable_to_kind(left, OperandKind::String, true)?
            || self.assignable_to_kind(right, OperandKind::String, true)?
        {
            return Some(self.primitive(PrimitiveKind::String));
        }
        Some(self.primitive(PrimitiveKind::Any))
    }
}

impl FlowEvaluator<'_, '_> {
    /// [`SliceExpr::NonNull`]: `getNonNullableType` of the operand's value
    /// — its `null` / `undefined` / `void` members removed under
    /// `strictNullChecks`, the value itself with it off. A member whose
    /// nullability this frame cannot prove (a type parameter, whose
    /// non-nullable type is `T & {}`) leaves the typed gap.
    pub(super) fn eval_non_null(&mut self, operand: &SliceExpr) -> Positional<SemanticNodeId> {
        let node = match self.eval_expr(operand) {
            Positional::Value(node) => node,
            other => return other,
        };
        if !self.nullability.is_strict() {
            return Positional::Value(node);
        }
        for arm in self.operand_arms(node) {
            let data = self.dispatch.graph().node_data(arm);
            let proven = match data.as_deref() {
                Some(
                    SemanticNodeData::Primitive(_)
                    | SemanticNodeData::Literal(_)
                    | SemanticNodeData::Object(_)
                    | SemanticNodeData::Array { .. }
                    | SemanticNodeData::Tuple { .. }
                    | SemanticNodeData::TemplateLiteral { .. }
                    | SemanticNodeData::Signature { .. },
                ) => true,
                Some(SemanticNodeData::TypeParam { .. }) | None => false,
                Some(_) => {
                    drop(data);
                    let null = self.primitive(PrimitiveKind::Null);
                    let undefined = self.primitive(PrimitiveKind::Undefined);
                    matches!(
                        self.dispatch.execute_relate_pair(null, arm),
                        RelationStep::NotAssignable
                    ) && matches!(
                        self.dispatch.execute_relate_pair(undefined, arm),
                        RelationStep::NotAssignable
                    )
                }
            };
            if !proven {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    FlowGap::UnmodeledExpression,
                ));
                return Positional::Unmodeled;
            }
        }
        Positional::Value(self.non_nullable_operand(node))
    }
}

/// What `getTypeFacts` says one operand type may be, for the logical
/// operators' result rules.
#[derive(Debug, Clone, Copy, Default)]
struct LogicalFacts {
    truthy: bool,
    falsy: bool,
    nullish: bool,
}

impl FlowEvaluator<'_, '_> {
    /// [`SliceExpr::Logical`]: the left operand evaluates; the right one
    /// evaluates on the edge the operator selects, under the left's
    /// narrowing on that edge; the other edge carries the opposite reading;
    /// the two edges join past the expression exactly as an `if`
    /// statement's arms do. The value is the checker's result type over
    /// the operands' types (`checkBinaryLikeExpression`).
    ///
    /// A logical expression nests its left operand (`(a && b) && c`), so a
    /// chain's left spine is walked from its innermost operand outward:
    /// each nested node's right operand evaluates here, under that node's
    /// own guard, and its value takes what [`Self::eval_expr`] would have
    /// applied to it (its operator widening, then the frame's null
    /// algebra) — a long chain costs no native stack per operand.
    pub(super) fn eval_logical(
        &mut self,
        operator: SliceLogical,
        left: &SliceExpr,
        right: &SliceExpr,
        guard: &SliceGuard,
        right_reachable: Option<bool>,
    ) -> Positional<SemanticNodeId> {
        let mut spine = Vec::new();
        let mut innermost = left;
        while let SliceExpr::Logical {
            operator,
            left,
            right,
            guard,
            right_reachable,
            widen,
            ..
        } = innermost
        {
            spine.push((
                innermost,
                *operator,
                &**right,
                guard,
                *right_reachable,
                *widen,
            ));
            innermost = left;
        }
        let mut left_type = match self.eval_expr(innermost) {
            Positional::Value(node) => node,
            other => return other,
        };
        for (node, operator, right, guard, right_reachable, widen) in spine.into_iter().rev() {
            // Once the connected demand has tripped, the frame closes with
            // the budget failure whatever the rest of the chain evaluates
            // to; the operands left are not evaluated.
            if self.dispatch.connected_demand_tripped() {
                return Positional::Unmodeled;
            }
            let value =
                match self.eval_logical_step(operator, left_type, right, guard, right_reachable) {
                    Positional::Value(value) => value,
                    other => return other,
                };
            let value = if widen {
                let fresh = self.operator_fresh_values(node, value);
                super::widen_values_within(self.dispatch, value, &fresh, self.nullability)
            } else {
                value
            };
            left_type = self
                .dispatch
                .erase_nullable_members(value, self.nullability);
        }
        self.eval_logical_step(operator, left_type, right, guard, right_reachable)
    }

    /// One logical node over its evaluated left operand (see
    /// [`Self::eval_logical`]).
    fn eval_logical_step(
        &mut self,
        operator: SliceLogical,
        left_type: SemanticNodeId,
        right: &SliceExpr,
        guard: &SliceGuard,
        right_reachable: Option<bool>,
    ) -> Positional<SemanticNodeId> {
        let right_type = match right_reachable {
            // No edge reaches the right operand: the value is the left's.
            Some(false) => return Positional::Value(left_type),
            // Only the right operand's edge exists.
            Some(true) => match self.eval_operand_value(right) {
                Positional::Value(node) => node,
                other => return other,
            },
            None => {
                // The right operand's edge runs on the left's TRUE reading
                // for `&&` and `??` (for `??` the guard is the nullish
                // test), on its FALSE reading for `||`.
                let right_positive = operator != SliceLogical::Or;
                let entry_products = self.products.clone();
                let entry_writes = self.products.observe_writes();
                let narrow_mark = self.narrowing_snapshot();
                self.apply_guard_scoped(guard, right_positive);
                let outcome = self.eval_operand_value(right);
                let right_products = self.products.clone();
                self.restore_narrowings(narrow_mark.clone());
                self.restore_arm_entry(&entry_products);
                self.apply_guard_scoped(guard, !right_positive);
                let short_products = self.products.clone();
                self.restore_narrowings(narrow_mark);
                self.restore_arm_entry(&entry_products);
                self.join_arm_writes(
                    &right_products,
                    true,
                    &short_products,
                    true,
                    &entry_products,
                    &entry_writes,
                );
                match outcome {
                    Positional::Value(node) => node,
                    other => return other,
                }
            }
        };
        let result = self.logical_result(operator, left_type, right_type);
        self.decided(result)
    }

    /// A logical operand's value: an assignment operand is read as
    /// [`Self::eval_assignment_value`] reads it.
    fn eval_operand_value(&mut self, operand: &SliceExpr) -> Positional<SemanticNodeId> {
        let SliceExpr::Assignment {
            target,
            value,
            freshness,
            definition,
            span,
            ..
        } = operand
        else {
            return self.eval_expr(operand);
        };
        self.eval_assignment_value(target, value, freshness, *definition, *span)
    }

    /// The value of a value-position `=` write, applied in evaluation
    /// order: the checker's `rightType`, the right-hand side's own type
    /// (`c ? (x = "s") : 0` is `"s" | 0` and `c && (x = "s")` is
    /// `"s" | false`), never the target's assignment-reduced type. Where
    /// that value is not kept apart from the write, the position takes the
    /// typed gap.
    pub(super) fn eval_assignment_value(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
        value: &SliceExpr,
        freshness: &crate::flow_slice_content::SliceFreshness,
        definition: verter_semantic::analysis::flow::SkeletonExprSiteId,
        span: verter_semantic::analysis::flow::FrameSpan,
    ) -> Positional<SemanticNodeId> {
        match self.eval_value_assignment(target, value, freshness, definition, span) {
            (Positional::Value(_), Some(assigned)) => Positional::Value(
                self.dispatch
                    .erase_nullable_members(assigned, self.nullability),
            ),
            (Positional::Value(_), None) => {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    FlowGap::UnmodeledExpression,
                ));
                Positional::Unmodeled
            }
            (other, _) => other,
        }
    }

    /// The logical operators' result type: `&&` is the left's definitely
    /// falsy part (of the right's base type with `strictNullChecks` off)
    /// beside the right when the left may be truthy; `||` the left's truthy
    /// part beside the right when the left may be falsy; `??` the left's
    /// non-nullable part beside the right when the left may be nullish —
    /// and the left alone otherwise. `None` when an operand's facts are
    /// not decidable here.
    fn logical_result(
        &mut self,
        operator: SliceLogical,
        left: SemanticNodeId,
        right: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let facts = self.logical_facts(left)?;
        Some(match operator {
            SliceLogical::And if facts.truthy => {
                let source = if self.nullability.is_strict() {
                    left
                } else {
                    self.base_type_of_literal(right)
                };
                let falsy = self.definitely_falsy_part(source)?;
                self.union(&[falsy, right])
            }
            SliceLogical::Or if facts.falsy => {
                let truthy = self.remove_definitely_falsy(left)?;
                self.reduced_union(vec![
                    super::ReductionArm::plain(truthy),
                    super::ReductionArm::plain(right),
                ])
                .0
            }
            SliceLogical::Coalesce if facts.nullish => {
                let non_nullable = if self.nullability.is_strict() {
                    self.non_nullable_operand(left)
                } else {
                    left
                };
                self.reduced_union(vec![
                    super::ReductionArm::plain(non_nullable),
                    super::ReductionArm::plain(right),
                ])
                .0
            }
            SliceLogical::And | SliceLogical::Or | SliceLogical::Coalesce => left,
        })
    }

    /// The members of an operand with `boolean` read as `true | false`.
    fn logical_arms(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        let graph = self.dispatch.graph();
        let mut arms = Vec::new();
        for arm in self.operand_arms(node) {
            if matches!(
                graph.node_data(arm).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
            ) {
                arms.push(
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(true))),
                );
                arms.push(
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(false))),
                );
            } else {
                arms.push(arm);
            }
        }
        arms
    }

    /// One non-union member's facts (`getTypeFactsWorker`), `None` for a
    /// member whose facts this frame cannot read off its node.
    fn arm_facts(&self, arm: SemanticNodeId) -> Option<LogicalFacts> {
        let strict = self.nullability.is_strict();
        let falsy_literal = |falsy: bool| LogicalFacts {
            truthy: !falsy,
            falsy: falsy || !strict,
            nullish: !strict,
        };
        let data = self.dispatch.graph().node_data(arm);
        Some(match data.as_deref()? {
            SemanticNodeData::Primitive(primitive) => match primitive {
                PrimitiveKind::Any | PrimitiveKind::Unknown => LogicalFacts {
                    truthy: true,
                    falsy: true,
                    nullish: true,
                },
                PrimitiveKind::Null | PrimitiveKind::Undefined | PrimitiveKind::Void => {
                    LogicalFacts {
                        truthy: false,
                        falsy: true,
                        nullish: true,
                    }
                }
                PrimitiveKind::Never => LogicalFacts::default(),
                PrimitiveKind::String
                | PrimitiveKind::Number
                | PrimitiveKind::BigInt
                | PrimitiveKind::Boolean => LogicalFacts {
                    truthy: true,
                    falsy: true,
                    nullish: !strict,
                },
                _ => LogicalFacts {
                    truthy: true,
                    falsy: !strict,
                    nullish: !strict,
                },
            },
            SemanticNodeData::Literal(literal) => falsy_literal(match literal {
                LiteralValue::String(value) => value.is_empty(),
                LiteralValue::Number(value) => *value == 0.0,
                LiteralValue::BigInt(value) => value.trim_start_matches('0').is_empty(),
                LiteralValue::Boolean(value) => !*value,
            }),
            SemanticNodeData::Object(_)
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Signature { .. } => LogicalFacts {
                truthy: true,
                falsy: !strict,
                nullish: !strict,
            },
            _ => return None,
        })
    }

    fn logical_facts(&self, node: SemanticNodeId) -> Option<LogicalFacts> {
        let mut facts = LogicalFacts::default();
        for arm in self.logical_arms(node) {
            let arm = self.arm_facts(arm)?;
            facts.truthy |= arm.truthy;
            facts.falsy |= arm.falsy;
            facts.nullish |= arm.nullish;
        }
        Some(facts)
    }

    /// `extractDefinitelyFalsyTypes`: each member's definitely falsy part
    /// — `""` of `string`, `0` of `number`, `0n` of `bigint`, `false` of
    /// `boolean`, a falsy literal, `null` / `undefined` / `void`, `any` /
    /// `unknown` themselves — and nothing of any other member.
    fn definitely_falsy_part(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let graph = self.dispatch.graph();
        let mut parts = Vec::new();
        for arm in self.logical_arms(node) {
            let part = match graph.node_data(arm).as_deref()? {
                SemanticNodeData::Primitive(PrimitiveKind::String) => Some(
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String("".into()))),
                ),
                SemanticNodeData::Primitive(PrimitiveKind::Number) => {
                    Some(graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(0.0))))
                }
                SemanticNodeData::Primitive(PrimitiveKind::BigInt) => Some(
                    graph.intern_node(SemanticNodeData::Literal(LiteralValue::BigInt("0".into()))),
                ),
                SemanticNodeData::Primitive(
                    PrimitiveKind::Null
                    | PrimitiveKind::Undefined
                    | PrimitiveKind::Void
                    | PrimitiveKind::Any
                    | PrimitiveKind::Unknown,
                ) => Some(arm),
                SemanticNodeData::Literal(_) => {
                    let facts = self.arm_facts(arm)?;
                    (!facts.truthy).then_some(arm)
                }
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Object(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. } => None,
                _ => return None,
            };
            parts.extend(part);
        }
        Some(match parts.as_slice() {
            [] => self.primitive(PrimitiveKind::Never),
            [single] => *single,
            _ => self.union(&parts),
        })
    }

    /// `removeDefinitelyFalsyTypes`: the members that may be truthy. Under
    /// `strictNullChecks` `unknown` is `{} | null | undefined` to the
    /// checker, whose possibly-truthy part is `{}` (`x || 1` over `x:
    /// unknown` is `{}`).
    fn remove_definitely_falsy(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut kept = Vec::new();
        for arm in self.logical_arms(node) {
            if self.nullability.is_strict()
                && matches!(
                    self.dispatch.graph().node_data(arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
                )
            {
                kept.push(self.unknown_without(&[PrimitiveKind::Null, PrimitiveKind::Undefined]));
                continue;
            }
            if self.arm_facts(arm)?.truthy {
                kept.push(arm);
            }
        }
        Some(match kept.as_slice() {
            [] => self.primitive(PrimitiveKind::Never),
            [single] => *single,
            _ => self.union(&kept),
        })
    }
}

impl FlowEvaluator<'_, '_> {
    /// [`SliceExpr::Not`]: `true` when every member of the operand is
    /// definitely falsy, `false` when every member is definitely truthy,
    /// `boolean` otherwise (`checkPrefixUnaryExpression` over the
    /// operand's `Truthy` / `Falsy` facts).
    pub(super) fn eval_not(&mut self, operand: &SliceExpr) -> Positional<SemanticNodeId> {
        let node = match self.eval_expr(operand) {
            Positional::Value(node) => node,
            other => return other,
        };
        let Some(facts) = self.logical_facts(node) else {
            return self.decided(None);
        };
        let graph = self.dispatch.graph();
        Positional::Value(match (facts.truthy, facts.falsy) {
            (true, false) => {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(false)))
            }
            (false, true) => {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(true)))
            }
            _ => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean)),
        })
    }

    /// The FRESH literal constituents of an operator position's value
    /// `node`: a `!`'s literal result, and the literals a logical
    /// expression's bare-literal operands (nested logicals included)
    /// contribute to its result. Empty for every other position.
    pub(super) fn operator_fresh_values(
        &self,
        expr: &SliceExpr,
        node: SemanticNodeId,
    ) -> Vec<SemanticNodeId> {
        let graph = self.dispatch.graph();
        let mut fresh: Vec<SemanticNodeId> = Vec::new();
        self.collect_operator_fresh_literals(expr, &mut fresh);
        if matches!(expr, SliceExpr::Not { .. })
            && matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Literal(_))
            )
        {
            return vec![node];
        }
        if fresh.is_empty() {
            return fresh;
        }
        self.top_level_literal_nodes(node)
            .into_iter()
            .filter(|kept| {
                let data = graph.node_data(*kept);
                fresh.iter().any(|value| graph.node_data(*value) == data)
            })
            .collect()
    }

    fn collect_operator_fresh_literals(&self, expr: &SliceExpr, out: &mut Vec<SemanticNodeId>) {
        match expr {
            // A value-position `=` is its right-hand side's type, fresh
            // literals included.
            SliceExpr::Assignment {
                value, freshness, ..
            } => {
                self.collect_fresh_leaves(value, freshness, out);
                return;
            }
            // A conditional's assignment arms carry their own fresh
            // literals (its bare literal arms are the lowering's).
            SliceExpr::Union { arms, .. } => {
                for arm in arms.iter() {
                    if matches!(arm, SliceExpr::Assignment { .. }) {
                        self.collect_operator_fresh_literals(arm, out);
                    }
                }
                return;
            }
            _ => {}
        }
        // A chain nests its left operands: its left spine is walked, and the
        // operands are visited in the recursion's order — the innermost left
        // operand, then each right operand from the innermost node out.
        let mut operands: Vec<(&SliceExpr, bool)> = Vec::new();
        let mut node = expr;
        while let SliceExpr::Logical {
            left,
            right,
            fresh_operands,
            ..
        } = node
        {
            operands.push((right, fresh_operands.1));
            if matches!(left.as_ref(), SliceExpr::Logical { .. }) {
                node = left;
            } else {
                operands.push((left, fresh_operands.0));
                break;
            }
        }
        for (operand, fresh) in operands.into_iter().rev() {
            match operand {
                SliceExpr::Type(leaf) if fresh => {
                    if let verter_type_expr::TypeExpr::Literal(_) = leaf.ty() {
                        out.push(self.lower_body_type(leaf.ty()));
                    }
                }
                nested @ (SliceExpr::Logical { .. } | SliceExpr::Assignment { .. }) => {
                    self.collect_operator_fresh_literals(nested, out)
                }
                _ => {}
            }
        }
    }

    /// The fresh literal leaves of `expr` under its freshness mirror: a
    /// fresh literal leaf, each fresh arm of a conditional, and the fresh
    /// literals of a nested value-position write or logical operator.
    fn collect_fresh_leaves(
        &self,
        expr: &SliceExpr,
        freshness: &crate::flow_slice_content::SliceFreshness,
        out: &mut Vec<SemanticNodeId>,
    ) {
        use crate::flow_slice_content::SliceFreshness;
        match (expr, freshness) {
            (SliceExpr::Type(leaf), SliceFreshness::Fresh) => {
                if let verter_type_expr::TypeExpr::Literal(_) = leaf.ty() {
                    out.push(self.lower_body_type(leaf.ty()));
                }
            }
            (SliceExpr::Union { arms, .. }, SliceFreshness::PerArm(verdicts))
                if arms.len() == verdicts.len() =>
            {
                for (arm, verdict) in arms.iter().zip(verdicts.iter()) {
                    self.collect_fresh_leaves(arm, verdict, out);
                }
            }
            (nested @ (SliceExpr::Assignment { .. } | SliceExpr::Logical { .. }), _) => {
                self.collect_operator_fresh_literals(nested, out)
            }
            _ => {}
        }
    }
}
