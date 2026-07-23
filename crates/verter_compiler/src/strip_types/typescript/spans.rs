use oxc_ast::ast::*;
use oxc_span::GetSpan;

// =========================================================================
// TS removal span collection (for template directive expressions)
// =========================================================================

/// Collect TypeScript-only byte ranges from a single expression AST.
///
/// Returns expression-relative `(start, end)` spans sorted by start position.
/// Used by `build_prefixed_expr()` to skip TS syntax when building resolved
/// expression strings for directive values (v-bind, v-if, etc.).
///
/// Only collects `remove` operations — template expressions never contain
/// enum declarations or class member accessibility modifiers that need `overwrite`.
pub fn collect_ts_removal_spans(expression: &Expression) -> Vec<(u32, u32)> {
    let mut spans = Vec::new();
    collect_expr_ts_spans(expression, &mut spans);
    spans.sort_by_key(|&(start, _)| start);
    spans
}

fn collect_expr_ts_spans(expr: &Expression, out: &mut Vec<(u32, u32)>) {
    match expr {
        // TS assertion expressions — remove type annotation, keep value
        Expression::TSAsExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSSatisfiesExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSTypeAssertion(e) => {
            out.push((e.span.start, e.expression.span().start));
            collect_expr_ts_spans(&e.expression, out);
        }
        Expression::TSInstantiationExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }

        // Expressions with type arguments
        Expression::CallExpression(call) => {
            if let Some(ta) = &call.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&call.callee, out);
            collect_args_ts_spans(&call.arguments, out);
        }
        Expression::NewExpression(new_expr) => {
            if let Some(ta) = &new_expr.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&new_expr.callee, out);
            collect_args_ts_spans(&new_expr.arguments, out);
        }
        Expression::TaggedTemplateExpression(tagged) => {
            if let Some(ta) = &tagged.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&tagged.tag, out);
            for e in &tagged.quasi.expressions {
                collect_expr_ts_spans(e, out);
            }
        }

        // Function expressions with type annotations
        Expression::ArrowFunctionExpression(arrow) => {
            if let Some(tp) = &arrow.type_parameters {
                out.push((tp.span.start, tp.span.end));
            }
            if let Some(rt) = &arrow.return_type {
                out.push((rt.span.start, rt.span.end));
            }
            collect_formal_params_ts_spans(&arrow.params, out);
            for stmt in &arrow.body.statements {
                collect_stmt_ts_spans(stmt, out);
            }
        }
        Expression::FunctionExpression(func) => {
            if let Some(tp) = &func.type_parameters {
                out.push((tp.span.start, tp.span.end));
            }
            if let Some(rt) = &func.return_type {
                out.push((rt.span.start, rt.span.end));
            }
            collect_formal_params_ts_spans(&func.params, out);
            if let Some(body) = &func.body {
                for stmt in &body.statements {
                    collect_stmt_ts_spans(stmt, out);
                }
            }
        }

        // Container expressions — recurse
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                match element {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_expr_ts_spans(&spread.argument, out);
                    }
                    ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = element.as_expression() {
                            collect_expr_ts_spans(e, out);
                        }
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        if p.computed {
                            if let Some(e) = p.key.as_expression() {
                                collect_expr_ts_spans(e, out);
                            }
                        }
                        collect_expr_ts_spans(&p.value, out);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        collect_expr_ts_spans(&spread.argument, out);
                    }
                }
            }
        }
        Expression::TemplateLiteral(t) => {
            for e in &t.expressions {
                collect_expr_ts_spans(e, out);
            }
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_expr_ts_spans(e, out);
            }
        }
        Expression::ParenthesizedExpression(p) => {
            collect_expr_ts_spans(&p.expression, out);
        }

        // Binary/unary/conditional — recurse
        Expression::AssignmentExpression(a) => {
            collect_assign_target_ts_spans(&a.left, out);
            collect_expr_ts_spans(&a.right, out);
        }
        Expression::BinaryExpression(b) => {
            collect_expr_ts_spans(&b.left, out);
            collect_expr_ts_spans(&b.right, out);
        }
        Expression::LogicalExpression(l) => {
            collect_expr_ts_spans(&l.left, out);
            collect_expr_ts_spans(&l.right, out);
        }
        Expression::UnaryExpression(u) => {
            collect_expr_ts_spans(&u.argument, out);
        }
        Expression::ConditionalExpression(c) => {
            collect_expr_ts_spans(&c.test, out);
            collect_expr_ts_spans(&c.consequent, out);
            collect_expr_ts_spans(&c.alternate, out);
        }

        // Member expressions
        Expression::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        Expression::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        Expression::PrivateFieldExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }

        // Optional chaining
        Expression::ChainExpression(chain) => {
            collect_chain_ts_spans(&chain.expression, out);
        }

        // Await/yield
        Expression::AwaitExpression(a) => {
            collect_expr_ts_spans(&a.argument, out);
        }
        Expression::YieldExpression(y) => {
            if let Some(arg) = &y.argument {
                collect_expr_ts_spans(arg, out);
            }
        }

        // Identifiers, literals, this — nothing to collect
        _ => {}
    }
}

