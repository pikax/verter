//! Helper functions for binding extraction.
//!
//! This module contains shared helper functions used by the slot, v-for,
//! and expression extraction modules.

use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::keywords::is_keyword;
use crate::common::Span;

/// Collect local binding names from a binding pattern.
///
/// This extracts all identifiers declared by the pattern itself.
/// For example, in `{ a, b: c }`, this returns `["a", "c"]`.
pub fn collect_pattern_locals<'a>(pattern: &'a BindingPattern<'a>, locals: &mut Vec<&'a str>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            locals.push(ident.name.as_str());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_locals(&prop.value, locals);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_locals(&rest.argument, locals);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_locals(elem, locals);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_locals(&rest.argument, locals);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_locals(&assign.left, locals);
        }
    }
}

/// Collect references from a binding pattern (default values).
///
/// This extracts identifiers that are referenced in default value expressions.
pub fn collect_pattern_references<'a>(
    pattern: &'a BindingPattern<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match pattern {
        BindingPattern::AssignmentPattern(assign) => {
            collect_expression_references(&assign.right, ignored, references);
            collect_pattern_references(&assign.left, ignored, references);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_references(&prop.value, ignored, references);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_references(&rest.argument, ignored, references);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_references(elem, ignored, references);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_references(&rest.argument, ignored, references);
            }
        }
        BindingPattern::BindingIdentifier(_) => {}
    }
}

/// Collect identifier references from an expression (excluding ignored identifiers).
pub fn collect_expression_references<'a>(
    expr: &'a Expression<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match expr {
        Expression::Identifier(ident) => {
            let name_bytes = ident.name.as_bytes();
            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) {
                references.insert(ident.name.as_str());
            }
        }
        Expression::BinaryExpression(binary) => {
            collect_expression_references(&binary.left, ignored, references);
            collect_expression_references(&binary.right, ignored, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_expression_references(&logical.left, ignored, references);
            collect_expression_references(&logical.right, ignored, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_expression_references(&cond.test, ignored, references);
            collect_expression_references(&cond.consequent, ignored, references);
            collect_expression_references(&cond.alternate, ignored, references);
        }
        Expression::CallExpression(call) => {
            collect_expression_references(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
            collect_expression_references(&member.expression, ignored, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(expr) = elem.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    if p.shorthand {
                        if let PropertyKey::StaticIdentifier(ident) = &p.key {
                            let name_bytes = ident.name.as_bytes();
                            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) {
                                references.insert(ident.name.as_str());
                            }
                        }
                    } else {
                        collect_expression_references(&p.value, ignored, references);
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expression_references(&paren.expression, ignored, references);
        }
        Expression::UnaryExpression(unary) => {
            collect_expression_references(&unary.argument, ignored, references);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_expression_references(&ts_as.expression, ignored, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_expression_references(&non_null.expression, ignored, references);
        }
        Expression::ChainExpression(chain) => {
            collect_chain_element_references(&chain.expression, ignored, references);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_expression_references(&await_expr.argument, ignored, references);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_expression_references(&assign.right, ignored, references);
        }
        Expression::NewExpression(new_expr) => {
            collect_expression_references(&new_expr.callee, ignored, references);
            for arg in &new_expr.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_expression_references(&tagged.tag, ignored, references);
            for expr in &tagged.quasi.expressions {
                collect_expression_references(expr, ignored, references);
            }
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(arg) = &yield_expr.argument {
                collect_expression_references(arg, ignored, references);
            }
        }
        _ => {}
    }
}

/// Collect references from chain elements (optional chaining).
pub fn collect_chain_element_references<'a>(
    element: &'a ChainElement<'a>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<&'a str>,
) {
    match element {
        ChainElement::CallExpression(call) => {
            collect_expression_references(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_references(expr, ignored, references);
                }
            }
        }
        ChainElement::StaticMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
        }
        ChainElement::ComputedMemberExpression(member) => {
            collect_expression_references(&member.object, ignored, references);
            collect_expression_references(&member.expression, ignored, references);
        }
        ChainElement::PrivateFieldExpression(field) => {
            collect_expression_references(&field.object, ignored, references);
        }
        _ => {}
    }
}

