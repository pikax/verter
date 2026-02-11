//! Span adjustment utilities for OXC AST nodes.
//!
//! These utilities allow adjusting spans in AST nodes after parsing
//! substrings, so the spans reflect positions in the original source.

use oxc_ast::ast::{
    Argument, ArrayExpression, ArrayExpressionElement, ArrayPattern, ArrowFunctionExpression,
    AssignmentTarget, AssignmentTargetMaybeDefault, AssignmentTargetProperty, AssignmentTargetRest,
    AwaitExpression, BinaryExpression, BindingPattern, BindingProperty, BindingRestElement,
    CallExpression, ChainElement, ChainExpression, ComputedMemberExpression, ConditionalExpression,
    ExportDefaultDeclarationKind, Expression, ForStatementInit, FormalParameter, FormalParameters,
    ImportExpression, LogicalExpression, MemberExpression, NewExpression, ObjectExpression,
    ObjectPattern, ObjectPropertyKind, ParenthesizedExpression, PrivateFieldExpression, Program,
    SequenceExpression, SimpleAssignmentTarget, SpreadElement, Statement, StaticMemberExpression,
    TSAsExpression, TSNonNullExpression, TSSatisfiesExpression, TSTypeAssertion,
    TaggedTemplateExpression, TemplateLiteral, UnaryExpression, UpdateExpression, YieldExpression,
};
use oxc_span::{GetSpanMut, Span};

/// Adjust a span by adding an offset to both start and end.
#[inline]
fn adjust_span(span: &mut Span, offset: u32) {
    span.start += offset;
    span.end += offset;
}

/// Adjust a span by subtracting an offset from both start and end.
#[inline]
fn adjust_span_subtract(span: &mut Span, offset: u32) {
    span.start = span.start.saturating_sub(offset);
    span.end = span.end.saturating_sub(offset);
}

/// Adjust all spans in an Expression by adding the given offset.
///
/// This recursively walks through the expression tree and adjusts
/// all span values to reflect positions in the original source string.
pub fn adjust_expression_spans(expr: &mut Expression<'_>, offset: u32) {
    if offset == 0 {
        return;
    }

    match expr {
        Expression::Identifier(id) => {
            adjust_span(&mut id.span, offset);
        }
        Expression::BooleanLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::NullLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::NumericLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::BigIntLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::StringLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::RegExpLiteral(lit) => {
            adjust_span(&mut lit.span, offset);
        }
        Expression::TemplateLiteral(tpl) => {
            adjust_template_literal_spans(tpl, offset);
        }
        Expression::ArrayExpression(arr) => {
            adjust_array_expression_spans(arr, offset);
        }
        Expression::ObjectExpression(obj) => {
            adjust_object_expression_spans(obj, offset);
        }
        Expression::ParenthesizedExpression(paren) => {
            adjust_parenthesized_expression_spans(paren, offset);
        }
        Expression::SequenceExpression(seq) => {
            adjust_sequence_expression_spans(seq, offset);
        }
        Expression::StaticMemberExpression(mem) => {
            adjust_static_member_expression_spans(mem, offset);
        }
        Expression::ComputedMemberExpression(mem) => {
            adjust_computed_member_expression_spans(mem, offset);
        }
        Expression::PrivateFieldExpression(mem) => {
            adjust_private_field_expression_spans(mem, offset);
        }
        Expression::CallExpression(call) => {
            adjust_call_expression_spans(call, offset);
        }
        Expression::NewExpression(new) => {
            adjust_new_expression_spans(new, offset);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            adjust_tagged_template_expression_spans(tagged, offset);
        }
        Expression::UnaryExpression(unary) => {
            adjust_unary_expression_spans(unary, offset);
        }
        Expression::UpdateExpression(update) => {
            adjust_update_expression_spans(update, offset);
        }
        Expression::BinaryExpression(binary) => {
            adjust_binary_expression_spans(binary, offset);
        }
        Expression::LogicalExpression(logical) => {
            adjust_logical_expression_spans(logical, offset);
        }
        Expression::ConditionalExpression(cond) => {
            adjust_conditional_expression_spans(cond, offset);
        }
        Expression::ArrowFunctionExpression(arrow) => {
            adjust_arrow_function_expression_spans(arrow, offset);
        }
        Expression::YieldExpression(yield_expr) => {
            adjust_yield_expression_spans(yield_expr, offset);
        }
        Expression::AwaitExpression(await_expr) => {
            adjust_await_expression_spans(await_expr, offset);
        }
        Expression::ChainExpression(chain) => {
            adjust_chain_expression_spans(chain, offset);
        }
        Expression::TSAsExpression(ts_as) => {
            adjust_ts_as_expression_spans(ts_as, offset);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            adjust_ts_satisfies_expression_spans(ts_sat, offset);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            adjust_ts_non_null_expression_spans(ts_non_null, offset);
        }
        Expression::TSTypeAssertion(ts_assert) => {
            adjust_ts_type_assertion_spans(ts_assert, offset);
        }
        Expression::ImportExpression(import) => {
            adjust_import_expression_spans(import, offset);
        }
        Expression::ThisExpression(this) => {
            adjust_span(&mut this.span, offset);
        }
        Expression::Super(sup) => {
            adjust_span(&mut sup.span, offset);
        }
        Expression::MetaProperty(meta) => {
            adjust_span(&mut meta.span, offset);
        }
        // FunctionExpression, ClassExpression, etc. are less common in v-for/v-slot
        // but we handle them for completeness
        Expression::FunctionExpression(func) => {
            adjust_span(&mut func.span, offset);
            // Could recurse into body, params, etc. if needed
        }
        Expression::ClassExpression(class) => {
            adjust_span(&mut class.span, offset);
        }
        Expression::AssignmentExpression(assign) => {
            adjust_span(&mut assign.span, offset);
            adjust_assignment_target_spans(&mut assign.left, offset);
            adjust_expression_spans(&mut assign.right, offset);
        }
        Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::PrivateInExpression(_)
        | Expression::TSInstantiationExpression(_)
        | Expression::V8IntrinsicExpression(_) => {
            // These are less common in Vue templates, skip for now
        }
    }
}

