//! Detectors for `useAttrs()` and `getCurrentInstance()` calls in script
//! setup bodies (D8 of Phase 11d ownership-domain analysis).

use oxc_ast::ast::{Expression, Statement};
use oxc_span::GetSpan;

use super::callee_identifier_name;

/// Result of detecting `useAttrs()` calls in script setup.
pub(super) struct UseAttrsDetection {
    /// Type argument text from `useAttrs<T>()`, if found.
    pub(super) type_arg: Option<String>,
    /// Content-relative end offsets of bare `useAttrs()` calls (no type param).
    pub(super) bare_call_ends: Vec<u32>,
}

/// Detect `useAttrs()` calls in the script setup body.
///
/// Returns both the type parameter text (from `useAttrs<T>()`) and the
/// end offsets of bare `useAttrs()` calls that need a type assertion cast.
///
/// Priority: `attrs` attribute > `useAttrs<T>()` > `{}` (default).
pub(super) fn detect_use_attrs_calls<'a>(
    body: &[Statement<'a>],
    source: &'a str,
) -> UseAttrsDetection {
    let mut result = UseAttrsDetection {
        type_arg: None,
        bare_call_ends: Vec::new(),
    };
    for stmt in body {
        let call = match stmt {
            Statement::VariableDeclaration(var_decl) => var_decl
                .declarations
                .iter()
                .find_map(|d| d.init.as_ref())
                .and_then(|e| {
                    if let Expression::CallExpression(c) = e {
                        Some(c.as_ref())
                    } else {
                        None
                    }
                }),
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::CallExpression(c) = &expr_stmt.expression {
                    Some(c.as_ref())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(call) = call {
            if let Some(name) = callee_identifier_name(&call.callee) {
                if name == "useAttrs" {
                    if let Some(tp) = &call.type_arguments {
                        if let Some(param) = tp.params.first() {
                            let span: oxc_span::Span = param.span();
                            let text = &source[span.start as usize..span.end as usize];
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                result.type_arg = Some(trimmed.to_string());
                            }
                        }
                    } else {
                        // Bare useAttrs() — collect end offset for casting
                        result.bare_call_ends.push(call.span().end);
                    }
                }
            }
        }
    }
    result
}

/// Detect if script setup body contains a `getCurrentInstance()` call.
pub(super) fn detect_get_current_instance(body: &[Statement<'_>]) -> bool {
    for stmt in body {
        if detect_gci_in_stmt(stmt) {
            return true;
        }
    }
    false
}

fn detect_gci_in_stmt(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(init) = &decl.init {
                    if detect_gci_in_expr(init) {
                        return true;
                    }
                }
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            if detect_gci_in_expr(&expr_stmt.expression) {
                return true;
            }
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                if detect_gci_in_expr(arg) {
                    return true;
                }
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                if detect_gci_in_stmt(s) {
                    return true;
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            if detect_gci_in_expr(&if_stmt.test) || detect_gci_in_stmt(&if_stmt.consequent) {
                return true;
            }
            if let Some(alt) = &if_stmt.alternate {
                if detect_gci_in_stmt(alt) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

fn detect_gci_in_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(name) = callee_identifier_name(&call.callee) {
                if name == "getCurrentInstance" {
                    return true;
                }
            }
            if detect_gci_in_expr(&call.callee) {
                return true;
            }
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    if detect_gci_in_expr(e) {
                        return true;
                    }
                }
            }
        }
        Expression::AssignmentExpression(ae) => {
            if detect_gci_in_expr(&ae.right) {
                return true;
            }
        }
        Expression::ParenthesizedExpression(p) => {
            if detect_gci_in_expr(&p.expression) {
                return true;
            }
        }
        Expression::ConditionalExpression(c) => {
            if detect_gci_in_expr(&c.test)
                || detect_gci_in_expr(&c.consequent)
                || detect_gci_in_expr(&c.alternate)
            {
                return true;
            }
        }
        Expression::LogicalExpression(l) => {
            if detect_gci_in_expr(&l.left) || detect_gci_in_expr(&l.right) {
                return true;
            }
        }
        _ => {}
    }
    false
}
