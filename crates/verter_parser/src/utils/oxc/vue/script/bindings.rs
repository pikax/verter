//! Binding type classification for Vue `<script setup>`.
//!
//! Extracts binding metadata from the OXC AST to determine the correct
//! accessor prefix/suffix for template expressions. This follows Vue's
//! official compiler classification (see `BindingTypes` in `@vue/compiler-core`).

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, ImportDeclaration, ImportDeclarationSpecifier,
    ObjectPropertyKind, PropertyKey, Statement, TSTypeParameterInstantiation, VariableDeclaration,
    VariableDeclarationKind,
};

use super::resolve_type::{resolve_type_elements_with_ctx_ref, TypeResolutionContext};
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
///
/// `type_ctx` is the shared, companion-aware type-resolution context built once
/// for the whole setup parse. `macro_prop_keys`, when `Some`, supplies the
/// `defineProps<T>` prop-key spans the macro pass already resolved through the
/// shared resolver, so the binding pass reuses them instead of resolving the
/// macro type a second time. A `None` (standalone callers without a macro pass)
/// resolves the type parameter locally.
pub fn extract_bindings<'a>(
    program: &oxc_ast::ast::Program<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
    macro_prop_keys: Option<&[Span]>,
) -> Vec<(Span, BindingType)> {
    let mut entries = Vec::new();

    for stmt in &program.body {
        classify_statement(stmt, &mut entries, ctx, type_ctx, macro_prop_keys);
    }

    entries
}

/// Classify a single top-level statement.
fn classify_statement<'a>(
    stmt: &'a Statement<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
    macro_prop_keys: Option<&[Span]>,
) {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            classify_variable_declaration(decl, entries, ctx, type_ctx, macro_prop_keys);
        }
        Statement::ImportDeclaration(import) => {
            classify_import(import, entries, ctx);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            classify_expression_statement(
                &expr_stmt.expression,
                entries,
                ctx,
                type_ctx,
                macro_prop_keys,
            );
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                entries.push((Span::from(id.span), BindingType::SetupConst));
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                entries.push((Span::from(id.span), BindingType::SetupConst));
            }
        }
        Statement::TSEnumDeclaration(e) => {
            // Enums have a runtime JS representation
            entries.push((Span::from(e.id.span), BindingType::SetupConst));
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
    macro_prop_keys: Option<&[Span]>,
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
                    extract_individual_props_from_expr(
                        init,
                        entries,
                        ctx,
                        type_ctx,
                        macro_prop_keys,
                    );
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
    let callee_name = get_callee_name(&call.callee);

    match callee_name.as_deref() {
        Some("ref" | "computed" | "shallowRef" | "toRef" | "customRef" | "defineModel") => {
            BindingType::SetupRef
        }
        Some("reactive" | "shallowReactive") => BindingType::SetupReactiveConst,
        Some(name) if name.starts_with("use") => BindingType::SetupMaybeRef,
        _ => BindingType::SetupConst,
    }
}

/// Get the callee name from a call expression (simple identifiers only).
fn get_callee_name<'a>(callee: &Expression<'a>) -> Option<String> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Check if an expression is a `defineProps()` or `withDefaults()` call.
fn is_define_props_call<'a>(expr: &Expression<'a>) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            matches!(
                get_callee_name(&call.callee).as_deref(),
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

/// Extract binding names from a pattern and classify them.
fn extract_pattern_bindings<'a>(
    pattern: &BindingPattern<'a>,
    binding_type: BindingType,
    entries: &mut Vec<(Span, BindingType)>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            entries.push((Span::from(ident.span), binding_type));
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                extract_pattern_bindings(&prop.value, binding_type, entries);
            }
            if let Some(rest) = &obj.rest {
                extract_pattern_bindings(&rest.argument, binding_type, entries);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                extract_pattern_bindings(elem, binding_type, entries);
            }
            if let Some(rest) = &arr.rest {
                extract_pattern_bindings(&rest.argument, binding_type, entries);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            extract_pattern_bindings(&assign.left, binding_type, entries);
        }
    }
}

/// Classify standalone expression statements (e.g., `defineProps<{msg: string}>()`).
fn classify_expression_statement<'a>(
    expr: &'a Expression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'a, 'a>,
    macro_prop_keys: Option<&[Span]>,
) {
    if let Expression::CallExpression(call) = expr {
        let callee_name = get_callee_name(&call.callee);
        match callee_name.as_deref() {
            Some("defineProps") => {
                extract_props_from_define_props(call, entries, ctx, type_ctx, macro_prop_keys);
            }
            Some("withDefaults") => {
                if let Some(first_arg) = call.arguments.first() {
                    if let Some(Expression::CallExpression(inner_call)) = first_arg.as_expression()
                    {
                        extract_props_from_define_props(
                            inner_call,
                            entries,
                            ctx,
                            type_ctx,
                            macro_prop_keys,
                        );
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
    macro_prop_keys: Option<&[Span]>,
) {
    if let Expression::CallExpression(call) = expr {
        let callee_name = get_callee_name(&call.callee);
        match callee_name.as_deref() {
            Some("defineProps") => {
                extract_props_from_define_props(call, entries, ctx, type_ctx, macro_prop_keys);
            }
            Some("withDefaults") => {
                if let Some(first_arg) = call.arguments.first() {
                    if let Some(Expression::CallExpression(inner_call)) = first_arg.as_expression()
                    {
                        extract_props_from_define_props(
                            inner_call,
                            entries,
                            ctx,
                            type_ctx,
                            macro_prop_keys,
                        );
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
    macro_prop_keys: Option<&[Span]>,
) {
    // 1. Type parameters: defineProps<{ foo: string }>() or defineProps<MyInterface>()
    if let Some(type_args) = &call.type_arguments {
        extract_props_from_type_params(type_args, entries, ctx, type_ctx, macro_prop_keys);
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
    macro_prop_keys: Option<&[Span]>,
) {
    // The macro pass already resolved this `defineProps<T>` type argument through
    // the shared resolver. When those prop-key spans are supplied, reuse them
    // verbatim rather than resolving the type a second time — the binding pass
    // only needs the local prop-key spans (cross-file members carry no usable
    // setup-content span and are excluded by the caller).
    if let Some(keys) = macro_prop_keys {
        for key in keys {
            entries.push((*key, BindingType::Props));
        }
        return;
    }

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
/// Type-only imports (`import type { ... }`) and per-specifier type imports
/// (`import { type Foo }`) are skipped — they have no runtime binding.
fn classify_import<'a>(
    import: &ImportDeclaration<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    _ctx: &ScriptParseContext<'a>,
) {
    // Skip entire type-only imports: `import type { ... } from '...'`
    if import.import_kind.is_type() {
        return;
    }

    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    // Skip per-specifier type imports: `import { type Foo } from '...'`
                    if s.import_kind.is_type() {
                        continue;
                    }
                    entries.push((Span::from(s.local.span), BindingType::SetupImport));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    entries.push((Span::from(s.local.span), BindingType::SetupImport));
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    entries.push((Span::from(s.local.span), BindingType::SetupImport));
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