fn adjust_template_literal_spans(tpl: &mut TemplateLiteral<'_>, offset: u32) {
    adjust_span(&mut tpl.span, offset);
    for quasi in &mut tpl.quasis {
        adjust_span(&mut quasi.span, offset);
    }
    for expr in &mut tpl.expressions {
        adjust_expression_spans(expr, offset);
    }
}

fn adjust_array_expression_spans(arr: &mut ArrayExpression<'_>, offset: u32) {
    adjust_span(&mut arr.span, offset);
    for elem in &mut arr.elements {
        match elem {
            ArrayExpressionElement::SpreadElement(spread) => {
                adjust_spread_element_spans(spread, offset);
            }
            ArrayExpressionElement::Elision(elision) => {
                adjust_span(&mut elision.span, offset);
            }
            _ => {
                if let Some(expr) = elem.as_expression_mut() {
                    adjust_expression_spans(expr, offset);
                }
            }
        }
    }
}

fn adjust_spread_element_spans(spread: &mut SpreadElement<'_>, offset: u32) {
    adjust_span(&mut spread.span, offset);
    adjust_expression_spans(&mut spread.argument, offset);
}

fn adjust_object_expression_spans(obj: &mut ObjectExpression<'_>, offset: u32) {
    adjust_span(&mut obj.span, offset);
    for prop in &mut obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(p) => {
                adjust_span(&mut p.span, offset);
                // Key might not be convertible to expression (e.g., shorthand properties like `{ foo }`)
                if let Some(key_expr) = p.key.as_expression_mut() {
                    adjust_expression_spans(key_expr, offset);
                }
                adjust_expression_spans(&mut p.value, offset);
            }
            ObjectPropertyKind::SpreadProperty(spread) => {
                adjust_span(&mut spread.span, offset);
                adjust_expression_spans(&mut spread.argument, offset);
            }
        }
    }
}

fn adjust_parenthesized_expression_spans(paren: &mut ParenthesizedExpression<'_>, offset: u32) {
    adjust_span(&mut paren.span, offset);
    adjust_expression_spans(&mut paren.expression, offset);
}

fn adjust_sequence_expression_spans(seq: &mut SequenceExpression<'_>, offset: u32) {
    adjust_span(&mut seq.span, offset);
    for expr in &mut seq.expressions {
        adjust_expression_spans(expr, offset);
    }
}

fn adjust_static_member_expression_spans(mem: &mut StaticMemberExpression<'_>, offset: u32) {
    adjust_span(&mut mem.span, offset);
    adjust_expression_spans(&mut mem.object, offset);
    adjust_span(&mut mem.property.span, offset);
}

fn adjust_computed_member_expression_spans(mem: &mut ComputedMemberExpression<'_>, offset: u32) {
    adjust_span(&mut mem.span, offset);
    adjust_expression_spans(&mut mem.object, offset);
    adjust_expression_spans(&mut mem.expression, offset);
}

fn adjust_private_field_expression_spans(mem: &mut PrivateFieldExpression<'_>, offset: u32) {
    adjust_span(&mut mem.span, offset);
    adjust_expression_spans(&mut mem.object, offset);
    adjust_span(&mut mem.field.span, offset);
}

fn adjust_call_expression_spans(call: &mut CallExpression<'_>, offset: u32) {
    adjust_span(&mut call.span, offset);
    adjust_expression_spans(&mut call.callee, offset);
    for arg in &mut call.arguments {
        adjust_argument_spans(arg, offset);
    }
}

fn adjust_argument_spans(arg: &mut Argument<'_>, offset: u32) {
    match arg {
        Argument::SpreadElement(spread) => {
            adjust_spread_element_spans(spread, offset);
        }
        _ => {
            if let Some(expr) = arg.as_expression_mut() {
                adjust_expression_spans(expr, offset);
            }
        }
    }
}

fn adjust_new_expression_spans(new: &mut NewExpression<'_>, offset: u32) {
    adjust_span(&mut new.span, offset);
    adjust_expression_spans(&mut new.callee, offset);
    for arg in &mut new.arguments {
        adjust_argument_spans(arg, offset);
    }
}

