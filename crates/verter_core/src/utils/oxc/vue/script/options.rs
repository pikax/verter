//! Options API script parsing.
//!
//! This module handles parsing of `<script>` blocks (without setup attribute),
//! detecting export default and extracting component options.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use oxc_ast::ast::*;

use super::macros::is_define_component;
use super::setup::{process_setup_statements, SetupContext};
use super::shared::ScriptParseContext;
use super::types::{
    DeclarationKind, DefaultExportType, ScriptDeclaration, ScriptDefaultExport, ScriptError,
    ScriptItem,
};
use crate::common::Span;

/// Context for options script parsing
pub struct OptionsContext {
    /// Whether we're inside export default
    in_export_default: bool,
}

impl Default for OptionsContext {
    fn default() -> Self {
        Self::new()
    }
}

impl OptionsContext {
    pub fn new() -> Self {
        Self {
            in_export_default: false,
        }
    }
}

/// Process statements for options mode and collect items
pub fn process_options_statements<'a>(
    statements: &[Statement<'a>],
    ctx: &ScriptParseContext<'a>,
    options_ctx: &mut OptionsContext,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) {
    for stmt in statements {
        process_options_statement(stmt, ctx, options_ctx, items, errors, is_async);
    }
}

/// Process a single statement in options mode
pub fn process_options_statement<'a>(
    stmt: &Statement<'a>,
    ctx: &ScriptParseContext<'a>,
    _options_ctx: &mut OptionsContext,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) {
    match stmt {
        // Skip imports - handled separately by shared
        Statement::ImportDeclaration(_) => {}

        // Handle export default
        Statement::ExportDefaultDeclaration(export) => {
            let default_export = process_default_export(export, ctx, items, errors, is_async);
            items.push(ScriptItem::DefaultExport(default_export));
        }

        // Track file-scoped variable declarations
        Statement::VariableDeclaration(var_decl) => {
            let kind = match var_decl.kind {
                VariableDeclarationKind::Const => DeclarationKind::Const,
                VariableDeclarationKind::Let => DeclarationKind::Let,
                VariableDeclarationKind::Var => DeclarationKind::Var,
                VariableDeclarationKind::Using => DeclarationKind::Const,
                VariableDeclarationKind::AwaitUsing => DeclarationKind::Const,
            };

            for declarator in &var_decl.declarations {
                collect_declarations_from_pattern(&declarator.id, kind, ctx, items);
            }
        }

        // Track file-scoped function declarations
        Statement::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                let kind = match (func.r#async, func.generator) {
                    (true, true) => DeclarationKind::AsyncGeneratorFunction,
                    (true, false) => DeclarationKind::AsyncFunction,
                    (false, true) => DeclarationKind::GeneratorFunction,
                    (false, false) => DeclarationKind::Function,
                };

                items.push(ScriptItem::Declaration(ScriptDeclaration {
                    span: ctx.adjust_span(func.span),
                    name: Some(id.name.as_str()),
                    name_span: Some(ctx.adjust_span(id.span)),
                    kind,
                    is_ref_like: false,
                }));
            }
        }

        // Track file-scoped class declarations
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                items.push(ScriptItem::Declaration(ScriptDeclaration {
                    span: ctx.adjust_span(class.span),
                    name: Some(id.name.as_str()),
                    name_span: Some(ctx.adjust_span(id.span)),
                    kind: DeclarationKind::Class,
                    is_ref_like: false,
                }));
            }
        }

        // Named exports - already handled by shared
        Statement::ExportNamedDeclaration(_) | Statement::ExportAllDeclaration(_) => {}

        _ => {}
    }
}

/// Process an export default declaration
fn process_default_export<'a>(
    export: &ExportDefaultDeclaration<'a>,
    ctx: &ScriptParseContext<'a>,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) -> ScriptDefaultExport<'a> {
    let span = ctx.adjust_span(export.span);

    // Check known declaration types first
    match &export.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
            return ScriptDefaultExport::new(span, DefaultExportType::Function);
        }
        ExportDefaultDeclarationKind::ClassDeclaration(_) => {
            return ScriptDefaultExport::new(span, DefaultExportType::Class);
        }
        ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {
            return ScriptDefaultExport::new(span, DefaultExportType::Other);
        }
        _ => {}
    }

    // For expression-based exports, use as_expression()
    if let Some(expr) = export.declaration.as_expression() {
        return analyze_default_export_expression(expr, span, ctx, items, errors, is_async);
    }

    // Fallback for other declaration types
    ScriptDefaultExport::new(span, DefaultExportType::Other)
}

