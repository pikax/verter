//! Options API script parsing.
//!
//! This module handles parsing of `<script>` blocks (without setup attribute),
//! detecting export default and extracting component options.

#![allow(unused_imports)]
#![allow(unused_variables)]

use oxc_ast::ast::*;

use super::macros::is_define_component;
use super::resolve_type::TypeResolutionContext;
use super::setup::{process_setup_statements, SetupContext};
use super::shared::ScriptParseContext;
use super::types::{
    DeclarationKind, DefaultExportType, ScriptDeclaration, ScriptDefaultExport, ScriptError,
    ScriptItem,
};
use crate::common::Span;
use crate::template::code_gen::binding::BindingType;

/// Context for options script parsing
pub struct OptionsContext;

impl Default for OptionsContext {
    fn default() -> Self {
        Self::new()
    }
}

impl OptionsContext {
    pub fn new() -> Self {
        Self
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
                collect_declarations_from_pattern(&declarator.id, kind, items);
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
                    span: Span::from(func.span),
                    name: Some(id.name.as_str()),
                    name_span: Some(Span::from(id.span)),
                    kind,
                    is_ref_like: false,
                }));
            }
        }

        // Track file-scoped class declarations
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                items.push(ScriptItem::Declaration(ScriptDeclaration {
                    span: Span::from(class.span),
                    name: Some(id.name.as_str()),
                    name_span: Some(Span::from(id.span)),
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
    let span = Span::from(export.span);

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
                .with_object_span(Span::from(obj.span));

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
                            (Some(Span::from(obj.span)), setup_span)
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
                let empty_type_ctx = TypeResolutionContext::new(ctx.source_bytes);
                let mut setup_ctx = SetupContext::new();
                process_setup_statements(
                    &body.statements,
                    ctx,
                    &empty_type_ctx,
                    &mut setup_ctx,
                    items,
                    errors,
                );
                if setup_ctx.is_async {
                    *is_async = true;
                }
                Some(Span::from(body.span))
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
                let empty_type_ctx = TypeResolutionContext::new(ctx.source_bytes);
                let mut setup_ctx = SetupContext::new();
                process_setup_statements(
                    &arrow.body.statements,
                    ctx,
                    &empty_type_ctx,
                    &mut setup_ctx,
                    items,
                    errors,
                );
                if setup_ctx.is_async {
                    *is_async = true;
                }
                Some(Span::from(arrow.body.span))
            }
        }
        _ => None,
    }
}

// ======================== Options API binding extraction ========================

/// Extract binding types from Options API default export.
///
/// Walks the `export default { ... }` or `export default defineComponent({ ... })`
/// object and extracts property names from `data()`, `props`, `computed`, `methods`,
/// and `inject`, returning them with their corresponding `BindingType`.
///
/// Spans are OXC-relative (0-based within the parsed content string).
pub fn extract_options_bindings<'a>(program: &Program<'a>) -> Vec<(Span, BindingType)> {
    let mut bindings = Vec::new();
    for stmt in &program.body {
        if let Statement::ExportDefaultDeclaration(export) = stmt {
            if let Some(expr) = export.declaration.as_expression() {
                extract_from_default_expression(expr, &mut bindings);
            }
        }
    }
    bindings
}

fn extract_from_default_expression(expr: &Expression<'_>, bindings: &mut Vec<(Span, BindingType)>) {
    // Only extract bindings from plain object exports: `export default { ... }`.
    // Vue's compiler does NOT perform binding analysis for `defineComponent({ ... })`
    // wrapped exports — those use `_ctx.` uniformly.
    if let Expression::ObjectExpression(obj) = expr {
        extract_from_options_object(obj, bindings);
    }
}

fn extract_from_options_object(
    obj: &ObjectExpression<'_>,
    bindings: &mut Vec<(Span, BindingType)>,
) {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            let key_name = match &p.key {
                PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
                PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
                _ => None,
            };

            match key_name {
                Some("data") => {
                    extract_data_return_keys(&p.value, bindings);
                }
                Some("props") => {
                    extract_props_keys(&p.value, bindings);
                }
                Some("computed") => {
                    extract_object_property_keys(&p.value, BindingType::Options, bindings);
                }
                Some("methods") => {
                    extract_object_property_keys(&p.value, BindingType::Options, bindings);
                }
                Some("inject") => {
                    extract_inject_keys(&p.value, bindings);
                }
                _ => {}
            }
        }
    }
}