fn adjust_tagged_template_expression_spans(tagged: &mut TaggedTemplateExpression<'_>, offset: u32) {
    adjust_span(&mut tagged.span, offset);
    adjust_expression_spans(&mut tagged.tag, offset);
    adjust_template_literal_spans(&mut tagged.quasi, offset);
}

fn adjust_unary_expression_spans(unary: &mut UnaryExpression<'_>, offset: u32) {
    adjust_span(&mut unary.span, offset);
    adjust_expression_spans(&mut unary.argument, offset);
}

fn adjust_update_expression_spans(update: &mut UpdateExpression<'_>, offset: u32) {
    adjust_span(&mut update.span, offset);
    match &mut update.argument {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
            adjust_span(&mut id.span, offset);
        }
        SimpleAssignmentTarget::TSAsExpression(ts_as) => {
            adjust_ts_as_expression_spans(ts_as, offset);
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(ts_sat) => {
            adjust_ts_satisfies_expression_spans(ts_sat, offset);
        }
        SimpleAssignmentTarget::TSNonNullExpression(ts_non_null) => {
            adjust_ts_non_null_expression_spans(ts_non_null, offset);
        }
        SimpleAssignmentTarget::TSTypeAssertion(ts_assert) => {
            adjust_ts_type_assertion_spans(ts_assert, offset);
        }
        _ => {
            if let Some(mem) = update.argument.as_member_expression_mut() {
                adjust_member_expression_spans(mem, offset);
            }
        }
    }
}

fn adjust_member_expression_spans(mem: &mut MemberExpression<'_>, offset: u32) {
    match mem {
        MemberExpression::ComputedMemberExpression(computed) => {
            adjust_computed_member_expression_spans(computed, offset);
        }
        MemberExpression::StaticMemberExpression(static_mem) => {
            adjust_static_member_expression_spans(static_mem, offset);
        }
        MemberExpression::PrivateFieldExpression(private) => {
            adjust_private_field_expression_spans(private, offset);
        }
    }
}

fn adjust_binary_expression_spans(binary: &mut BinaryExpression<'_>, offset: u32) {
    adjust_span(&mut binary.span, offset);
    adjust_expression_spans(&mut binary.left, offset);
    adjust_expression_spans(&mut binary.right, offset);
}

fn adjust_logical_expression_spans(logical: &mut LogicalExpression<'_>, offset: u32) {
    adjust_span(&mut logical.span, offset);
    adjust_expression_spans(&mut logical.left, offset);
    adjust_expression_spans(&mut logical.right, offset);
}

fn adjust_conditional_expression_spans(cond: &mut ConditionalExpression<'_>, offset: u32) {
    adjust_span(&mut cond.span, offset);
    adjust_expression_spans(&mut cond.test, offset);
    adjust_expression_spans(&mut cond.consequent, offset);
    adjust_expression_spans(&mut cond.alternate, offset);
}

fn adjust_arrow_function_expression_spans(arrow: &mut ArrowFunctionExpression<'_>, offset: u32) {
    adjust_span(&mut arrow.span, offset);
    adjust_formal_parameters_spans(&mut arrow.params, offset);
    // Body could be expression or function body - handle expression case
    if arrow.expression {
        if let Some(oxc_ast::ast::Statement::ExpressionStatement(stmt)) =
            arrow.body.statements.first_mut()
        {
            adjust_expression_spans(&mut stmt.expression, offset);
        }
    }
}

fn adjust_yield_expression_spans(yield_expr: &mut YieldExpression<'_>, offset: u32) {
    adjust_span(&mut yield_expr.span, offset);
    if let Some(arg) = &mut yield_expr.argument {
        adjust_expression_spans(arg, offset);
    }
}

fn adjust_await_expression_spans(await_expr: &mut AwaitExpression<'_>, offset: u32) {
    adjust_span(&mut await_expr.span, offset);
    adjust_expression_spans(&mut await_expr.argument, offset);
}

fn adjust_chain_expression_spans(chain: &mut ChainExpression<'_>, offset: u32) {
    adjust_span(&mut chain.span, offset);
    match &mut chain.expression {
        ChainElement::CallExpression(call) => {
            adjust_call_expression_spans(call, offset);
        }
        ChainElement::TSNonNullExpression(ts_non_null) => {
            adjust_ts_non_null_expression_spans(ts_non_null, offset);
        }
        _ => {
            if let Some(mem) = chain.expression.as_member_expression_mut() {
                adjust_member_expression_spans(mem, offset);
            }
        }
    }
}

fn adjust_ts_as_expression_spans(ts_as: &mut TSAsExpression<'_>, offset: u32) {
    adjust_span(&mut ts_as.span, offset);
    adjust_expression_spans(&mut ts_as.expression, offset);
    // Type annotation spans could also be adjusted if needed
}

fn adjust_ts_satisfies_expression_spans(ts_sat: &mut TSSatisfiesExpression<'_>, offset: u32) {
    adjust_span(&mut ts_sat.span, offset);
    adjust_expression_spans(&mut ts_sat.expression, offset);
}