/// Analyze an expression used as default export
fn analyze_default_export_expression<'a>(
    expr: &Expression<'a>,
    span: Span,
    ctx: &ScriptParseContext<'a>,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) -> ScriptDefaultExport<'a> {
    match expr {
        // Plain object: export default { ... }
        Expression::ObjectExpression(obj) => {
            let mut default_export = ScriptDefaultExport::new(span, DefaultExportType::Object)
                .with_object_span(ctx.adjust_span(obj.span));

            // Look for setup function
            if let Some(setup_body_span) = find_setup_in_object(obj, ctx, items, errors, is_async) {
                default_export = default_export.with_setup_body_span(setup_body_span);
            }

            default_export
        }

        // Call expression: possibly defineComponent({ ... })
        Expression::CallExpression(call) => {
            let is_define_comp = match &call.callee {
                Expression::Identifier(id) => is_define_component(id.name.as_bytes()),
                _ => false,
            };

            if is_define_comp {
                // Check first argument for object
                let (object_span, setup_body_span) =
                    call.arguments.first().map_or((None, None), |arg| {
                        if let Some(Expression::ObjectExpression(obj)) = arg.as_expression() {
                            let setup_span =
                                find_setup_in_object(obj, ctx, items, errors, is_async);
                            (Some(ctx.adjust_span(obj.span)), setup_span)
                        } else {
                            (None, None)
                        }
                    });

                let mut default_export =
                    ScriptDefaultExport::new(span, DefaultExportType::DefineComponent);

                if let Some(obj_span) = object_span {
                    default_export = default_export.with_object_span(obj_span);
                }
                if let Some(setup_span) = setup_body_span {
                    default_export = default_export.with_setup_body_span(setup_span);
                }

                default_export
            } else {
                ScriptDefaultExport::new(span, DefaultExportType::Other)
            }
        }

        // Arrow function: export default () => { ... }
        Expression::ArrowFunctionExpression(_) => {
            ScriptDefaultExport::new(span, DefaultExportType::ArrowFunction)
        }

        // Function expression: export default function() { ... }
        Expression::FunctionExpression(_) => {
            ScriptDefaultExport::new(span, DefaultExportType::Function)
        }

        _ => ScriptDefaultExport::new(span, DefaultExportType::Other),
    }
}

/// Find setup function in an object expression and process its body
fn find_setup_in_object<'a>(
    obj: &ObjectExpression<'a>,
    ctx: &ScriptParseContext<'a>,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) -> Option<Span> {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            // Check if key is "setup"
            let is_setup = match &p.key {
                PropertyKey::StaticIdentifier(id) => id.name.as_bytes() == b"setup",
                PropertyKey::StringLiteral(s) => s.value.as_bytes() == b"setup",
                _ => false,
            };

            if is_setup {
                // Found setup - process its body
                return process_setup_value(&p.value, ctx, items, errors, is_async);
            }

            // Also check for method shorthand: setup() { ... }
            if p.method {
                let is_method_setup = match &p.key {
                    PropertyKey::StaticIdentifier(id) => id.name.as_bytes() == b"setup",
                    _ => false,
                };

                if is_method_setup {
                    return process_setup_value(&p.value, ctx, items, errors, is_async);
                }
            }
        }
    }

    None
}

/// Process the setup function value and return its body span
fn process_setup_value<'a>(
    value: &Expression<'a>,
    ctx: &ScriptParseContext<'a>,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
    is_async: &mut bool,
) -> Option<Span> {
    match value {
        Expression::FunctionExpression(func) => {
            if func.r#async {
                *is_async = true;
            }
            if let Some(body) = &func.body {
                // Process setup body like script setup (but without macros)
                let mut setup_ctx = SetupContext::new();
                process_setup_statements(&body.statements, ctx, &mut setup_ctx, items, errors);
                if setup_ctx.is_async {
                    *is_async = true;
                }
                Some(ctx.adjust_span(body.span))
            } else {
                None
            }
        }
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.r#async {
                *is_async = true;
            }
            // Arrow function body is always a FunctionBody struct
            // If arrow.expression is true, it's a single expression body
            if arrow.expression {
                // Expression body - no statements to process
                None
            } else {
                // Block body with statements
                let mut setup_ctx = SetupContext::new();
                process_setup_statements(
                    &arrow.body.statements,
                    ctx,
                    &mut setup_ctx,
                    items,
                    errors,
                );
                if setup_ctx.is_async {
                    *is_async = true;
                }
                Some(ctx.adjust_span(arrow.body.span))
            }
        }
        _ => None,
    }
}

/// Collect declarations from a binding pattern
fn collect_declarations_from_pattern<'a>(
    pattern: &BindingPattern<'a>,
    kind: DeclarationKind,
    ctx: &ScriptParseContext<'a>,
    items: &mut Vec<ScriptItem<'a>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            items.push(ScriptItem::Declaration(ScriptDeclaration {
                span: ctx.adjust_span(id.span),
                name: Some(id.name.as_str()),
                name_span: Some(ctx.adjust_span(id.span)),
                kind,
                is_ref_like: false,
            }));
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_declarations_from_pattern(&prop.value, kind, ctx, items);
            }
            if let Some(rest) = &obj.rest {
                collect_declarations_from_pattern(&rest.argument, kind, ctx, items);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_declarations_from_pattern(elem, kind, ctx, items);
            }
            if let Some(rest) = &arr.rest {
                collect_declarations_from_pattern(&rest.argument, kind, ctx, items);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_declarations_from_pattern(&assign.left, kind, ctx, items);
        }
    }
}
