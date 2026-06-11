//! Binding type classification for Vue `<script setup>`.
//!
//! Extracts binding metadata from the OXC AST to determine the correct
//! accessor prefix/suffix for template expressions. This follows Vue's
//! official compiler classification (see `BindingTypes` in `@vue/compiler-core`).
//!
//! The generic statement/pattern/import binding INVENTORY lives in the
//! framework-neutral [`crate::utils::oxc::script::bindings`] module; this
//! module owns only the Vue classification layer (macro awareness,
//! reactivity sniffing, `BindingType` mapping) and delegates the inventory
//! walks to the neutral module.

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, ImportDeclaration, ObjectPropertyKind, PropertyKey,
    Statement, TSTypeParameterInstantiation, VariableDeclaration, VariableDeclarationKind,
};

use crate::utils::oxc::script::bindings::{
    callee_identifier_name, collect_import_binding_spans, collect_pattern_binding_spans,
    declaration_binding_span,
};
use crate::utils::oxc::script::type_surface::{
    build_type_context, resolve_type_elements_with_ctx_ref, TypeResolutionContext,
};

use super::shared::ScriptParseContext;
use crate::common::Span;
use crate::types::BindingType;

/// Extract binding metadata from a parsed `<script setup>` program.
///
/// Walks top-level statements and classifies each binding:
/// - Variable declarations → based on initializer (ref, reactive, literal, etc.)
/// - Import declarations → `SetupConst` (type-only imports are skipped)
/// - Function/class/enum declarations → `SetupConst`
/// - TypeScript type/interface declarations → skipped (no runtime binding)
/// - Vue macros (defineProps, defineModel, etc.) → per-macro classification
pub fn extract_bindings<'a>(
    program: &oxc_ast::ast::Program<'a>,
    ctx: &ScriptParseContext<'a>,
) -> Vec<(Span, BindingType)> {
    let mut entries = Vec::new();
    let type_ctx = build_type_context(program, ctx.source_bytes, ctx.content_offset);

    for stmt in &program.body {
        classify_statement(stmt, &mut entries, ctx, &type_ctx);
    }

    entries
}

/// Classify a single top-level statement.
fn classify_statement<'a>(
    stmt: &'a Statement<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            classify_variable_declaration(decl, entries, ctx, type_ctx);
        }
        Statement::ImportDeclaration(import) => {
            classify_import(import, entries, ctx);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            classify_expression_statement(&expr_stmt.expression, entries, ctx, type_ctx);
        }
        // Function / class / enum declarations all bind a runtime value;
        // the neutral inventory yields the identifier span.
        Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSEnumDeclaration(_) => {
            if let Some(span) = declaration_binding_span(stmt) {
                entries.push((span, BindingType::SetupConst));
            }
        }
        // TypeScript-only declarations — no runtime binding
        Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => {}
        _ => {}
    }
}

/// Classify variable declarations: const/let/var.
fn classify_variable_declaration<'a>(
    decl: &'a VariableDeclaration<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    let is_const = decl.kind == VariableDeclarationKind::Const;

    for declarator in &decl.declarations {
        // Handle destructuring from defineProps
        if is_const {
            if let Some(init) = &declarator.init {
                if is_define_props_call(init) {
                    extract_destructured_props(&declarator.id, entries);
                    // Also extract individual prop names so the template can use $props.propName
                    // even when the whole object is assigned to a variable.
                    extract_individual_props_from_expr(init, entries, ctx, type_ctx);
                    continue;
                }
            }
        }

        let binding_type = if is_const {
            if let Some(init) = &declarator.init {
                classify_const_init(init)
            } else {
                BindingType::SetupConst
            }
        } else {
            BindingType::SetupLet
        };

        extract_pattern_bindings(&declarator.id, binding_type, entries);
    }
}