fn adjust_ts_non_null_expression_spans(ts_non_null: &mut TSNonNullExpression<'_>, offset: u32) {
    adjust_span(&mut ts_non_null.span, offset);
    adjust_expression_spans(&mut ts_non_null.expression, offset);
}

fn adjust_ts_type_assertion_spans(ts_assert: &mut TSTypeAssertion<'_>, offset: u32) {
    adjust_span(&mut ts_assert.span, offset);
    adjust_expression_spans(&mut ts_assert.expression, offset);
}

fn adjust_import_expression_spans(import: &mut ImportExpression<'_>, offset: u32) {
    adjust_span(&mut import.span, offset);
    adjust_expression_spans(&mut import.source, offset);
    if let Some(options) = &mut import.options {
        adjust_expression_spans(options, offset);
    }
}

fn adjust_assignment_target_spans(target: &mut AssignmentTarget<'_>, offset: u32) {
    match target {
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            adjust_span(&mut arr.span, offset);
            for elem in arr.elements.iter_mut().flatten() {
                adjust_assignment_target_maybe_default_spans(elem, offset);
            }
            if let Some(rest) = &mut arr.rest {
                adjust_assignment_target_rest_spans(rest, offset);
            }
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            adjust_span(&mut obj.span, offset);
            for prop in &mut obj.properties {
                adjust_assignment_target_property_spans(prop, offset);
            }
            if let Some(rest) = &mut obj.rest {
                adjust_assignment_target_rest_spans(rest, offset);
            }
        }
        _ => {
            if let Some(simple) = target.as_simple_assignment_target_mut() {
                adjust_simple_assignment_target_spans(simple, offset);
            }
        }
    }
}

fn adjust_assignment_target_rest_spans(rest: &mut AssignmentTargetRest<'_>, offset: u32) {
    adjust_span(&mut rest.span, offset);
    adjust_assignment_target_spans(&mut rest.target, offset);
}

fn adjust_assignment_target_maybe_default_spans(
    target: &mut AssignmentTargetMaybeDefault<'_>,
    offset: u32,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            adjust_span(&mut with_default.span, offset);
            adjust_assignment_target_spans(&mut with_default.binding, offset);
            adjust_expression_spans(&mut with_default.init, offset);
        }
        _ => {
            if let Some(simple) = target.as_assignment_target_mut() {
                adjust_assignment_target_spans(simple, offset);
            }
        }
    }
}

fn adjust_assignment_target_property_spans(prop: &mut AssignmentTargetProperty<'_>, offset: u32) {
    match prop {
        AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
            adjust_span(&mut id.span, offset);
            adjust_span(&mut id.binding.span, offset);
            if let Some(init) = &mut id.init {
                adjust_expression_spans(init, offset);
            }
        }
        AssignmentTargetProperty::AssignmentTargetPropertyProperty(p) => {
            adjust_span(&mut p.span, offset);
            // Name might not be convertible to expression (e.g., shorthand properties)
            if let Some(name_expr) = p.name.as_expression_mut() {
                adjust_expression_spans(name_expr, offset);
            }
            adjust_assignment_target_maybe_default_spans(&mut p.binding, offset);
        }
    }
}

fn adjust_simple_assignment_target_spans(target: &mut SimpleAssignmentTarget<'_>, offset: u32) {
    match target {
        SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
            adjust_span(&mut id.span, offset);
        }
        SimpleAssignmentTarget::TSAsExpression(ts_as) => {
            adjust_ts_as_expression_spans(ts_as, offset);
        }
        SimpleAssignmentTarget::TSSatisfiesExpression(ts_sat) => {
            adjust_ts_satisfies_expression_spans(ts_sat, offset);
        }
        SimpleAssignmentTarget::TSNonNullExpression(ts_non_null) => {
            adjust_ts_non_null_expression_spans(ts_non_null, offset);
        }
        SimpleAssignmentTarget::TSTypeAssertion(ts_assert) => {
            adjust_ts_type_assertion_spans(ts_assert, offset);
        }
        _ => {
            if let Some(mem) = target.as_member_expression_mut() {
                adjust_member_expression_spans(mem, offset);
            }
        }
    }
}

/// Adjust all spans in FormalParameters by adding the given offset.
pub fn adjust_formal_parameters_spans(params: &mut FormalParameters<'_>, offset: u32) {
    if offset == 0 {
        return;
    }

    adjust_span(&mut params.span, offset);
    for param in &mut params.items {
        adjust_formal_parameter_spans(param, offset);
    }
    if let Some(rest) = &mut params.rest {
        adjust_span(&mut rest.span, offset);
        adjust_binding_pattern_spans(&mut rest.rest.argument, offset);
    }
}

fn adjust_formal_parameter_spans(param: &mut FormalParameter<'_>, offset: u32) {
    adjust_span(&mut param.span, offset);
    adjust_binding_pattern_spans(&mut param.pattern, offset);
    if let Some(init) = &mut param.initializer {
        adjust_expression_spans(init, offset);
    }
    // Type annotation spans could be adjusted if needed
}

fn adjust_binding_rest_element_spans(rest: &mut BindingRestElement<'_>, offset: u32) {
    adjust_span(&mut rest.span, offset);
    adjust_binding_pattern_spans(&mut rest.argument, offset);
}