/// Collect type references from TypeScript type annotations.
pub fn collect_type_references<'a>(ts_type: &'a TSType<'a>, references: &mut FxHashSet<&'a str>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(ident) = &type_ref.type_name {
                let name_bytes = ident.name.as_bytes();
                if !is_keyword(name_bytes) {
                    references.insert(ident.name.as_str());
                }
            }
            // Also check generic type arguments
            if let Some(args) = &type_ref.type_arguments {
                for arg in &args.params {
                    collect_type_references(arg, references);
                }
            }
        }
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(annotation) = &prop.type_annotation {
                            collect_type_references(&annotation.type_annotation, references);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(annotation) = &method.return_type {
                            collect_type_references(&annotation.type_annotation, references);
                        }
                    }
                    _ => {}
                }
            }
        }
        TSType::TSUnionType(union) => {
            for t in &union.types {
                collect_type_references(t, references);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for t in &intersection.types {
                collect_type_references(t, references);
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_references(&arr.element_type, references);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                match elem {
                    TSTupleElement::TSOptionalType(opt) => {
                        collect_type_references(&opt.type_annotation, references);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_references(&rest.type_annotation, references);
                    }
                    _ => {
                        if let Some(t) = elem.as_ts_type() {
                            collect_type_references(t, references);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_references(&cond.check_type, references);
            collect_type_references(&cond.extends_type, references);
            collect_type_references(&cond.true_type, references);
            collect_type_references(&cond.false_type, references);
        }
        TSType::TSFunctionType(func) => {
            collect_type_references(&func.return_type.type_annotation, references);
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_type_references(&indexed.object_type, references);
            collect_type_references(&indexed.index_type, references);
        }
        TSType::TSMappedType(mapped) => {
            if let Some(t) = &mapped.type_annotation {
                collect_type_references(t, references);
            }
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_type_references(&operator.type_annotation, references);
        }
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let name_bytes = ident.name.as_bytes();
                if !is_keyword(name_bytes) {
                    references.insert(ident.name.as_str());
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_references(&paren.type_annotation, references);
        }
        _ => {}
    }
}

/// Collect TypeScript type references from an expression (for type assertions like `as T`).
pub fn collect_ts_type_references_from_expression<'a>(
    expr: &'a Expression<'a>,
    references: &mut FxHashSet<&'a str>,
) {
    match expr {
        Expression::TSAsExpression(ts_as) => {
            collect_type_references(&ts_as.type_annotation, references);
            collect_ts_type_references_from_expression(&ts_as.expression, references);
        }
        Expression::TSSatisfiesExpression(satisfies) => {
            collect_type_references(&satisfies.type_annotation, references);
            collect_ts_type_references_from_expression(&satisfies.expression, references);
        }
        Expression::TSTypeAssertion(assertion) => {
            collect_type_references(&assertion.type_annotation, references);
            collect_ts_type_references_from_expression(&assertion.expression, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_ts_type_references_from_expression(&non_null.expression, references);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_ts_type_references_from_expression(&paren.expression, references);
        }
        Expression::CallExpression(call) => {
            collect_ts_type_references_from_expression(&call.callee, references);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_ts_type_references_from_expression(e, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_ts_type_references_from_expression(&member.object, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_ts_type_references_from_expression(&member.object, references);
            collect_ts_type_references_from_expression(&member.expression, references);
        }
        Expression::BinaryExpression(binary) => {
            collect_ts_type_references_from_expression(&binary.left, references);
            collect_ts_type_references_from_expression(&binary.right, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_ts_type_references_from_expression(&logical.left, references);
            collect_ts_type_references_from_expression(&logical.right, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_ts_type_references_from_expression(&cond.test, references);
            collect_ts_type_references_from_expression(&cond.consequent, references);
            collect_ts_type_references_from_expression(&cond.alternate, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(e) = elem.as_expression() {
                    collect_ts_type_references_from_expression(e, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_ts_type_references_from_expression(&p.value, references);
                }
            }
        }
        _ => {}
    }
}

/// Collect locals from an array assignment target (for v-for destructuring).
pub fn collect_assignment_target_locals_array(
    arr: &ArrayAssignmentTarget<'_>,
    locals: &mut Vec<String>,
) {
    for elem in arr.elements.iter().flatten() {
        collect_assignment_target_maybe_default_locals(elem, locals);
    }
    if let Some(rest) = &arr.rest {
        collect_assignment_target_locals(&rest.target, locals);
    }
}

/// Collect locals from an object assignment target (for v-for destructuring).
pub fn collect_assignment_target_locals_object(
    obj: &ObjectAssignmentTarget<'_>,
    locals: &mut Vec<String>,
) {
    for prop in &obj.properties {
        match prop {
            AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(ident) => {
                locals.push(ident.binding.name.to_string());
            }
            AssignmentTargetProperty::AssignmentTargetPropertyProperty(prop) => {
                collect_assignment_target_maybe_default_locals(&prop.binding, locals);
            }
        }
    }
    if let Some(rest) = &obj.rest {
        collect_assignment_target_locals(&rest.target, locals);
    }
}

/// Collect locals from an assignment target.
pub fn collect_assignment_target_locals(target: &AssignmentTarget<'_>, locals: &mut Vec<String>) {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(ident) => {
            locals.push(ident.name.to_string());
        }
        AssignmentTarget::ArrayAssignmentTarget(arr) => {
            collect_assignment_target_locals_array(arr, locals);
        }
        AssignmentTarget::ObjectAssignmentTarget(obj) => {
            collect_assignment_target_locals_object(obj, locals);
        }
        _ => {}
    }
}

/// Collect locals from assignment target with possible default value.
pub fn collect_assignment_target_maybe_default_locals(
    target: &AssignmentTargetMaybeDefault<'_>,
    locals: &mut Vec<String>,
) {
    match target {
        AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(with_default) => {
            collect_assignment_target_locals(&with_default.binding, locals);
        }
        _ => {
            if let Some(t) = target.as_assignment_target() {
                collect_assignment_target_locals(t, locals);
            }
        }
    }
}

// =============================================================================
// Span-based collection functions
// =============================================================================
// These functions collect spans instead of string references, avoiding
// self-referential struct issues and saving memory. Use span.slice(source)
// to get the string value when needed.

/// Collect local binding spans from a binding pattern.
///
/// This extracts spans of all identifiers declared by the pattern itself.
/// For example, in `{ a, b: c }`, this returns spans for "a" and "c".
pub fn collect_pattern_local_spans(pattern: &BindingPattern<'_>, locals: &mut Vec<Span>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            locals.push(ident.span.into());
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_local_spans(&prop.value, locals);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_local_spans(&rest.argument, locals);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_local_spans(elem, locals);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_local_spans(&rest.argument, locals);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_local_spans(&assign.left, locals);
        }
    }
}

/// Collect reference spans from a binding pattern (default values).
///
/// This extracts spans of identifiers that are referenced in default value expressions.
pub fn collect_pattern_reference_spans(
    pattern: &BindingPattern<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match pattern {
        BindingPattern::AssignmentPattern(assign) => {
            collect_expression_reference_spans(&assign.right, ignored, references);
            collect_pattern_reference_spans(&assign.left, ignored, references);
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_reference_spans(&prop.value, ignored, references);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_reference_spans(&rest.argument, ignored, references);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_reference_spans(elem, ignored, references);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_reference_spans(&rest.argument, ignored, references);
            }
        }
        BindingPattern::BindingIdentifier(_) => {}
    }
}

/// Collect identifier reference spans from an expression (excluding ignored identifiers).
pub fn collect_expression_reference_spans(
    expr: &Expression<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match expr {
        Expression::Identifier(ident) => {
            let name_bytes = ident.name.as_bytes();
            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) {
                references.insert(ident.span.into());
            }
        }
        Expression::BinaryExpression(binary) => {
            collect_expression_reference_spans(&binary.left, ignored, references);
            collect_expression_reference_spans(&binary.right, ignored, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_expression_reference_spans(&logical.left, ignored, references);
            collect_expression_reference_spans(&logical.right, ignored, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_expression_reference_spans(&cond.test, ignored, references);
            collect_expression_reference_spans(&cond.consequent, ignored, references);
            collect_expression_reference_spans(&cond.alternate, ignored, references);
        }
        Expression::CallExpression(call) => {
            collect_expression_reference_spans(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
            collect_expression_reference_spans(&member.expression, ignored, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(expr) = elem.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    if p.shorthand {
                        if let PropertyKey::StaticIdentifier(ident) = &p.key {
                            let name_bytes = ident.name.as_bytes();
                            if !ignored.contains(name_bytes) && !is_keyword(name_bytes) {
                                references.insert(ident.span.into());
                            }
                        }
                    } else {
                        collect_expression_reference_spans(&p.value, ignored, references);
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_expression_reference_spans(&paren.expression, ignored, references);
        }
        Expression::UnaryExpression(unary) => {
            collect_expression_reference_spans(&unary.argument, ignored, references);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_expression_reference_spans(&ts_as.expression, ignored, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_expression_reference_spans(&non_null.expression, ignored, references);
        }
        Expression::ChainExpression(chain) => {
            collect_chain_element_reference_spans(&chain.expression, ignored, references);
        }
        Expression::AwaitExpression(await_expr) => {
            collect_expression_reference_spans(&await_expr.argument, ignored, references);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::AssignmentExpression(assign) => {
            collect_expression_reference_spans(&assign.right, ignored, references);
        }
        Expression::NewExpression(new_expr) => {
            collect_expression_reference_spans(&new_expr.callee, ignored, references);
            for arg in &new_expr.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_expression_reference_spans(&tagged.tag, ignored, references);
            for expr in &tagged.quasi.expressions {
                collect_expression_reference_spans(expr, ignored, references);
            }
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(arg) = &yield_expr.argument {
                collect_expression_reference_spans(arg, ignored, references);
            }
        }
        _ => {}
    }
}

/// Collect reference spans from chain elements (optional chaining).
pub fn collect_chain_element_reference_spans(
    element: &ChainElement<'_>,
    ignored: &FxHashSet<&[u8]>,
    references: &mut FxHashSet<Span>,
) {
    match element {
        ChainElement::CallExpression(call) => {
            collect_expression_reference_spans(&call.callee, ignored, references);
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    collect_expression_reference_spans(expr, ignored, references);
                }
            }
        }
        ChainElement::StaticMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
        }
        ChainElement::ComputedMemberExpression(member) => {
            collect_expression_reference_spans(&member.object, ignored, references);
            collect_expression_reference_spans(&member.expression, ignored, references);
        }
        ChainElement::PrivateFieldExpression(field) => {
            collect_expression_reference_spans(&field.object, ignored, references);
        }
        _ => {}
    }
}

/// Collect type reference spans from TypeScript type annotations.
pub fn collect_type_reference_spans(ts_type: &TSType<'_>, references: &mut FxHashSet<Span>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            if let TSTypeName::IdentifierReference(ident) = &type_ref.type_name {
                let name_bytes = ident.name.as_bytes();
                if !is_keyword(name_bytes) {
                    references.insert(ident.span.into());
                }
            }
            // Also check generic type arguments
            if let Some(args) = &type_ref.type_arguments {
                for arg in &args.params {
                    collect_type_reference_spans(arg, references);
                }
            }
        }
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if let Some(annotation) = &prop.type_annotation {
                            collect_type_reference_spans(&annotation.type_annotation, references);
                        }
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if let Some(annotation) = &method.return_type {
                            collect_type_reference_spans(&annotation.type_annotation, references);
                        }
                    }
                    _ => {}
                }
            }
        }
        TSType::TSUnionType(union) => {
            for t in &union.types {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for t in &intersection.types {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSArrayType(arr) => {
            collect_type_reference_spans(&arr.element_type, references);
        }
        TSType::TSTupleType(tuple) => {
            for elem in &tuple.element_types {
                match elem {
                    TSTupleElement::TSOptionalType(opt) => {
                        collect_type_reference_spans(&opt.type_annotation, references);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_reference_spans(&rest.type_annotation, references);
                    }
                    _ => {
                        if let Some(t) = elem.as_ts_type() {
                            collect_type_reference_spans(t, references);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_reference_spans(&cond.check_type, references);
            collect_type_reference_spans(&cond.extends_type, references);
            collect_type_reference_spans(&cond.true_type, references);
            collect_type_reference_spans(&cond.false_type, references);
        }
        TSType::TSFunctionType(func) => {
            collect_type_reference_spans(&func.return_type.type_annotation, references);
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_type_reference_spans(&indexed.object_type, references);
            collect_type_reference_spans(&indexed.index_type, references);
        }
        TSType::TSMappedType(mapped) => {
            if let Some(t) = &mapped.type_annotation {
                collect_type_reference_spans(t, references);
            }
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_type_reference_spans(&operator.type_annotation, references);
        }
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                let name_bytes = ident.name.as_bytes();
                if !is_keyword(name_bytes) {
                    references.insert(ident.span.into());
                }
            }
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_reference_spans(&paren.type_annotation, references);
        }
        _ => {}
    }
}

/// Collect TypeScript type reference spans from an expression (for type assertions like `as T`).
pub fn collect_ts_type_reference_spans_from_expression(
    expr: &Expression<'_>,
    references: &mut FxHashSet<Span>,
) {
    match expr {
        Expression::TSAsExpression(ts_as) => {
            collect_type_reference_spans(&ts_as.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&ts_as.expression, references);
        }
        Expression::TSSatisfiesExpression(satisfies) => {
            collect_type_reference_spans(&satisfies.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&satisfies.expression, references);
        }
        Expression::TSTypeAssertion(assertion) => {
            collect_type_reference_spans(&assertion.type_annotation, references);
            collect_ts_type_reference_spans_from_expression(&assertion.expression, references);
        }
        Expression::TSNonNullExpression(non_null) => {
            collect_ts_type_reference_spans_from_expression(&non_null.expression, references);
        }
        Expression::ParenthesizedExpression(paren) => {
            collect_ts_type_reference_spans_from_expression(&paren.expression, references);
        }
        Expression::CallExpression(call) => {
            collect_ts_type_reference_spans_from_expression(&call.callee, references);
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    collect_ts_type_reference_spans_from_expression(e, references);
                }
            }
        }
        Expression::StaticMemberExpression(member) => {
            collect_ts_type_reference_spans_from_expression(&member.object, references);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_ts_type_reference_spans_from_expression(&member.object, references);
            collect_ts_type_reference_spans_from_expression(&member.expression, references);
        }
        Expression::BinaryExpression(binary) => {
            collect_ts_type_reference_spans_from_expression(&binary.left, references);
            collect_ts_type_reference_spans_from_expression(&binary.right, references);
        }
        Expression::LogicalExpression(logical) => {
            collect_ts_type_reference_spans_from_expression(&logical.left, references);
            collect_ts_type_reference_spans_from_expression(&logical.right, references);
        }
        Expression::ConditionalExpression(cond) => {
            collect_ts_type_reference_spans_from_expression(&cond.test, references);
            collect_ts_type_reference_spans_from_expression(&cond.consequent, references);
            collect_ts_type_reference_spans_from_expression(&cond.alternate, references);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(e) = elem.as_expression() {
                    collect_ts_type_reference_spans_from_expression(e, references);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    collect_ts_type_reference_spans_from_expression(&p.value, references);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    #[test]
    fn test_collect_expression_references_identifier() {
        let allocator = Allocator::default();
        let source = "foo";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
    }

    #[test]
    fn test_collect_expression_references_with_ignored() {
        let allocator = Allocator::default();
        let source = "foo + bar";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let mut ignored = FxHashSet::default();
        ignored.insert(b"foo" as &[u8]);
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(!references.contains("foo"));
        assert!(references.contains("bar"));
    }

    #[test]
    fn test_collect_expression_references_keywords_ignored() {
        let allocator = Allocator::default();
        let source = "foo && true || null";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
        assert!(!references.contains("true"));
        assert!(!references.contains("null"));
    }

    #[test]
    fn test_collect_expression_references_member_access() {
        let allocator = Allocator::default();
        let source = "foo.bar[baz]";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo"));
        assert!(references.contains("baz"));
        assert!(!references.contains("bar")); // property access, not reference
    }

    #[test]
    fn test_collect_expression_references_shorthand_property() {
        let allocator = Allocator::default();
        let source = "{ foo, bar: baz }";
        let parser = Parser::new(&allocator, source, SourceType::tsx());
        let expr = parser.parse_expression().unwrap();

        let ignored = FxHashSet::default();
        let mut references = FxHashSet::default();
        collect_expression_references(&expr, &ignored, &mut references);

        assert!(references.contains("foo")); // shorthand
        assert!(references.contains("baz")); // value
        assert!(!references.contains("bar")); // key
    }
}