/// Classify the initializer of a `const` declaration.
///
/// Mirrors Vue's `walkDeclaration` + `canNeverBeRef` logic:
/// - Literal primitives → `LiteralConst`
/// - Call expressions → depends on callee (ref/computed/reactive/use*)
/// - Expressions that structurally can never be a ref (arrays, objects,
///   functions, classes, unary, binary, update, tagged template) → `SetupConst`
/// - Everything else (ternary, identifiers, member access, await, yield,
///   assignment, sequence, etc.) → `SetupMaybeRef` because the result
///   might be a ref at runtime
fn classify_const_init<'a>(init: &Expression<'a>) -> BindingType {
    match init {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => BindingType::LiteralConst,

        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => BindingType::LiteralConst,

        Expression::CallExpression(call) => classify_call_expression(call),

        // Expressions that can never be a ref → SetupConst
        // (matches Vue's canNeverBeRef())
        Expression::UnaryExpression(_)
        | Expression::BinaryExpression(_)
        | Expression::ArrayExpression(_)
        | Expression::ObjectExpression(_)
        | Expression::FunctionExpression(_)
        | Expression::ArrowFunctionExpression(_)
        | Expression::UpdateExpression(_)
        | Expression::ClassExpression(_)
        | Expression::TaggedTemplateExpression(_) => BindingType::SetupConst,

        // For SequenceExpression, check last expression
        Expression::SequenceExpression(seq) => {
            if let Some(last) = seq.expressions.last() {
                if can_never_be_ref(last) {
                    BindingType::SetupConst
                } else {
                    BindingType::SetupMaybeRef
                }
            } else {
                BindingType::SetupMaybeRef
            }
        }

        // Everything else (ternary, identifiers, member access, await,
        // yield, assignment, etc.) might evaluate to a ref
        _ => BindingType::SetupMaybeRef,
    }
}

/// Check if an expression structurally can never produce a ref value.
/// Mirrors Vue's `canNeverBeRef()` from `@vue/compiler-sfc`.
fn can_never_be_ref<'a>(expr: &Expression<'a>) -> bool {
    matches!(
        expr,
        Expression::UnaryExpression(_)
            | Expression::BinaryExpression(_)
            | Expression::ArrayExpression(_)
            | Expression::ObjectExpression(_)
            | Expression::FunctionExpression(_)
            | Expression::ArrowFunctionExpression(_)
            | Expression::UpdateExpression(_)
            | Expression::ClassExpression(_)
            | Expression::TaggedTemplateExpression(_)
    )
}

/// Classify a call expression in a const initializer.
fn classify_call_expression<'a>(call: &CallExpression<'a>) -> BindingType {
    let callee_name = callee_identifier_name(&call.callee);

    match callee_name.as_deref() {
        Some("ref" | "computed" | "shallowRef" | "toRef" | "customRef" | "defineModel") => {
            BindingType::SetupRef
        }
        Some("reactive" | "shallowReactive") => BindingType::SetupReactiveConst,
        Some(name) if name.starts_with("use") => BindingType::SetupMaybeRef,
        _ => BindingType::SetupConst,
    }
}

/// Check if an expression is a `defineProps()` or `withDefaults()` call.
fn is_define_props_call<'a>(expr: &Expression<'a>) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            matches!(
                callee_identifier_name(&call.callee).as_deref(),
                Some("defineProps" | "withDefaults")
            )
        }
        _ => false,
    }
}

/// Extract destructured props: `const { msg: m } = defineProps<...>()`
///
/// For an object pattern, each binding is `PropsAliased`.
/// For a plain identifier (`const props = defineProps()`), the binding is `SetupConst`
/// because the variable holds the whole props object — accessed via `$setup.props`
/// in standalone mode, not `$props.props`.
fn extract_destructured_props<'a>(
    pattern: &BindingPattern<'a>,
    entries: &mut Vec<(Span, BindingType)>,
) {
    match pattern {
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                extract_pattern_bindings(&prop.value, BindingType::PropsAliased, entries);
            }
            if let Some(rest) = &obj.rest {
                extract_pattern_bindings(&rest.argument, BindingType::PropsAliased, entries);
            }
        }
        BindingPattern::BindingIdentifier(ident) => {
            // Plain identifier `const props = defineProps()` — the variable IS the
            // props object itself. It's a setup binding, not an individual prop.
            entries.push((Span::from(ident.span), BindingType::SetupConst));
        }
        _ => {}
    }
}

/// Extract binding names from a pattern and classify them all as
/// `binding_type` (the neutral inventory walks the pattern; this layer
/// only attaches the Vue classification).
fn extract_pattern_bindings<'a>(
    pattern: &BindingPattern<'a>,
    binding_type: BindingType,
    entries: &mut Vec<(Span, BindingType)>,
) {
    let mut spans = Vec::new();
    collect_pattern_binding_spans(pattern, &mut spans);
    entries.extend(spans.into_iter().map(|span| (span, binding_type)));
}