fn adjust_binding_pattern_spans(pattern: &mut BindingPattern<'_>, offset: u32) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            adjust_span(&mut id.span, offset);
        }
        BindingPattern::ObjectPattern(obj) => {
            adjust_object_pattern_spans(obj, offset);
        }
        BindingPattern::ArrayPattern(arr) => {
            adjust_array_pattern_spans(arr, offset);
        }
        BindingPattern::AssignmentPattern(assign) => {
            adjust_span(&mut assign.span, offset);
            adjust_binding_pattern_spans(&mut assign.left, offset);
            adjust_expression_spans(&mut assign.right, offset);
        }
    }
    // Type annotation spans could be adjusted if needed
}

fn adjust_object_pattern_spans(obj: &mut ObjectPattern<'_>, offset: u32) {
    adjust_span(&mut obj.span, offset);
    for prop in &mut obj.properties {
        adjust_binding_property_spans(prop, offset);
    }
    if let Some(rest) = &mut obj.rest {
        adjust_binding_rest_element_spans(rest, offset);
    }
}

fn adjust_binding_property_spans(prop: &mut BindingProperty<'_>, offset: u32) {
    adjust_span(&mut prop.span, offset);
    // Key might not be convertible to expression (e.g., shorthand properties)
    if let Some(key_expr) = prop.key.as_expression_mut() {
        adjust_expression_spans(key_expr, offset);
    }
    adjust_binding_pattern_spans(&mut prop.value, offset);
}

fn adjust_array_pattern_spans(arr: &mut ArrayPattern<'_>, offset: u32) {
    adjust_span(&mut arr.span, offset);
    for elem in arr.elements.iter_mut().flatten() {
        adjust_binding_pattern_spans(elem, offset);
    }
    if let Some(rest) = &mut arr.rest {
        adjust_binding_rest_element_spans(rest, offset);
    }
}

// =============================================================================
// Subtraction variants for unwrapping parsed content
// =============================================================================

/// Subtract offset from all spans in FormalParameters.
///
/// This is used when parsing wrapped content (e.g., `(content)=>{}`) to
/// adjust spans back to the original content positions.
pub fn subtract_formal_parameters_spans(params: &mut FormalParameters<'_>, offset: u32) {
    if offset == 0 {
        return;
    }

    adjust_span_subtract(&mut params.span, offset);
    for param in &mut params.items {
        subtract_formal_parameter_spans(param, offset);
    }
    if let Some(rest) = &mut params.rest {
        adjust_span_subtract(&mut rest.span, offset);
        subtract_binding_pattern_spans(&mut rest.rest.argument, offset);
    }
}

fn subtract_formal_parameter_spans(param: &mut FormalParameter<'_>, offset: u32) {
    adjust_span_subtract(&mut param.span, offset);
    subtract_binding_pattern_spans(&mut param.pattern, offset);
    if let Some(init) = &mut param.initializer {
        subtract_expression_spans(init, offset);
    }
}

fn subtract_binding_pattern_spans(pattern: &mut BindingPattern<'_>, offset: u32) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            adjust_span_subtract(&mut id.span, offset);
        }
        BindingPattern::ObjectPattern(obj) => {
            subtract_object_pattern_spans(obj, offset);
        }
        BindingPattern::ArrayPattern(arr) => {
            subtract_array_pattern_spans(arr, offset);
        }
        BindingPattern::AssignmentPattern(assign) => {
            adjust_span_subtract(&mut assign.span, offset);
            subtract_binding_pattern_spans(&mut assign.left, offset);
            subtract_expression_spans(&mut assign.right, offset);
        }
    }
}

fn subtract_object_pattern_spans(obj: &mut ObjectPattern<'_>, offset: u32) {
    adjust_span_subtract(&mut obj.span, offset);
    for prop in &mut obj.properties {
        subtract_binding_property_spans(prop, offset);
    }
    if let Some(rest) = &mut obj.rest {
        subtract_binding_rest_element_spans(rest, offset);
    }
}

fn subtract_binding_property_spans(prop: &mut BindingProperty<'_>, offset: u32) {
    adjust_span_subtract(&mut prop.span, offset);
    // Key might not be convertible to expression (e.g., shorthand properties)
    if let Some(key_expr) = prop.key.as_expression_mut() {
        subtract_expression_spans(key_expr, offset);
    }
    subtract_binding_pattern_spans(&mut prop.value, offset);
}

fn subtract_array_pattern_spans(arr: &mut ArrayPattern<'_>, offset: u32) {
    adjust_span_subtract(&mut arr.span, offset);
    for elem in arr.elements.iter_mut().flatten() {
        subtract_binding_pattern_spans(elem, offset);
    }
    if let Some(rest) = &mut arr.rest {
        subtract_binding_rest_element_spans(rest, offset);
    }
}

fn subtract_binding_rest_element_spans(rest: &mut BindingRestElement<'_>, offset: u32) {
    adjust_span_subtract(&mut rest.span, offset);
    subtract_binding_pattern_spans(&mut rest.argument, offset);
}