fn collect_chain_ts_spans(element: &ChainElement, out: &mut Vec<(u32, u32)>) {
    match element {
        ChainElement::CallExpression(call) => {
            if let Some(ta) = &call.type_arguments {
                out.push((ta.span.start, ta.span.end));
            }
            collect_expr_ts_spans(&call.callee, out);
            collect_args_ts_spans(&call.arguments, out);
        }
        ChainElement::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        ChainElement::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        ChainElement::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        ChainElement::PrivateFieldExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
    }
}

fn collect_args_ts_spans(args: &[Argument], out: &mut Vec<(u32, u32)>) {
    for arg in args {
        match arg {
            Argument::SpreadElement(spread) => {
                collect_expr_ts_spans(&spread.argument, out);
            }
            _ => {
                if let Some(expr) = arg.as_expression() {
                    collect_expr_ts_spans(expr, out);
                }
            }
        }
    }
}

fn collect_assign_target_ts_spans(target: &AssignmentTarget, out: &mut Vec<(u32, u32)>) {
    match target {
        AssignmentTarget::TSAsExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        AssignmentTarget::TSSatisfiesExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        AssignmentTarget::TSNonNullExpression(e) => {
            out.push((e.expression.span().end, e.span.end));
            collect_expr_ts_spans(&e.expression, out);
        }
        AssignmentTarget::TSTypeAssertion(e) => {
            out.push((e.span.start, e.expression.span().start));
            collect_expr_ts_spans(&e.expression, out);
        }
        AssignmentTarget::ComputedMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
            collect_expr_ts_spans(&m.expression, out);
        }
        AssignmentTarget::StaticMemberExpression(m) => {
            collect_expr_ts_spans(&m.object, out);
        }
        _ => {}
    }
}

fn collect_formal_params_ts_spans(params: &FormalParameters, out: &mut Vec<(u32, u32)>) {
    for param in &params.items {
        if let Some(ta) = &param.type_annotation {
            out.push((ta.span.start, ta.span.end));
        }
    }
    if let Some(rest) = &params.rest {
        if let Some(ta) = &rest.type_annotation {
            out.push((ta.span.start, ta.span.end));
        }
    }
}

fn collect_stmt_ts_spans(stmt: &Statement, out: &mut Vec<(u32, u32)>) {
    match stmt {
        Statement::ExpressionStatement(e) => collect_expr_ts_spans(&e.expression, out),
        Statement::ReturnStatement(r) => {
            if let Some(arg) = &r.argument {
                collect_expr_ts_spans(arg, out);
            }
        }
        Statement::VariableDeclaration(v) => {
            for decl in &v.declarations {
                if let Some(ta) = &decl.type_annotation {
                    out.push((ta.span.start, ta.span.end));
                }
                if let Some(init) = &decl.init {
                    collect_expr_ts_spans(init, out);
                }
            }
        }
        Statement::IfStatement(i) => {
            collect_expr_ts_spans(&i.test, out);
            collect_stmt_ts_spans(&i.consequent, out);
            if let Some(alt) = &i.alternate {
                collect_stmt_ts_spans(alt, out);
            }
        }
        Statement::BlockStatement(b) => {
            for s in &b.body {
                collect_stmt_ts_spans(s, out);
            }
        }
        _ => {}
    }
}