/// Classify standalone expression statements (e.g., `defineProps<{msg: string}>()`).
fn classify_expression_statement<'a>(
    expr: &'a Expression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    if let Expression::CallExpression(call) = expr {
        let callee_name = callee_identifier_name(&call.callee);
        match callee_name.as_deref() {
            Some("defineProps") => {
                extract_props_from_define_props(call, entries, ctx, type_ctx);
            }
            Some("withDefaults") => {
                if let Some(first_arg) = call.arguments.first() {
                    if let Some(Expression::CallExpression(inner_call)) = first_arg.as_expression()
                    {
                        extract_props_from_define_props(inner_call, entries, ctx, type_ctx);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract individual prop names from a defineProps() or withDefaults() call expression.
/// Called for variable declarations (`const props = defineProps<{...}>()`) to ensure
/// individual prop names are classified as Props alongside the whole-object binding.
fn extract_individual_props_from_expr<'a>(
    expr: &'a Expression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    if let Expression::CallExpression(call) = expr {
        let callee_name = callee_identifier_name(&call.callee);
        match callee_name.as_deref() {
            Some("defineProps") => {
                extract_props_from_define_props(call, entries, ctx, type_ctx);
            }
            Some("withDefaults") => {
                if let Some(first_arg) = call.arguments.first() {
                    if let Some(Expression::CallExpression(inner_call)) = first_arg.as_expression()
                    {
                        extract_props_from_define_props(inner_call, entries, ctx, type_ctx);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract individual prop names from a `defineProps()` call.
/// Handles all syntactic variants:
/// - Type params: `defineProps<{ foo: string }>()` or `defineProps<MyInterface>()`
/// - Runtime object: `defineProps({ foo: String })`
/// - Runtime array: `defineProps(['foo', 'bar'])`
fn extract_props_from_define_props<'a>(
    call: &'a CallExpression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    // 1. Type parameters: defineProps<{ foo: string }>() or defineProps<MyInterface>()
    if let Some(type_args) = &call.type_arguments {
        extract_props_from_type_params(type_args, entries, ctx, type_ctx);
        return;
    }

    // 2. Runtime arguments: defineProps({ foo: String }) or defineProps(['foo'])
    if let Some(first_arg) = call.arguments.first() {
        if let Some(expr) = first_arg.as_expression() {
            extract_props_from_runtime_arg(expr, entries, ctx);
        }
    }
}

/// Extract prop names from TypeScript type parameters of `defineProps`.
/// Handles inline type literals, type references (interfaces/aliases), and other TS constructs.
fn extract_props_from_type_params<'a>(
    type_params: &'a TSTypeParameterInstantiation<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
) {
    if let Some(first_param) = type_params.params.first() {
        // Use the type resolution infrastructure to resolve all type variants:
        // TSTypeLiteral, TSTypeReference (interfaces/aliases), unions, intersections, etc.
        //
        // `defineProps<T>()` is a top-level macro entry: T IS the macro
        // T's own body, so members reached through it get
        // `declared_in_macro_type_arg = true` (subject to the
        // heritage flip semantics inside the resolver).
        let resolved =
            resolve_type_elements_with_ctx_ref(first_param, ctx.content_offset, type_ctx, true);
        for prop in &resolved.props {
            entries.push((prop.key, BindingType::Props));
        }
    }
}

/// Extract prop names from runtime defineProps arguments.
/// Handles object syntax `{ foo: String }` and array syntax `['foo', 'bar']`.
fn extract_props_from_runtime_arg<'a>(
    expr: &Expression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    match expr {
        Expression::ObjectExpression(obj) => {
            for prop_kind in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop_kind {
                    if let PropertyKey::StaticIdentifier(ident) = &p.key {
                        // StaticIdentifier spans are NOT adjusted by adjust_program_spans()
                        // (only expression-convertible keys are), so add content_offset.
                        let offset = ctx.content_offset;
                        entries.push((
                            Span::new(ident.span.start + offset, ident.span.end + offset),
                            BindingType::Props,
                        ));
                    }
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                // Array syntax: defineProps(['foo', 'bar'])
                // StringLiteral elements ARE adjusted by adjust_program_spans().
                // Spans include quotes, so adjust +1/-1 to get the bare name.
                if let Some(Expression::StringLiteral(s)) = elem.as_expression() {
                    entries.push((
                        Span::new(s.span.start + 1, s.span.end - 1),
                        BindingType::Props,
                    ));
                }
            }
        }
        _ => {}
    }
}

/// Classify import declarations.
///
/// The neutral inventory yields the runtime binding spans (type-only
/// imports and per-specifier type imports bind nothing); every runtime
/// import binding classifies as `SetupImport`.
fn classify_import<'a>(
    import: &ImportDeclaration<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    _ctx: &ScriptParseContext<'a>,
) {
    let mut spans = Vec::new();
    collect_import_binding_spans(import, &mut spans);
    entries.extend(
        spans
            .into_iter()
            .map(|span| (span, BindingType::SetupImport)),
    );
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