/// Subtract offset from all spans in an Expression.
fn subtract_expression_spans(expr: &mut Expression<'_>, offset: u32) {
    if offset == 0 {
        return;
    }

    match expr {
        Expression::Identifier(id) => {
            adjust_span_subtract(&mut id.span, offset);
        }
        Expression::BooleanLiteral(lit) => {
            adjust_span_subtract(&mut lit.span, offset);
        }
        Expression::NullLiteral(lit) => {
            adjust_span_subtract(&mut lit.span, offset);
        }
        Expression::NumericLiteral(lit) => {
            adjust_span_subtract(&mut lit.span, offset);
        }
        Expression::StringLiteral(lit) => {
            adjust_span_subtract(&mut lit.span, offset);
        }
        Expression::CallExpression(call) => {
            adjust_span_subtract(&mut call.span, offset);
            subtract_expression_spans(&mut call.callee, offset);
            for arg in &mut call.arguments {
                if let Some(expr) = arg.as_expression_mut() {
                    subtract_expression_spans(expr, offset);
                }
            }
        }
        Expression::StaticMemberExpression(mem) => {
            adjust_span_subtract(&mut mem.span, offset);
            subtract_expression_spans(&mut mem.object, offset);
            adjust_span_subtract(&mut mem.property.span, offset);
        }
        Expression::ComputedMemberExpression(mem) => {
            adjust_span_subtract(&mut mem.span, offset);
            subtract_expression_spans(&mut mem.object, offset);
            subtract_expression_spans(&mut mem.expression, offset);
        }
        Expression::ArrayExpression(arr) => {
            adjust_span_subtract(&mut arr.span, offset);
            for elem in &mut arr.elements {
                if let Some(expr) = elem.as_expression_mut() {
                    subtract_expression_spans(expr, offset);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            adjust_span_subtract(&mut obj.span, offset);
            for prop in &mut obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    adjust_span_subtract(&mut p.span, offset);
                    // Key might not be convertible to expression (e.g., shorthand properties like `{ foo }`)
                    if let Some(key_expr) = p.key.as_expression_mut() {
                        subtract_expression_spans(key_expr, offset);
                    }
                    subtract_expression_spans(&mut p.value, offset);
                }
            }
        }
        _ => {
            // For other expression types in default values, just adjust the span
            // This is a simplified version - extend as needed
        }
    }
}

// =============================================================================
// Program / Statement span adjustment
// =============================================================================

/// Adjust all spans in a Program by adding the given offset.
///
/// This walks the entire AST (statements, declarations, expressions) and
/// offsets every span so they reflect positions in the original source file
/// rather than the parsed substring.
pub fn adjust_program_spans(program: &mut Program<'_>, offset: u32) {
    if offset == 0 {
        return;
    }

    adjust_span(&mut program.span, offset);
    for stmt in &mut program.body {
        adjust_statement_spans(stmt, offset);
    }
}