/// Extract keys from the object returned by `data()`.
fn extract_data_return_keys(value: &Expression<'_>, bindings: &mut Vec<(Span, BindingType)>) {
    match value {
        // data() { return { ... } } or data: function() { return { ... } }
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                extract_return_object_keys(&body.statements, BindingType::Data, bindings);
            }
        }
        // data: () => ({ ... })
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                // Expression body: the first statement is the expression
                for stmt in &arrow.body.statements {
                    if let Statement::ExpressionStatement(es) = stmt {
                        // Unwrap parenthesized: (({...}))
                        let inner = unwrap_parens(&es.expression);
                        if let Expression::ObjectExpression(obj) = inner {
                            extract_ident_keys_from_object(obj, BindingType::Data, bindings);
                        }
                    }
                }
            } else {
                extract_return_object_keys(&arrow.body.statements, BindingType::Data, bindings);
            }
        }
        _ => {}
    }
}

/// Walk statements looking for `return { ... }` and extract object keys.
fn extract_return_object_keys(
    stmts: &[Statement<'_>],
    bt: BindingType,
    bindings: &mut Vec<(Span, BindingType)>,
) {
    for stmt in stmts {
        if let Statement::ReturnStatement(ret) = stmt {
            if let Some(expr) = &ret.argument {
                let inner = unwrap_parens(expr);
                if let Expression::ObjectExpression(obj) = inner {
                    extract_ident_keys_from_object(obj, bt, bindings);
                }
            }
        }
    }
}

/// Extract property names from an object expression. Only extracts StaticIdentifier
/// keys (not computed or string literal keys, which are rare for data/computed/methods).
fn extract_ident_keys_from_object(
    obj: &ObjectExpression<'_>,
    bt: BindingType,
    bindings: &mut Vec<(Span, BindingType)>,
) {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let PropertyKey::StaticIdentifier(id) = &p.key {
                bindings.push((Span::from(id.span), bt));
            }
        }
    }
}

/// Extract props keys from `props: [...]` or `props: { ... }`.
fn extract_props_keys(value: &Expression<'_>, bindings: &mut Vec<(Span, BindingType)>) {
    match value {
        // props: ['foo', 'bar']
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    // String literal span includes quotes — adjust to content only
                    let span = Span {
                        start: s.span.start + 1,
                        end: s.span.end - 1,
                    };
                    if span.end > span.start {
                        bindings.push((span, BindingType::Props));
                    }
                }
            }
        }
        // props: { foo: String, bar: { type: Number } }
        Expression::ObjectExpression(obj) => {
            extract_ident_keys_from_object(obj, BindingType::Props, bindings);
        }
        _ => {}
    }
}

/// Extract keys from object expression (for computed/methods).
fn extract_object_property_keys(
    value: &Expression<'_>,
    bt: BindingType,
    bindings: &mut Vec<(Span, BindingType)>,
) {
    if let Expression::ObjectExpression(obj) = value {
        extract_ident_keys_from_object(obj, bt, bindings);
    }
}

/// Extract inject keys from `inject: [...]` or `inject: { ... }`.
fn extract_inject_keys(value: &Expression<'_>, bindings: &mut Vec<(Span, BindingType)>) {
    match value {
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let ArrayExpressionElement::StringLiteral(s) = elem {
                    let span = Span {
                        start: s.span.start + 1,
                        end: s.span.end - 1,
                    };
                    if span.end > span.start {
                        bindings.push((span, BindingType::Options));
                    }
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            extract_ident_keys_from_object(obj, BindingType::Options, bindings);
        }
        _ => {}
    }
}

/// Unwrap parenthesized expressions: `((expr))` → `expr`.
fn unwrap_parens<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(p) => unwrap_parens(&p.expression),
        _ => expr,
    }
}

// ======================== Statement helpers ========================

/// Collect declarations from a binding pattern
fn collect_declarations_from_pattern<'a>(
    pattern: &BindingPattern<'a>,
    kind: DeclarationKind,
    items: &mut Vec<ScriptItem<'a>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            items.push(ScriptItem::Declaration(ScriptDeclaration {
                span: Span::from(id.span),
                name: Some(id.name.as_str()),
                name_span: Some(Span::from(id.span)),
                kind,
                is_ref_like: false,
            }));
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_declarations_from_pattern(&prop.value, kind, items);
            }
            if let Some(rest) = &obj.rest {
                collect_declarations_from_pattern(&rest.argument, kind, items);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_declarations_from_pattern(elem, kind, items);
            }
            if let Some(rest) = &arr.rest {
                collect_declarations_from_pattern(&rest.argument, kind, items);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_declarations_from_pattern(&assign.left, kind, items);
        }
    }
}
