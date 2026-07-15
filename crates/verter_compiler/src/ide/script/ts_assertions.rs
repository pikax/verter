//! TS angle-bracket type-assertion rewriter (D5 of
//! ownership-domain analysis).
//!
//! TypeScript's `TSTypeAssertion` syntax (`<string>foo`) is ambiguous with
//! JSX elements in TSX files. This module rewrites them to the equivalent
//! `as` syntax: `(foo as string)`.
//!
//! Since the main script parse uses TSX mode (where `<T>expr` is parsed as
//! JSX rather than a type assertion), we perform a separate lightweight TS
//! parse here to correctly detect them.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Argument, ArrayExpressionElement, Expression, ObjectPropertyKind, Statement};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use crate::code_transform::CodeTransform;

/// Rewrite `<Type>expr` angle bracket type assertions to `(expr as Type)` for TSX validity.
pub(super) fn rewrite_ts_type_assertions(
    content_str: &str,
    content_start: u32,
    ct: &mut CodeTransform<'_>,
) {
    // Parse as TypeScript (not TSX) so OXC produces TSTypeAssertion nodes
    let ts_alloc = Allocator::default();
    let ts_source_type = SourceType::ts();
    let ts_ret = Parser::new(&ts_alloc, content_str, ts_source_type).parse();

    let mut assertions: Vec<(u32, u32, u32)> = Vec::new(); // (assertion_start, expr_start, assertion_end)
    collect_type_assertions_from_stmts(&ts_ret.program.body, &mut assertions);

    if assertions.is_empty() {
        return;
    }

    for &(assertion_start, expr_start, assertion_end) in &assertions {
        // Extract type text from between `<` and `>`
        // The range `assertion_start..expr_start` in content_str is `<Type>`
        let type_text = &content_str[(assertion_start + 1) as usize..(expr_start - 1) as usize];

        let abs_start = content_start + assertion_start;
        let abs_expr_start = content_start + expr_start;
        let abs_end = content_start + assertion_end;

        // Replace `<Type>` with `(`
        ct.overwrite(abs_start, abs_expr_start, "(");
        // Append ` as Type)` after the expression
        ct.append_left(abs_end, &format!(" as {})", type_text));
    }
}

fn collect_type_assertions_from_stmts(stmts: &[Statement<'_>], out: &mut Vec<(u32, u32, u32)>) {
    for stmt in stmts {
        collect_type_assertions_from_stmt(stmt, out);
    }
}

fn collect_type_assertions_from_stmt(stmt: &Statement<'_>, out: &mut Vec<(u32, u32, u32)>) {
    match stmt {
        Statement::ExpressionStatement(es) => {
            collect_type_assertions_from_expr(&es.expression, out);
        }
        Statement::VariableDeclaration(vd) => {
            for decl in &vd.declarations {
                if let Some(init) = &decl.init {
                    collect_type_assertions_from_expr(init, out);
                }
            }
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                collect_type_assertions_from_expr(arg, out);
            }
        }
        Statement::IfStatement(ifs) => {
            collect_type_assertions_from_expr(&ifs.test, out);
            collect_type_assertions_from_stmt(&ifs.consequent, out);
            if let Some(alt) = &ifs.alternate {
                collect_type_assertions_from_stmt(alt, out);
            }
        }
        Statement::BlockStatement(block) => {
            collect_type_assertions_from_stmts(&block.body, out);
        }
        Statement::ForStatement(fs) => {
            if let Some(body) = Some(&fs.body) {
                collect_type_assertions_from_stmt(body, out);
            }
        }
        Statement::WhileStatement(ws) => {
            collect_type_assertions_from_expr(&ws.test, out);
            collect_type_assertions_from_stmt(&ws.body, out);
        }
        _ => {}
    }
}

fn collect_type_assertions_from_expr(expr: &Expression<'_>, out: &mut Vec<(u32, u32, u32)>) {
    match expr {
        Expression::TSTypeAssertion(ta) => {
            // Record this assertion (process inner first for nesting)
            collect_type_assertions_from_expr(&ta.expression, out);
            out.push((ta.span.start, ta.expression.span().start, ta.span.end));
        }
        Expression::AssignmentExpression(ae) => {
            collect_type_assertions_from_expr(&ae.right, out);
        }
        Expression::BinaryExpression(be) => {
            collect_type_assertions_from_expr(&be.left, out);
            collect_type_assertions_from_expr(&be.right, out);
        }
        Expression::LogicalExpression(le) => {
            collect_type_assertions_from_expr(&le.left, out);
            collect_type_assertions_from_expr(&le.right, out);
        }
        Expression::ConditionalExpression(ce) => {
            collect_type_assertions_from_expr(&ce.test, out);
            collect_type_assertions_from_expr(&ce.consequent, out);
            collect_type_assertions_from_expr(&ce.alternate, out);
        }
        Expression::CallExpression(call) => {
            collect_type_assertions_from_expr(&call.callee, out);
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    collect_type_assertions_from_expr(&spread.argument, out);
                } else {
                    collect_type_assertions_from_expr(arg.to_expression(), out);
                }
            }
        }
        Expression::ParenthesizedExpression(pe) => {
            collect_type_assertions_from_expr(&pe.expression, out);
        }
        Expression::SequenceExpression(se) => {
            for e in &se.expressions {
                collect_type_assertions_from_expr(e, out);
            }
        }
        Expression::ArrayExpression(ae) => {
            for el in &ae.elements {
                match el {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_type_assertions_from_expr(&spread.argument, out);
                    }
                    ArrayExpressionElement::TSTypeAssertion(ta) => {
                        collect_type_assertions_from_expr(&ta.expression, out);
                        out.push((ta.span.start, ta.expression.span().start, ta.span.end));
                    }
                    _ => {}
                }
            }
        }
        Expression::ObjectExpression(oe) => {
            for prop in &oe.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = prop {
                    collect_type_assertions_from_expr(&op.value, out);
                }
            }
        }
        Expression::ArrowFunctionExpression(afe) => {
            collect_type_assertions_from_stmts(&afe.body.statements, out);
        }
        Expression::TSAsExpression(tsa) => {
            collect_type_assertions_from_expr(&tsa.expression, out);
        }
        Expression::TSSatisfiesExpression(tss) => {
            collect_type_assertions_from_expr(&tss.expression, out);
        }
        Expression::TSNonNullExpression(tsnn) => {
            collect_type_assertions_from_expr(&tsnn.expression, out);
        }
        Expression::AwaitExpression(ae) => {
            collect_type_assertions_from_expr(&ae.argument, out);
        }
        Expression::UnaryExpression(ue) => {
            collect_type_assertions_from_expr(&ue.argument, out);
        }
        Expression::TemplateLiteral(tl) => {
            for expr in &tl.expressions {
                collect_type_assertions_from_expr(expr, out);
            }
        }
        Expression::ComputedMemberExpression(cme) => {
            collect_type_assertions_from_expr(&cme.object, out);
            collect_type_assertions_from_expr(&cme.expression, out);
        }
        Expression::StaticMemberExpression(sme) => {
            collect_type_assertions_from_expr(&sme.object, out);
        }
        Expression::PrivateFieldExpression(pfe) => {
            collect_type_assertions_from_expr(&pfe.object, out);
        }
        _ => {}
    }
}
