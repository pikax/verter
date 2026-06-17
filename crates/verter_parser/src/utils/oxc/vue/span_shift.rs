//! Wrapped → file-relative span shifting for v-slot formal parameters.
//!
//! A v-slot expression is parsed inside a synthetic arrow wrapper
//! (`{ content }` → `({ content })=>{}`), so every parsed span carries a leading
//! one-byte `(` prefix. These helpers move a parsed `FormalParameters` tree
//! straight to file-relative coordinates in a single traversal: value-position
//! spans (the params/pattern structure and every reachable default-value
//! reference) lose the synthetic prefix and land on their true source byte, while
//! display-only positions (type annotations) keep it folded in via the file-only
//! walk in [`super::span`].

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, ArrayPattern, BindingPattern, BindingProperty,
    BindingRestElement, ChainElement, Expression, FormalParameter, FormalParameters, ObjectPattern,
    ObjectPropertyKind, PropertyKey,
};
use oxc_span::Span;

use super::span::{adjust_assignment_target_spans, adjust_expression_spans, adjust_span};

/// Move a span from wrapped-relative to file-relative in one step: strip the
/// synthetic arrow-wrapper prefix (saturating, so the wrapper paren maps to the
/// content start) and add the file offset.
#[inline]
fn shift_span_into(span: &mut Span, wrapper_offset: u32, file_offset: u32) {
    span.start = span.start.saturating_sub(wrapper_offset) + file_offset;
    span.end = span.end.saturating_sub(wrapper_offset) + file_offset;
}

/// Shift every span in `FormalParameters` from wrapped (`(content)=>{}`) coordinates
/// straight to file-relative in a single traversal.
///
/// Value-position spans (the params/pattern structure and reachable default-value
/// expressions) lose the one-byte wrapper prefix and land at their file position.
/// Type-annotation spans take only the file shift — they keep the synthetic prefix
/// because they are display-only positions that are never source-mapped, so the
/// wrapper byte is left folded into them rather than spent on a separate unwrap walk.
pub fn shift_formal_parameters_spans(
    params: &mut FormalParameters<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    if wrapper_offset == 0 && file_offset == 0 {
        return;
    }

    shift_span_into(&mut params.span, wrapper_offset, file_offset);
    for param in &mut params.items {
        shift_formal_parameter_spans(param, wrapper_offset, file_offset);
    }
    if let Some(rest) = &mut params.rest {
        shift_span_into(&mut rest.span, wrapper_offset, file_offset);
        if let Some(ta) = &mut rest.type_annotation {
            adjust_span(&mut ta.span, file_offset);
        }
        shift_binding_pattern_spans(&mut rest.rest.argument, wrapper_offset, file_offset);
    }
}

fn shift_formal_parameter_spans(
    param: &mut FormalParameter<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    shift_span_into(&mut param.span, wrapper_offset, file_offset);
    shift_binding_pattern_spans(&mut param.pattern, wrapper_offset, file_offset);
    if let Some(ta) = &mut param.type_annotation {
        adjust_span(&mut ta.span, file_offset);
    }
    if let Some(init) = &mut param.initializer {
        shift_default_expression_spans(init, wrapper_offset, file_offset);
    }
}

fn shift_binding_pattern_spans(
    pattern: &mut BindingPattern<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            shift_span_into(&mut id.span, wrapper_offset, file_offset);
        }
        BindingPattern::ObjectPattern(obj) => {
            shift_object_pattern_spans(obj, wrapper_offset, file_offset);
        }
        BindingPattern::ArrayPattern(arr) => {
            shift_array_pattern_spans(arr, wrapper_offset, file_offset);
        }
        BindingPattern::AssignmentPattern(assign) => {
            shift_span_into(&mut assign.span, wrapper_offset, file_offset);
            shift_binding_pattern_spans(&mut assign.left, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut assign.right, wrapper_offset, file_offset);
        }
    }
}

fn shift_object_pattern_spans(obj: &mut ObjectPattern<'_>, wrapper_offset: u32, file_offset: u32) {
    shift_span_into(&mut obj.span, wrapper_offset, file_offset);
    for prop in &mut obj.properties {
        shift_binding_property_spans(prop, wrapper_offset, file_offset);
    }
    if let Some(rest) = &mut obj.rest {
        shift_binding_rest_element_spans(rest, wrapper_offset, file_offset);
    }
}