fn adjust_statement_spans(stmt: &mut Statement<'_>, offset: u32) {
    match stmt {
        // ---- Core statements ----
        Statement::ExpressionStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.expression, offset);
        }
        Statement::BlockStatement(block) => {
            adjust_span(&mut block.span, offset);
            for s in &mut block.body {
                adjust_statement_spans(s, offset);
            }
        }
        Statement::ReturnStatement(s) => {
            adjust_span(&mut s.span, offset);
            if let Some(arg) = &mut s.argument {
                adjust_expression_spans(arg, offset);
            }
        }
        Statement::IfStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.test, offset);
            adjust_statement_spans(&mut s.consequent, offset);
            if let Some(alt) = &mut s.alternate {
                adjust_statement_spans(alt, offset);
            }
        }
        Statement::ForStatement(s) => {
            adjust_span(&mut s.span, offset);
            if let Some(init) = &mut s.init {
                adjust_for_statement_init_spans(init, offset);
            }
            if let Some(test) = &mut s.test {
                adjust_expression_spans(test, offset);
            }
            if let Some(update) = &mut s.update {
                adjust_expression_spans(update, offset);
            }
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::ForInStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_for_statement_left_spans(&mut s.left, offset);
            adjust_expression_spans(&mut s.right, offset);
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::ForOfStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_for_statement_left_spans(&mut s.left, offset);
            adjust_expression_spans(&mut s.right, offset);
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::WhileStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.test, offset);
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::DoWhileStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_statement_spans(&mut s.body, offset);
            adjust_expression_spans(&mut s.test, offset);
        }
        Statement::SwitchStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.discriminant, offset);
            for case in &mut s.cases {
                adjust_span(&mut case.span, offset);
                if let Some(test) = &mut case.test {
                    adjust_expression_spans(test, offset);
                }
                for stmt in &mut case.consequent {
                    adjust_statement_spans(stmt, offset);
                }
            }
        }
        Statement::TryStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_span(&mut s.block.span, offset);
            for stmt in &mut s.block.body {
                adjust_statement_spans(stmt, offset);
            }
            if let Some(handler) = &mut s.handler {
                adjust_span(&mut handler.span, offset);
                if let Some(param) = &mut handler.param {
                    adjust_binding_pattern_spans(&mut param.pattern, offset);
                }
                adjust_span(&mut handler.body.span, offset);
                for stmt in &mut handler.body.body {
                    adjust_statement_spans(stmt, offset);
                }
            }
            if let Some(finalizer) = &mut s.finalizer {
                adjust_span(&mut finalizer.span, offset);
                for stmt in &mut finalizer.body {
                    adjust_statement_spans(stmt, offset);
                }
            }
        }
        Statement::ThrowStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.argument, offset);
        }
        Statement::LabeledStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_span(&mut s.label.span, offset);
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::BreakStatement(s) => {
            adjust_span(&mut s.span, offset);
            if let Some(label) = &mut s.label {
                adjust_span(&mut label.span, offset);
            }
        }
        Statement::ContinueStatement(s) => {
            adjust_span(&mut s.span, offset);
            if let Some(label) = &mut s.label {
                adjust_span(&mut label.span, offset);
            }
        }
        Statement::EmptyStatement(s) => {
            adjust_span(&mut s.span, offset);
        }
        Statement::WithStatement(s) => {
            adjust_span(&mut s.span, offset);
            adjust_expression_spans(&mut s.object, offset);
            adjust_statement_spans(&mut s.body, offset);
        }
        Statement::DebuggerStatement(s) => {
            adjust_span(&mut s.span, offset);
        }

        // ---- Declarations ----
        Statement::VariableDeclaration(decl) => {
            adjust_variable_declaration_spans(decl, offset);
        }
        Statement::FunctionDeclaration(func) => {
            adjust_function_spans(func, offset);
        }
        Statement::ClassDeclaration(class) => {
            adjust_class_spans(class, offset);
        }

        // ---- Module declarations ----
        Statement::ImportDeclaration(decl) => {
            adjust_span(&mut decl.span, offset);
            adjust_span(&mut decl.source.span, offset);
            if let Some(specifiers) = &mut decl.specifiers {
                for spec in specifiers {
                    match spec {
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            adjust_span(&mut s.span, offset);
                            adjust_span(s.imported.span_mut(), offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            adjust_span(&mut s.span, offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                        oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            adjust_span(&mut s.span, offset);
                            adjust_span(&mut s.local.span, offset);
                        }
                    }
                }
            }
            if let Some(with_clause) = &mut decl.with_clause {
                adjust_span(&mut with_clause.span, offset);
            }
        }
        Statement::ExportAllDeclaration(decl) => {
            adjust_span(&mut decl.span, offset);
            adjust_span(&mut decl.source.span, offset);
            if let Some(exported) = &mut decl.exported {
                adjust_span(exported.span_mut(), offset);
            }
        }
        Statement::ExportDefaultDeclaration(decl) => {
            adjust_span(&mut decl.span, offset);
            match &mut decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    adjust_function_spans(func, offset);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    adjust_class_spans(class, offset);
                }
                _ => {
                    if let Some(expr) = decl.declaration.as_expression_mut() {
                        adjust_expression_spans(expr, offset);
                    }
                }
            }
        }
        Statement::ExportNamedDeclaration(decl) => {
            adjust_span(&mut decl.span, offset);
            if let Some(d) = &mut decl.declaration {
                adjust_declaration_spans(d, offset);
            }
            for spec in &mut decl.specifiers {
                adjust_span(&mut spec.span, offset);
                adjust_span(spec.local.span_mut(), offset);
                adjust_span(spec.exported.span_mut(), offset);
            }
            if let Some(source) = &mut decl.source {
                adjust_span(&mut source.span, offset);
            }
        }

        // ---- TypeScript declarations ----
        Statement::TSTypeAliasDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        Statement::TSInterfaceDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        Statement::TSEnumDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
            for member in &mut d.body.members {
                adjust_span(&mut member.span, offset);
                if let Some(init) = &mut member.initializer {
                    adjust_expression_spans(init, offset);
                }
            }
        }
        Statement::TSModuleDeclaration(d) => {
            adjust_span(&mut d.span, offset);
        }
        Statement::TSImportEqualsDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        Statement::TSExportAssignment(d) => {
            adjust_span(&mut d.span, offset);
            adjust_expression_spans(&mut d.expression, offset);
        }
        Statement::TSNamespaceExportDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        Statement::TSGlobalDeclaration(d) => {
            adjust_span(&mut d.span, offset);
        }
    }
}

fn adjust_for_statement_init_spans(init: &mut ForStatementInit<'_>, offset: u32) {
    match init {
        ForStatementInit::VariableDeclaration(decl) => {
            adjust_variable_declaration_spans(decl, offset);
        }
        _ => {
            if let Some(expr) = init.as_expression_mut() {
                adjust_expression_spans(expr, offset);
            }
        }
    }
}

fn adjust_for_statement_left_spans(
    left: &mut oxc_ast::ast::ForStatementLeft<'_>,
    offset: u32,
) {
    match left {
        oxc_ast::ast::ForStatementLeft::VariableDeclaration(decl) => {
            adjust_variable_declaration_spans(decl, offset);
        }
        _ => {
            if let Some(target) = left.as_assignment_target_mut() {
                adjust_assignment_target_spans(target, offset);
            }
        }
    }
}