fn shift_binding_property_spans(
    prop: &mut BindingProperty<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    shift_span_into(&mut prop.span, wrapper_offset, file_offset);
    // Identifier-name keys are not `Expression` variants and are not binding sites,
    // so they were never unwrapped; only an expression key (`{ [k]: v }`) is.
    if let Some(key_expr) = prop.key.as_expression_mut() {
        shift_default_expression_spans(key_expr, wrapper_offset, file_offset);
    }
    shift_binding_pattern_spans(&mut prop.value, wrapper_offset, file_offset);
}

fn shift_array_pattern_spans(arr: &mut ArrayPattern<'_>, wrapper_offset: u32, file_offset: u32) {
    shift_span_into(&mut arr.span, wrapper_offset, file_offset);
    for elem in arr.elements.iter_mut().flatten() {
        shift_binding_pattern_spans(elem, wrapper_offset, file_offset);
    }
    if let Some(rest) = &mut arr.rest {
        shift_binding_rest_element_spans(rest, wrapper_offset, file_offset);
    }
}

fn shift_binding_rest_element_spans(
    rest: &mut BindingRestElement<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    shift_span_into(&mut rest.span, wrapper_offset, file_offset);
    shift_binding_pattern_spans(&mut rest.argument, wrapper_offset, file_offset);
}