fn adjust_variable_declaration_spans(
    decl: &mut oxc_ast::ast::VariableDeclaration<'_>,
    offset: u32,
) {
    adjust_span(&mut decl.span, offset);
    for declarator in &mut decl.declarations {
        adjust_span(&mut declarator.span, offset);
        adjust_binding_pattern_spans(&mut declarator.id, offset);
        if let Some(init) = &mut declarator.init {
            adjust_expression_spans(init, offset);
        }
    }
}

fn adjust_function_spans(func: &mut oxc_ast::ast::Function<'_>, offset: u32) {
    adjust_span(&mut func.span, offset);
    if let Some(id) = &mut func.id {
        adjust_span(&mut id.span, offset);
    }
    adjust_formal_parameters_spans(&mut func.params, offset);
    if let Some(body) = &mut func.body {
        adjust_span(&mut body.span, offset);
        for stmt in &mut body.statements {
            adjust_statement_spans(stmt, offset);
        }
    }
}

fn adjust_class_spans(class: &mut oxc_ast::ast::Class<'_>, offset: u32) {
    adjust_span(&mut class.span, offset);
    if let Some(id) = &mut class.id {
        adjust_span(&mut id.span, offset);
    }
    if let Some(super_class) = &mut class.super_class {
        adjust_expression_spans(super_class, offset);
    }
    adjust_span(&mut class.body.span, offset);
    for element in &mut class.body.body {
        match element {
            oxc_ast::ast::ClassElement::MethodDefinition(method) => {
                adjust_span(&mut method.span, offset);
                if let Some(key_expr) = method.key.as_expression_mut() {
                    adjust_expression_spans(key_expr, offset);
                }
                adjust_function_spans(&mut method.value, offset);
            }
            oxc_ast::ast::ClassElement::PropertyDefinition(prop) => {
                adjust_span(&mut prop.span, offset);
                if let Some(key_expr) = prop.key.as_expression_mut() {
                    adjust_expression_spans(key_expr, offset);
                }
                if let Some(value) = &mut prop.value {
                    adjust_expression_spans(value, offset);
                }
            }
            oxc_ast::ast::ClassElement::StaticBlock(block) => {
                adjust_span(&mut block.span, offset);
                for stmt in &mut block.body {
                    adjust_statement_spans(stmt, offset);
                }
            }
            oxc_ast::ast::ClassElement::AccessorProperty(prop) => {
                adjust_span(&mut prop.span, offset);
                if let Some(key_expr) = prop.key.as_expression_mut() {
                    adjust_expression_spans(key_expr, offset);
                }
                if let Some(value) = &mut prop.value {
                    adjust_expression_spans(value, offset);
                }
            }
            oxc_ast::ast::ClassElement::TSIndexSignature(sig) => {
                adjust_span(&mut sig.span, offset);
            }
        }
    }
}

fn adjust_declaration_spans(decl: &mut oxc_ast::ast::Declaration<'_>, offset: u32) {
    match decl {
        oxc_ast::ast::Declaration::VariableDeclaration(d) => {
            adjust_variable_declaration_spans(d, offset);
        }
        oxc_ast::ast::Declaration::FunctionDeclaration(f) => {
            adjust_function_spans(f, offset);
        }
        oxc_ast::ast::Declaration::ClassDeclaration(c) => {
            adjust_class_spans(c, offset);
        }
        oxc_ast::ast::Declaration::TSTypeAliasDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        oxc_ast::ast::Declaration::TSInterfaceDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
        oxc_ast::ast::Declaration::TSEnumDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
            for member in &mut d.body.members {
                adjust_span(&mut member.span, offset);
                if let Some(init) = &mut member.initializer {
                    adjust_expression_spans(init, offset);
                }
            }
        }
        oxc_ast::ast::Declaration::TSModuleDeclaration(d) => {
            adjust_span(&mut d.span, offset);
        }
        oxc_ast::ast::Declaration::TSGlobalDeclaration(d) => {
            adjust_span(&mut d.span, offset);
        }
        oxc_ast::ast::Declaration::TSImportEqualsDeclaration(d) => {
            adjust_span(&mut d.span, offset);
            adjust_span(&mut d.id.span, offset);
        }
    }
}

// =============================================================================
// Diagnostic span adjustment
// =============================================================================

/// Adjust all label spans in a list of diagnostics by adding the given offset.
///
/// OXC parser errors contain `LabeledSpan`s with offsets relative to the parsed
/// substring. This function shifts them so they reference positions in the
/// original source file.
pub fn adjust_diagnostics_spans(errors: &mut [oxc_diagnostics::OxcDiagnostic], offset: u32) {
    if offset == 0 {
        return;
    }
    let offset_usize = offset as usize;
    for diag in errors.iter_mut() {
        if let Some(labels) = &mut diag.labels {
            for label in labels.iter_mut() {
                *label = oxc_diagnostics::LabeledSpan::new(
                    label.label().map(|s| s.to_string()),
                    label.offset() + offset_usize,
                    label.len(),
                );
            }
        }
    }
}