/// Shift a v-slot default-value expression from wrapped (`(content)=>{}`)
/// coordinates straight to file-relative in one traversal.
///
/// Every span a reference collector ([`collect_expression_reference_spans`]) can
/// reach inside a default value is a value-position reference, so the whole
/// reachable subtree loses the synthetic one-byte wrapper prefix and lands on its
/// true source byte. The match is exhaustive over `Expression`: each
/// reference-bearing kind is walked with the wrapper-strip shift, so no compound
/// expression can silently fall through to a file-offset-only adjustment.
///
/// Two position classes deliberately keep the wrapper byte folded in via the
/// file-only [`adjust_expression_spans`], matching every non-value walk:
/// - Embedded TypeScript type annotations (`x = y as Foo`): the `Foo` annotation
///   is display-only and never source-mapped, so the `as`/`!` arms shift only the
///   value expression and leave the annotation untouched.
/// - Expression kinds the reference collector treats as opaque or ignores
///   (arrow/function/class bodies introduce their own scope; `satisfies`, type
///   assertions, `import()`, update, JSX, `this`/`super`/meta, regexp/bigint
///   literals carry no collected free references), which take only the file shift.
///
/// [`collect_expression_reference_spans`]: crate::utils::oxc::bindings::collect_expression_reference_spans
fn shift_default_expression_spans(
    expr: &mut Expression<'_>,
    wrapper_offset: u32,
    file_offset: u32,
) {
    match expr {
        // ---- Leaves: the node IS the value span ----
        Expression::Identifier(id) => shift_span_into(&mut id.span, wrapper_offset, file_offset),
        Expression::BooleanLiteral(lit) => {
            shift_span_into(&mut lit.span, wrapper_offset, file_offset)
        }
        Expression::NullLiteral(lit) => shift_span_into(&mut lit.span, wrapper_offset, file_offset),
        Expression::NumericLiteral(lit) => {
            shift_span_into(&mut lit.span, wrapper_offset, file_offset)
        }
        Expression::StringLiteral(lit) => {
            shift_span_into(&mut lit.span, wrapper_offset, file_offset)
        }

        // ---- Compound value expressions: strip the wrapper across the subtree ----
        Expression::TemplateLiteral(tpl) => {
            shift_span_into(&mut tpl.span, wrapper_offset, file_offset);
            for quasi in &mut tpl.quasis {
                shift_span_into(&mut quasi.span, wrapper_offset, file_offset);
            }
            for sub in &mut tpl.expressions {
                shift_default_expression_spans(sub, wrapper_offset, file_offset);
            }
        }
        Expression::ArrayExpression(arr) => {
            shift_span_into(&mut arr.span, wrapper_offset, file_offset);
            for elem in &mut arr.elements {
                match elem {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        shift_span_into(&mut spread.span, wrapper_offset, file_offset);
                        shift_default_expression_spans(
                            &mut spread.argument,
                            wrapper_offset,
                            file_offset,
                        );
                    }
                    ArrayExpressionElement::Elision(elision) => {
                        shift_span_into(&mut elision.span, wrapper_offset, file_offset);
                    }
                    _ => {
                        if let Some(e) = elem.as_expression_mut() {
                            shift_default_expression_spans(e, wrapper_offset, file_offset);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            shift_span_into(&mut obj.span, wrapper_offset, file_offset);
            for prop in &mut obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        shift_span_into(&mut p.span, wrapper_offset, file_offset);
                        // A shorthand key (`{ foo }`) is itself the collected
                        // reference, so its identifier span must be stripped too;
                        // a computed key (`{ [k]: v }`) is a nested value
                        // expression. Both move with the rest of the tree.
                        match &mut p.key {
                            PropertyKey::StaticIdentifier(id) => {
                                shift_span_into(&mut id.span, wrapper_offset, file_offset);
                            }
                            PropertyKey::PrivateIdentifier(id) => {
                                shift_span_into(&mut id.span, wrapper_offset, file_offset);
                            }
                            _ => {
                                if let Some(key_expr) = p.key.as_expression_mut() {
                                    shift_default_expression_spans(
                                        key_expr,
                                        wrapper_offset,
                                        file_offset,
                                    );
                                }
                            }
                        }
                        shift_default_expression_spans(&mut p.value, wrapper_offset, file_offset);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        shift_span_into(&mut spread.span, wrapper_offset, file_offset);
                        shift_default_expression_spans(
                            &mut spread.argument,
                            wrapper_offset,
                            file_offset,
                        );
                    }
                }
            }
        }
        Expression::ParenthesizedExpression(paren) => {
            shift_span_into(&mut paren.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut paren.expression, wrapper_offset, file_offset);
        }
        Expression::SequenceExpression(seq) => {
            shift_span_into(&mut seq.span, wrapper_offset, file_offset);
            for sub in &mut seq.expressions {
                shift_default_expression_spans(sub, wrapper_offset, file_offset);
            }
        }
        Expression::StaticMemberExpression(mem) => {
            shift_span_into(&mut mem.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
            shift_span_into(&mut mem.property.span, wrapper_offset, file_offset);
        }
        Expression::ComputedMemberExpression(mem) => {
            shift_span_into(&mut mem.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut mem.expression, wrapper_offset, file_offset);
        }
        Expression::PrivateFieldExpression(mem) => {
            shift_span_into(&mut mem.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
            shift_span_into(&mut mem.field.span, wrapper_offset, file_offset);
        }
        Expression::CallExpression(call) => {
            shift_span_into(&mut call.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut call.callee, wrapper_offset, file_offset);
            for arg in &mut call.arguments {
                shift_call_argument_spans(arg, wrapper_offset, file_offset);
            }
        }
        Expression::NewExpression(new) => {
            shift_span_into(&mut new.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut new.callee, wrapper_offset, file_offset);
            for arg in &mut new.arguments {
                shift_call_argument_spans(arg, wrapper_offset, file_offset);
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            shift_span_into(&mut tagged.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut tagged.tag, wrapper_offset, file_offset);
            shift_span_into(&mut tagged.quasi.span, wrapper_offset, file_offset);
            for quasi in &mut tagged.quasi.quasis {
                shift_span_into(&mut quasi.span, wrapper_offset, file_offset);
            }
            for sub in &mut tagged.quasi.expressions {
                shift_default_expression_spans(sub, wrapper_offset, file_offset);
            }
        }
        Expression::UnaryExpression(unary) => {
            shift_span_into(&mut unary.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut unary.argument, wrapper_offset, file_offset);
        }
        Expression::BinaryExpression(binary) => {
            shift_span_into(&mut binary.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut binary.left, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut binary.right, wrapper_offset, file_offset);
        }
        Expression::LogicalExpression(logical) => {
            shift_span_into(&mut logical.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut logical.left, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut logical.right, wrapper_offset, file_offset);
        }
        Expression::ConditionalExpression(cond) => {
            shift_span_into(&mut cond.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut cond.test, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut cond.consequent, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut cond.alternate, wrapper_offset, file_offset);
        }
        Expression::YieldExpression(yield_expr) => {
            shift_span_into(&mut yield_expr.span, wrapper_offset, file_offset);
            if let Some(arg) = &mut yield_expr.argument {
                shift_default_expression_spans(arg, wrapper_offset, file_offset);
            }
        }
        Expression::AwaitExpression(await_expr) => {
            shift_span_into(&mut await_expr.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut await_expr.argument, wrapper_offset, file_offset);
        }
        Expression::ChainExpression(chain) => {
            shift_span_into(&mut chain.span, wrapper_offset, file_offset);
            match &mut chain.expression {
                ChainElement::CallExpression(call) => {
                    shift_span_into(&mut call.span, wrapper_offset, file_offset);
                    shift_default_expression_spans(&mut call.callee, wrapper_offset, file_offset);
                    for arg in &mut call.arguments {
                        shift_call_argument_spans(arg, wrapper_offset, file_offset);
                    }
                }
                ChainElement::StaticMemberExpression(mem) => {
                    shift_span_into(&mut mem.span, wrapper_offset, file_offset);
                    shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
                    shift_span_into(&mut mem.property.span, wrapper_offset, file_offset);
                }
                ChainElement::ComputedMemberExpression(mem) => {
                    shift_span_into(&mut mem.span, wrapper_offset, file_offset);
                    shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
                    shift_default_expression_spans(
                        &mut mem.expression,
                        wrapper_offset,
                        file_offset,
                    );
                }
                ChainElement::PrivateFieldExpression(mem) => {
                    shift_span_into(&mut mem.span, wrapper_offset, file_offset);
                    shift_default_expression_spans(&mut mem.object, wrapper_offset, file_offset);
                    shift_span_into(&mut mem.field.span, wrapper_offset, file_offset);
                }
                ChainElement::TSNonNullExpression(ts_non_null) => {
                    shift_span_into(&mut ts_non_null.span, wrapper_offset, file_offset);
                    shift_default_expression_spans(
                        &mut ts_non_null.expression,
                        wrapper_offset,
                        file_offset,
                    );
                }
            }
        }
        // `as`/`!` carry a value expression plus a display-only type annotation:
        // strip the wrapper from the value, leave the annotation folded (it is
        // never source-mapped), exactly as the file-only walk leaves it.
        Expression::TSAsExpression(ts_as) => {
            shift_span_into(&mut ts_as.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut ts_as.expression, wrapper_offset, file_offset);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            shift_span_into(&mut ts_non_null.span, wrapper_offset, file_offset);
            shift_default_expression_spans(
                &mut ts_non_null.expression,
                wrapper_offset,
                file_offset,
            );
        }
        Expression::AssignmentExpression(assign) => {
            shift_span_into(&mut assign.span, wrapper_offset, file_offset);
            // Only the right-hand side is a collected reference; the assignment
            // target keeps the file-only placement of every non-value position.
            adjust_assignment_target_spans(&mut assign.left, file_offset);
            shift_default_expression_spans(&mut assign.right, wrapper_offset, file_offset);
        }

        // Opaque scopes and kinds the reference collector does not reach: the
        // synthetic prefix stays folded in (file shift only), matching every
        // non-value walk. Listed explicitly so a new `Expression` variant fails
        // the match rather than silently inheriting either behaviour.
        Expression::ArrowFunctionExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ClassExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::TSSatisfiesExpression(_)
        | Expression::TSTypeAssertion(_)
        | Expression::TSInstantiationExpression(_)
        | Expression::ImportExpression(_)
        | Expression::ThisExpression(_)
        | Expression::Super(_)
        | Expression::MetaProperty(_)
        | Expression::RegExpLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::JSXElement(_)
        | Expression::JSXFragment(_)
        | Expression::PrivateInExpression(_)
        | Expression::V8IntrinsicExpression(_) => adjust_expression_spans(expr, file_offset),
    }
}

/// Shift a call/`new` argument from wrapped to file-relative, stripping the
/// synthetic wrapper prefix across the value subtree (including spread arguments).
fn shift_call_argument_spans(arg: &mut Argument<'_>, wrapper_offset: u32, file_offset: u32) {
    match arg {
        Argument::SpreadElement(spread) => {
            shift_span_into(&mut spread.span, wrapper_offset, file_offset);
            shift_default_expression_spans(&mut spread.argument, wrapper_offset, file_offset);
        }
        _ => {
            if let Some(e) = arg.as_expression_mut() {
                shift_default_expression_spans(e, wrapper_offset, file_offset);
            }
        }
    }
}
