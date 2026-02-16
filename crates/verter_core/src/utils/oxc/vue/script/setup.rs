//! Script setup mode parsing.
//!
//! This module handles parsing of `<script setup>` blocks,
//! extracting macros, declarations, and async markers.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::macros::{
    detect_macro_kind, MacroArrayArg, MacroDeclarator, MacroObjectArg, MacroProperty,
    MacroTypeParams, ScriptMacro, VueMacroKind,
};
use super::resolve_type::{
    infer_runtime_type, resolve_type_elements_with_ctx_ref, ResolvedElements, TypeResolutionContext,
};
use super::shared::ScriptParseContext;
use super::types::{
    DeclarationKind, ScriptAsync, ScriptBinding, ScriptDeclaration, ScriptError, ScriptErrorKind,
    ScriptItem, ScriptTypeDeclaration, TypeDeclarationKind,
};
use super::usage::{
    detect_vue_api_call, CallSiteContext, EmitCallUsage, EmitEventName, InjectUsage, LifecycleHook,
    LifecycleUsage, ProvideKey, ProvideKeyKind, ProvideUsage, ReactiveKind, ReactiveStateUsage,
    SyncContextUsage, TemplateUtilUsage, UsageCollector, VueApiCategory, VueApiKind, WatcherUsage,
};
use crate::common::Span;

/// Context for setup script parsing
pub struct SetupContext {
    /// Whether we've seen an await (makes script async)
    pub is_async: bool,
    /// Track if we're inside a function (macros should warn there)
    function_depth: u32,
    /// Track if we're inside a block that shouldn't track declarations
    block_depth: u32,
}

impl Default for SetupContext {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupContext {
    pub fn new() -> Self {
        Self {
            is_async: false,
            function_depth: 0,
            block_depth: 0,
        }
    }

    /// Check if we should track declarations at current depth (top-level only)
    fn should_track_declarations(&self) -> bool {
        self.block_depth == 0
    }

    /// Enter a function body
    pub fn enter_function(&mut self) {
        self.function_depth += 1;
    }

    /// Leave a function body
    pub fn leave_function(&mut self) {
        self.function_depth = self.function_depth.saturating_sub(1);
    }

    /// Enter a block scope
    pub fn enter_block(&mut self) {
        self.block_depth += 1;
    }

    /// Leave a block scope
    pub fn leave_block(&mut self) {
        self.block_depth = self.block_depth.saturating_sub(1);
    }
}

/// Process statements for setup mode and collect items
pub fn process_setup_statements<'a>(
    statements: &[Statement<'a>],
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
) {
    for stmt in statements {
        process_setup_statement(stmt, ctx, type_ctx, setup_ctx, items, errors);
    }
}

/// Process a single statement in setup mode
pub fn process_setup_statement<'a>(
    stmt: &Statement<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
    errors: &mut Vec<ScriptError>,
) {
    match stmt {
        // Skip imports - handled separately by shared
        Statement::ImportDeclaration(_) => {}

        // Variable declarations
        Statement::VariableDeclaration(var_decl) => {
            // `declare const/let/var` has no runtime value — treat as type declaration
            if var_decl.declare {
                let name = var_decl
                    .declarations
                    .first()
                    .and_then(|d| extract_binding_name(&d.id));
                items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                    span: Span::from(var_decl.span),
                    name,
                    kind: TypeDeclarationKind::TypeAlias,
                }));
            } else {
                process_variable_declaration(var_decl, ctx, type_ctx, setup_ctx, items);
            }
        }

        // Function declarations
        Statement::FunctionDeclaration(func) => {
            // `declare function` has no runtime value — treat as type declaration
            if func.declare {
                let name = func.id.as_ref().map(|id| id.name.as_str());
                items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                    span: Span::from(func.span),
                    name,
                    kind: TypeDeclarationKind::TypeAlias,
                }));
            } else {
                process_function_declaration(func, ctx, setup_ctx, items);
            }
        }

        // Class declarations
        Statement::ClassDeclaration(class) => {
            // `declare class` has no runtime value — treat as type declaration
            if class.declare {
                let name = class.id.as_ref().map(|id| id.name.as_str());
                items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                    span: Span::from(class.span),
                    name,
                    kind: TypeDeclarationKind::TypeAlias,
                }));
            } else if setup_ctx.should_track_declarations() {
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
        }

        // Expression statements (may contain macro calls, await, etc.)
        Statement::ExpressionStatement(expr_stmt) => {
            process_expression_statement(expr_stmt, ctx, type_ctx, setup_ctx, items);
        }

        // Block statements that prevent declaration tracking
        Statement::BlockStatement(block) => {
            setup_ctx.enter_block();
            process_setup_statements(&block.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::IfStatement(if_stmt) => {
            // Check condition for await
            check_expression_for_async(&if_stmt.test, setup_ctx, items);

            setup_ctx.enter_block();
            process_setup_statement(&if_stmt.consequent, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();

            if let Some(alt) = &if_stmt.alternate {
                setup_ctx.enter_block();
                process_setup_statement(alt, ctx, type_ctx, setup_ctx, items, errors);
                setup_ctx.leave_block();
            }
        }

        Statement::ForStatement(for_stmt) => {
            setup_ctx.enter_block();
            process_setup_statement(&for_stmt.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::ForInStatement(for_in) => {
            setup_ctx.enter_block();
            process_setup_statement(&for_in.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::ForOfStatement(for_of) => {
            // for await ... of
            if for_of.r#await {
                setup_ctx.is_async = true;
                items.push(ScriptItem::Async(ScriptAsync {
                    span: Span::from(for_of.span),
                }));
            }
            setup_ctx.enter_block();
            process_setup_statement(&for_of.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::WhileStatement(while_stmt) => {
            setup_ctx.enter_block();
            process_setup_statement(&while_stmt.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::DoWhileStatement(do_while) => {
            setup_ctx.enter_block();
            process_setup_statement(&do_while.body, ctx, type_ctx, setup_ctx, items, errors);
            setup_ctx.leave_block();
        }

        Statement::SwitchStatement(switch_stmt) => {
            setup_ctx.enter_block();
            for case in &switch_stmt.cases {
                for stmt in &case.consequent {
                    process_setup_statement(stmt, ctx, type_ctx, setup_ctx, items, errors);
                }
            }
            setup_ctx.leave_block();
        }

        Statement::TryStatement(try_stmt) => {
            setup_ctx.enter_block();
            process_setup_statements(
                &try_stmt.block.body,
                ctx,
                type_ctx,
                setup_ctx,
                items,
                errors,
            );
            setup_ctx.leave_block();

            if let Some(handler) = &try_stmt.handler {
                setup_ctx.enter_block();
                process_setup_statements(
                    &handler.body.body,
                    ctx,
                    type_ctx,
                    setup_ctx,
                    items,
                    errors,
                );
                setup_ctx.leave_block();
            }

            if let Some(finalizer) = &try_stmt.finalizer {
                setup_ctx.enter_block();
                process_setup_statements(&finalizer.body, ctx, type_ctx, setup_ctx, items, errors);
                setup_ctx.leave_block();
            }
        }

        // Errors in setup mode
        Statement::ExportDefaultDeclaration(export) => {
            errors.push(ScriptError {
                span: Span::from(export.span),
                message: ScriptErrorKind::ExportDefaultInSetup,
            });
        }

        Statement::ReturnStatement(ret) => {
            errors.push(ScriptError {
                span: Span::from(ret.span),
                message: ScriptErrorKind::ReturnInSetup,
            });
        }

        // Named exports in setup are allowed (for type exports).
        // Unwrap exported type declarations so they get stripped from output.
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = &export.declaration {
                // Use export's outer span so `export` keyword is also removed
                let export_span = Span::from(export.span);
                match decl {
                    Declaration::TSTypeAliasDeclaration(alias) => {
                        items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                            span: export_span,
                            name: Some(alias.id.name.as_str()),
                            kind: TypeDeclarationKind::TypeAlias,
                        }));
                    }
                    Declaration::TSInterfaceDeclaration(interface) => {
                        items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                            span: export_span,
                            name: Some(interface.id.name.as_str()),
                            kind: TypeDeclarationKind::Interface,
                        }));
                    }
                    Declaration::TSEnumDeclaration(ts_enum) => {
                        items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                            span: export_span,
                            name: Some(ts_enum.id.name.as_str()),
                            kind: TypeDeclarationKind::Enum,
                        }));
                    }
                    _ => {}
                }
            }
        }
        Statement::ExportAllDeclaration(_) => {}

        // TypeScript-only declarations - need to be moved outside the component
        Statement::TSTypeAliasDeclaration(type_alias) => {
            items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                span: Span::from(type_alias.span),
                name: Some(type_alias.id.name.as_str()),
                kind: TypeDeclarationKind::TypeAlias,
            }));
        }

        Statement::TSInterfaceDeclaration(interface) => {
            items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                span: Span::from(interface.span),
                name: Some(interface.id.name.as_str()),
                kind: TypeDeclarationKind::Interface,
            }));
        }

        Statement::TSEnumDeclaration(ts_enum) => {
            items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                span: Span::from(ts_enum.span),
                name: Some(ts_enum.id.name.as_str()),
                kind: TypeDeclarationKind::Enum,
            }));
        }

        Statement::TSModuleDeclaration(module) => {
            let name = match &module.id {
                oxc_ast::ast::TSModuleDeclarationName::Identifier(id) => Some(id.name.as_str()),
                oxc_ast::ast::TSModuleDeclarationName::StringLiteral(s) => Some(s.value.as_str()),
            };
            items.push(ScriptItem::TypeDeclaration(ScriptTypeDeclaration {
                span: Span::from(module.span),
                name,
                kind: TypeDeclarationKind::Module,
            }));
        }

        // Other statements
        _ => {}
    }
}

/// Check if an expression is a call to a ref-creating Vue API.
/// These return a Ref wrapper, so inline template mode needs `.value`.
fn is_ref_creating_call(init: &Expression<'_>) -> bool {
    if let Expression::CallExpression(call) = init {
        if let Expression::Identifier(id) = &call.callee {
            return matches!(
                id.name.as_str(),
                "ref" | "computed" | "shallowRef" | "customRef" | "toRef"
            );
        }
    }
    false
}

/// Process a variable declaration
fn process_variable_declaration<'a>(
    var_decl: &VariableDeclaration<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
) {
    let kind = match var_decl.kind {
        VariableDeclarationKind::Const => DeclarationKind::Const,
        VariableDeclarationKind::Let => DeclarationKind::Let,
        VariableDeclarationKind::Var => DeclarationKind::Var,
        VariableDeclarationKind::Using => DeclarationKind::Const,
        VariableDeclarationKind::AwaitUsing => {
            setup_ctx.is_async = true;
            items.push(ScriptItem::Async(ScriptAsync {
                span: Span::from(var_decl.span),
            }));
            DeclarationKind::Const
        }
    };

    for declarator in &var_decl.declarations {
        // Detect if initializer is a ref-creating call (only relevant for const)
        let is_ref_like = kind == DeclarationKind::Const
            && declarator
                .init
                .as_ref()
                .is_some_and(|init| is_ref_creating_call(init));

        // Check if init is a macro call
        if let Some(init) = &declarator.init {
            // Check for await in init
            check_expression_for_async(init, setup_ctx, items);

            // Build declarator info for macro
            let macro_declarator = Some(MacroDeclarator {
                name: extract_binding_name(&declarator.id),
                binding_span: Span::from(declarator.id.span()),
                statement_span: Span::from(var_decl.span),
            });

            // Check if init is a macro call
            if let Some(macro_item) =
                try_parse_macro_from_expression(init, ctx, type_ctx, macro_declarator)
            {
                items.push(ScriptItem::Macro(macro_item));
            }
        }

        // Track declarations at top level
        if setup_ctx.should_track_declarations() {
            collect_declarations_from_pattern(&declarator.id, kind, is_ref_like, items);
        }
    }
}

/// Extract the binding name from a pattern (only for simple identifiers)
fn extract_binding_name<'a>(pattern: &BindingPattern<'a>) -> Option<&'a str> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
        _ => None, // Destructuring patterns don't have a single name
    }
}

/// Process a function declaration
fn process_function_declaration<'a>(
    func: &Function<'a>,
    ctx: &ScriptParseContext<'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
) {
    if setup_ctx.should_track_declarations() {
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

    // Don't recurse into function body for declarations,
    // but we still track that we're in a function for macro warnings
}

/// Process an expression statement
fn process_expression_statement<'a>(
    expr_stmt: &ExpressionStatement<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
) {
    // Check for await expressions
    check_expression_for_async(&expr_stmt.expression, setup_ctx, items);

    // Check for macro calls at expression level (no declarator for standalone expressions)
    if let Some(macro_item) =
        try_parse_macro_from_expression(&expr_stmt.expression, ctx, type_ctx, None)
    {
        items.push(ScriptItem::Macro(macro_item));
    }
}

/// Check an expression for await and mark as async if found
fn check_expression_for_async<'a>(
    expr: &Expression<'a>,
    setup_ctx: &mut SetupContext,
    items: &mut Vec<ScriptItem<'a>>,
) {
    match expr {
        Expression::AwaitExpression(await_expr) => {
            setup_ctx.is_async = true;
            items.push(ScriptItem::Async(ScriptAsync {
                span: Span::from(await_expr.span),
            }));
            // Also check the argument
            check_expression_for_async(&await_expr.argument, setup_ctx, items);
        }
        Expression::CallExpression(call) => {
            check_expression_for_async(&call.callee, setup_ctx, items);
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    check_expression_for_async(&spread.argument, setup_ctx, items);
                } else if let Some(expr) = arg.as_expression() {
                    check_expression_for_async(expr, setup_ctx, items);
                }
            }
        }
        Expression::BinaryExpression(bin) => {
            check_expression_for_async(&bin.left, setup_ctx, items);
            check_expression_for_async(&bin.right, setup_ctx, items);
        }
        Expression::ConditionalExpression(cond) => {
            check_expression_for_async(&cond.test, setup_ctx, items);
            check_expression_for_async(&cond.consequent, setup_ctx, items);
            check_expression_for_async(&cond.alternate, setup_ctx, items);
        }
        Expression::AssignmentExpression(assign) => {
            check_expression_for_async(&assign.right, setup_ctx, items);
        }
        Expression::ParenthesizedExpression(paren) => {
            check_expression_for_async(&paren.expression, setup_ctx, items);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                check_expression_for_async(expr, setup_ctx, items);
            }
        }
        Expression::UnaryExpression(unary) => {
            check_expression_for_async(&unary.argument, setup_ctx, items);
        }
        Expression::LogicalExpression(logical) => {
            check_expression_for_async(&logical.left, setup_ctx, items);
            check_expression_for_async(&logical.right, setup_ctx, items);
        }
        Expression::ComputedMemberExpression(computed) => {
            check_expression_for_async(&computed.object, setup_ctx, items);
            check_expression_for_async(&computed.expression, setup_ctx, items);
        }
        Expression::StaticMemberExpression(static_member) => {
            check_expression_for_async(&static_member.object, setup_ctx, items);
        }
        Expression::PrivateFieldExpression(private) => {
            check_expression_for_async(&private.object, setup_ctx, items);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let ArrayExpressionElement::SpreadElement(spread) = elem {
                    check_expression_for_async(&spread.argument, setup_ctx, items);
                } else if let Some(expr) = elem.as_expression() {
                    check_expression_for_async(expr, setup_ctx, items);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(p) => {
                        check_expression_for_async(&p.value, setup_ctx, items);
                    }
                    ObjectPropertyKind::SpreadProperty(spread) => {
                        check_expression_for_async(&spread.argument, setup_ctx, items);
                    }
                }
            }
        }
        Expression::NewExpression(new_expr) => {
            check_expression_for_async(&new_expr.callee, setup_ctx, items);
            for arg in &new_expr.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    check_expression_for_async(&spread.argument, setup_ctx, items);
                } else if let Some(expr) = arg.as_expression() {
                    check_expression_for_async(expr, setup_ctx, items);
                }
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            check_expression_for_async(&tagged.tag, setup_ctx, items);
        }
        Expression::TemplateLiteral(template) => {
            for expr in &template.expressions {
                check_expression_for_async(expr, setup_ctx, items);
            }
        }
        Expression::YieldExpression(yield_expr) => {
            if let Some(arg) = &yield_expr.argument {
                check_expression_for_async(arg, setup_ctx, items);
            }
        }
        // Don't recurse into function expressions - they have their own async context
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => {}
        _ => {}
    }
}

/// Try to parse a macro from an expression
fn try_parse_macro_from_expression<'a>(
    expr: &Expression<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    declarator: Option<MacroDeclarator<'a>>,
) -> Option<ScriptMacro<'a>> {
    match expr {
        Expression::CallExpression(call) => parse_macro_call(call, ctx, type_ctx, declarator),
        _ => None,
    }
}

/// Parse a CallExpression into a ScriptMacro enum variant
pub fn parse_macro_call<'a>(
    call: &CallExpression<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
    declarator: Option<MacroDeclarator<'a>>,
) -> Option<ScriptMacro<'a>> {
    // Get callee name as bytes
    let name = match &call.callee {
        Expression::Identifier(id) => id.name.as_bytes(),
        _ => return None,
    };

    let kind = detect_macro_kind(name)?;
    let span = Span::from(call.span);

    // Extract type parameters if present
    let type_params = call
        .type_arguments
        .as_ref()
        .map(|tp| extract_type_params(tp, ctx, type_ctx));

    match kind {
        VueMacroKind::DefineProps => {
            let (object_arg, array_arg) = extract_arg_spans(call, ctx);
            Some(ScriptMacro::DefineProps {
                span,
                declarator,
                type_params,
                object_arg,
                array_arg,
            })
        }
        VueMacroKind::DefineEmits => {
            let (object_arg, array_arg) = extract_arg_spans(call, ctx);
            Some(ScriptMacro::DefineEmits {
                span,
                declarator,
                type_params,
                object_arg,
                array_arg,
            })
        }
        VueMacroKind::DefineExpose => {
            let object_arg = extract_object_arg_from_call(call, 0, ctx);
            Some(ScriptMacro::DefineExpose {
                span,
                declarator,
                object_arg,
            })
        }
        VueMacroKind::DefineOptions => {
            let object_arg = extract_object_arg_from_call(call, 0, ctx);
            Some(ScriptMacro::DefineOptions {
                span,
                declarator,
                object_arg,
            })
        }
        VueMacroKind::DefineModel => {
            // defineModel(name?, options?)
            let name_span = call.arguments.first().and_then(|arg| {
                if let Some(Expression::StringLiteral(s)) = arg.as_expression() {
                    Some(Span::from(s.span))
                } else {
                    None
                }
            });

            // Options is second arg if first is string, or first arg if no string
            let options_idx = if name_span.is_some() { 1 } else { 0 };
            let options_span = call.arguments.get(options_idx).and_then(|arg| {
                if let Some(Expression::ObjectExpression(obj)) = arg.as_expression() {
                    Some(Span::from(obj.span))
                } else {
                    None
                }
            });

            Some(ScriptMacro::DefineModel {
                span,
                declarator,
                type_params,
                name_span,
                options_span,
            })
        }
        VueMacroKind::DefineSlots => Some(ScriptMacro::DefineSlots {
            span,
            declarator,
            type_params,
        }),
        VueMacroKind::WithDefaults => {
            // First arg should be defineProps call
            let (define_props_span, define_props_type_params) = call
                .arguments
                .first()
                .and_then(|arg| {
                    if let Some(Expression::CallExpression(inner)) = arg.as_expression() {
                        // Verify it's defineProps
                        if let Expression::Identifier(id) = &inner.callee {
                            if id.name.as_bytes() == b"defineProps" {
                                let inner_type_params = inner
                                    .type_arguments
                                    .as_ref()
                                    .map(|tp| extract_type_params(tp, ctx, type_ctx));
                                return Some((Some(Span::from(inner.span)), inner_type_params));
                            }
                        }
                    }
                    None
                })
                .unwrap_or((None, None));

            // Second arg is defaults object
            let defaults = extract_object_arg_from_call(call, 1, ctx);

            Some(ScriptMacro::WithDefaults {
                span,
                declarator,
                define_props_span,
                define_props_type_params,
                defaults,
            })
        }
    }
}

/// Extract type parameters from a TSTypeParameterInstantiation.
///
/// Uses the `TypeResolutionContext` to resolve type references (interfaces, type aliases)
/// declared in the same SFC. Unresolvable external types produce empty results.
fn extract_type_params<'a>(
    tp: &TSTypeParameterInstantiation<'a>,
    ctx: &ScriptParseContext<'a>,
    type_ctx: &TypeResolutionContext<'_, 'a>,
) -> MacroTypeParams {
    let full_span = tp.span;
    let offset = ctx.content_offset;

    // Type annotation spans are NOT adjusted by adjust_program_spans(),
    // so we add content_offset to convert from local to SFC coordinates.

    // The < is at the start
    let lt_span = Span::new(full_span.start + offset, full_span.start + 1 + offset);

    // The > is at the end
    let gt_span = Span::new(full_span.end - 1 + offset, full_span.end + offset);

    // The type content is between < and >
    let type_span = Span::new(full_span.start + 1 + offset, full_span.end - 1 + offset);

    // Resolve the type from the first type parameter using the type context
    // This enables resolution of SFC-local interfaces and type aliases
    let resolved = tp
        .params
        .first()
        .map(|ts_type| resolve_type_elements_with_ctx_ref(ts_type, ctx.content_offset, type_ctx))
        .unwrap_or_default();

    // Detect unresolvable type references: the first type param is a TSTypeReference
    // (e.g., `Props` in `defineProps<Props>()`) but resolution produced empty results.
    // This distinguishes `defineProps<{}>()` (empty type literal, no error) from
    // `defineProps<ExternalProps>()` (unresolvable reference, should warn).
    let is_type_reference = tp
        .params
        .first()
        .is_some_and(|ts_type| matches!(ts_type, TSType::TSTypeReference(_)));
    let unresolved_type_ref = is_type_reference && resolved.props.is_empty();

    // Infer runtime types from the root type (for simple types like `string`, `number`)
    let runtime_types = tp
        .params
        .first()
        .map(|ts_type| infer_runtime_type(ts_type))
        .unwrap_or_default();

    MacroTypeParams {
        lt_span,
        type_span,
        gt_span,
        resolved,
        runtime_types,
        unresolved_type_ref,
    }
}

/// Extract both object and array arguments from a call expression
fn extract_arg_spans<'a>(
    call: &CallExpression<'a>,
    ctx: &ScriptParseContext<'a>,
) -> (Option<MacroObjectArg<'a>>, Option<MacroArrayArg>) {
    let object_arg = extract_object_arg_from_call(call, 0, ctx);
    let array_arg = extract_array_arg_from_call(call, 0, ctx);
    (object_arg, array_arg)
}

/// Extract an object argument from a call at a specific index
fn extract_object_arg_from_call<'a>(
    call: &CallExpression<'a>,
    index: usize,
    ctx: &ScriptParseContext<'a>,
) -> Option<MacroObjectArg<'a>> {
    call.arguments.get(index).and_then(|arg| {
        if let Some(Expression::ObjectExpression(obj)) = arg.as_expression() {
            Some(extract_object_arg(obj, ctx))
        } else {
            None
        }
    })
}

/// Extract an array argument from a call at a specific index
fn extract_array_arg_from_call(
    call: &CallExpression<'_>,
    index: usize,
    ctx: &ScriptParseContext<'_>,
) -> Option<MacroArrayArg> {
    call.arguments.get(index).and_then(|arg| {
        if let Some(Expression::ArrayExpression(arr)) = arg.as_expression() {
            Some(extract_array_arg(arr, ctx))
        } else {
            None
        }
    })
}

/// Extract object argument details
fn extract_object_arg<'a>(
    obj: &ObjectExpression<'a>,
    ctx: &ScriptParseContext<'a>,
) -> MacroObjectArg<'a> {
    let mut properties = Vec::new();

    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop {
            if let Some((name, name_span)) = extract_property_key(&p.key, ctx) {
                let value_span = if p.shorthand {
                    None
                } else {
                    Some(Span::from(p.value.span()))
                };
                properties.push(MacroProperty {
                    name,
                    name_span,
                    value_span,
                });
            }
        }
    }

    MacroObjectArg {
        span: Span::from(obj.span),
        properties,
    }
}

/// Extract array argument details
fn extract_array_arg(arr: &ArrayExpression<'_>, ctx: &ScriptParseContext<'_>) -> MacroArrayArg {
    let element_spans = arr
        .elements
        .iter()
        .filter_map(|elem| match elem {
            ArrayExpressionElement::SpreadElement(s) => Some(Span::from(s.span)),
            ArrayExpressionElement::Elision(_) => None,
            _ => elem.as_expression().map(|e| Span::from(e.span())),
        })
        .collect();

    MacroArrayArg {
        span: Span::from(arr.span),
        element_spans,
    }
}

/// Extract property key name and span
fn extract_property_key<'a>(
    key: &PropertyKey<'a>,
    ctx: &ScriptParseContext<'a>,
) -> Option<(&'a str, Span)> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some((id.name.as_str(), Span::from(id.span))),
        PropertyKey::StringLiteral(s) => Some((s.value.as_str(), Span::from(s.span))),
        PropertyKey::NumericLiteral(n) => {
            // For numeric keys, we'd need to convert to string
            // For now, skip these as they're rare in Vue macros
            None
        }
        _ => None,
    }
}

/// Collect declarations from a binding pattern
fn collect_declarations_from_pattern<'a>(
    pattern: &BindingPattern<'a>,
    kind: DeclarationKind,
    is_ref_like: bool,
    items: &mut Vec<ScriptItem<'a>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            items.push(ScriptItem::Declaration(ScriptDeclaration {
                span: Span::from(id.span),
                name: Some(id.name.as_str()),
                name_span: Some(Span::from(id.span)),
                kind,
                is_ref_like,
            }));
        }
        BindingPattern::ObjectPattern(obj) => {
            // Destructured bindings are never ref-like
            for prop in &obj.properties {
                collect_declarations_from_pattern(&prop.value, kind, false, items);
            }
            if let Some(rest) = &obj.rest {
                collect_declarations_from_pattern(&rest.argument, kind, false, items);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            // Destructured bindings are never ref-like
            for elem in arr.elements.iter().flatten() {
                collect_declarations_from_pattern(elem, kind, false, items);
            }
            if let Some(rest) = &arr.rest {
                collect_declarations_from_pattern(&rest.argument, kind, false, items);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_declarations_from_pattern(&assign.left, kind, is_ref_like, items);
        }
    }
}

// =============================================================================
// Vue API Usage Collection
// =============================================================================

/// Check an expression tree for Vue API calls and collect usage.
/// This runs in parallel with `check_expression_for_async`.
pub fn check_expression_for_usage<'a>(
    expr: &Expression<'a>,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    current_binding_span: Option<Span>,
) {
    match expr {
        // Track await expressions for before/after context
        Expression::AwaitExpression(await_expr) => {
            usage_ctx.record_await(Span::from(await_expr.span));
            // Also check the argument for Vue API calls
            check_expression_for_usage(&await_expr.argument, ctx, usage_ctx, None);
        }
        Expression::CallExpression(call) => {
            // Check if callee is a Vue API function
            if let Expression::Identifier(id) = &call.callee {
                if let Some(api_kind) = detect_vue_api_call(id.name.as_bytes()) {
                    collect_api_usage(call, api_kind, ctx, usage_ctx, current_binding_span);
                }
            }

            // Recurse into arguments (may contain nested Vue API calls)
            for arg in &call.arguments {
                if let Some(expr) = arg.as_expression() {
                    check_expression_for_usage(expr, ctx, usage_ctx, None);
                }
            }
        }
        Expression::BinaryExpression(bin) => {
            check_expression_for_usage(&bin.left, ctx, usage_ctx, None);
            check_expression_for_usage(&bin.right, ctx, usage_ctx, None);
        }
        Expression::ConditionalExpression(cond) => {
            check_expression_for_usage(&cond.test, ctx, usage_ctx, None);
            check_expression_for_usage(&cond.consequent, ctx, usage_ctx, None);
            check_expression_for_usage(&cond.alternate, ctx, usage_ctx, None);
        }
        Expression::AssignmentExpression(assign) => {
            check_expression_for_usage(&assign.right, ctx, usage_ctx, None);
        }
        Expression::ParenthesizedExpression(paren) => {
            check_expression_for_usage(&paren.expression, ctx, usage_ctx, current_binding_span);
        }
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                check_expression_for_usage(expr, ctx, usage_ctx, None);
            }
        }
        Expression::LogicalExpression(logical) => {
            check_expression_for_usage(&logical.left, ctx, usage_ctx, None);
            check_expression_for_usage(&logical.right, ctx, usage_ctx, None);
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(expr) = elem.as_expression() {
                    check_expression_for_usage(expr, ctx, usage_ctx, None);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    check_expression_for_usage(&p.value, ctx, usage_ctx, None);
                }
            }
        }
        // Don't recurse into function expressions - they have their own scope
        Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_) => {}
        _ => {}
    }
}

/// Collect specific Vue API usage details
fn collect_api_usage<'a>(
    call: &CallExpression<'a>,
    kind: VueApiKind,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    binding_span: Option<Span>,
) {
    let span = Span::from(call.span);

    match kind.category() {
        VueApiCategory::DependencyInjection => {
            collect_di_usage(call, kind, ctx, usage_ctx, binding_span, span);
        }
        VueApiCategory::Reactivity => {
            collect_reactivity_usage(kind, usage_ctx, binding_span, span, call, ctx);
        }
        VueApiCategory::Lifecycle => {
            collect_lifecycle_usage(call, kind, ctx, usage_ctx, span);
        }
        VueApiCategory::Watchers => {
            collect_watcher_usage(call, kind, ctx, usage_ctx, span);
        }
        VueApiCategory::TemplateUtils => {
            collect_template_util_usage(call, kind, ctx, usage_ctx, binding_span, span);
        }
        VueApiCategory::InstanceAccess => {
            collect_instance_access_usage(ctx, usage_ctx, binding_span, span);
        }
    }
}

/// Collect provide/inject usage
fn collect_di_usage<'a>(
    call: &CallExpression<'a>,
    kind: VueApiKind,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    binding_span: Option<Span>,
    span: Span,
) {
    match kind {
        VueApiKind::Provide => {
            if let Some(usage) = extract_provide_usage(call, ctx, span) {
                usage_ctx.record_provide(usage);
            }
        }
        VueApiKind::Inject => {
            if let Some(usage) = extract_inject_usage(call, ctx, binding_span, span) {
                usage_ctx.record_inject(usage);
            }
        }
        _ => {}
    }
}

/// Extract provide() call details
fn extract_provide_usage<'a>(
    call: &CallExpression<'a>,
    ctx: &ScriptParseContext<'a>,
    span: Span,
) -> Option<ProvideUsage> {
    // Get key (first argument)
    let key = call
        .arguments
        .first()
        .and_then(|arg| extract_provide_key(arg.as_expression()?, ctx))?;

    // Get value (second argument)
    let value_span = call
        .arguments
        .get(1)
        .and_then(|a| a.as_expression())
        .map(|e| Span::from(e.span()))?;

    Some(ProvideUsage {
        span,
        key,
        value_span,
    })
}

/// Extract inject() call details
fn extract_inject_usage<'a>(
    call: &CallExpression<'a>,
    ctx: &ScriptParseContext<'a>,
    binding_span: Option<Span>,
    span: Span,
) -> Option<InjectUsage> {
    // Get key (first argument)
    let key = call
        .arguments
        .first()
        .and_then(|arg| extract_provide_key(arg.as_expression()?, ctx))?;

    // Check if default value is provided (second argument)
    let has_default = call.arguments.len() > 1;

    Some(InjectUsage {
        span,
        key,
        has_default,
        binding_span,
    })
}

/// Extract provide/inject key from expression
fn extract_provide_key<'a>(
    expr: &Expression<'a>,
    ctx: &ScriptParseContext<'a>,
) -> Option<ProvideKey> {
    match expr {
        Expression::StringLiteral(s) => Some(ProvideKey {
            span: Span::from(s.span),
            kind: ProvideKeyKind::StringLiteral,
        }),
        Expression::Identifier(id) => Some(ProvideKey {
            span: Span::from(id.span),
            kind: ProvideKeyKind::Symbol,
        }),
        _ => Some(ProvideKey {
            span: Span::from(expr.span()),
            kind: ProvideKeyKind::Dynamic,
        }),
    }
}

/// Collect reactivity API usage (ref, reactive, computed, etc.)
fn collect_reactivity_usage<'a>(
    kind: VueApiKind,
    usage_ctx: &mut UsageCollector<'a>,
    binding_span: Option<Span>,
    span: Span,
    call: &CallExpression<'a>,
    ctx: &ScriptParseContext<'a>,
) {
    if let Some(reactive_kind) = ReactiveKind::from_api_kind(kind) {
        if let Some(binding) = binding_span {
            let initializer_span = call
                .arguments
                .first()
                .and_then(|a| a.as_expression())
                .map(|e| Span::from(e.span()));

            usage_ctx.record_reactive(ReactiveStateUsage {
                kind: reactive_kind,
                binding_span: binding,
                initializer_span,
            });
        }
    }
}

/// Collect lifecycle hook usage
fn collect_lifecycle_usage<'a>(
    call: &CallExpression<'a>,
    kind: VueApiKind,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    span: Span,
) {
    if let Some(hook) = LifecycleHook::from_api_kind(kind) {
        // Get callback span (first argument)
        let callback_span = call
            .arguments
            .first()
            .and_then(|a| a.as_expression())
            .map(|e| Span::from(e.span()))
            .unwrap_or(span);

        usage_ctx.record_lifecycle(LifecycleUsage {
            span,
            hook,
            callback_span,
        });
    }
}

/// Collect watcher usage (watch, watchEffect, etc.)
fn collect_watcher_usage<'a>(
    call: &CallExpression<'a>,
    kind: VueApiKind,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    span: Span,
) {
    let (callback_span, source_spans) = match kind {
        VueApiKind::Watch => {
            // watch(source, callback, options?)
            let source_spans: Vec<Span> = call
                .arguments
                .first()
                .and_then(|a| a.as_expression())
                .map(|e| vec![Span::from(e.span())])
                .unwrap_or_default();

            let callback_span = call
                .arguments
                .get(1)
                .and_then(|a| a.as_expression())
                .map(|e| Span::from(e.span()))
                .unwrap_or(span);

            (callback_span, source_spans)
        }
        _ => {
            // watchEffect, watchPostEffect, watchSyncEffect - just callback
            let callback_span = call
                .arguments
                .first()
                .and_then(|a| a.as_expression())
                .map(|e| Span::from(e.span()))
                .unwrap_or(span);

            (callback_span, Vec::new())
        }
    };

    usage_ctx.record_watcher(WatcherUsage {
        span,
        kind,
        callback_span,
        source_spans,
    });
}

/// Collect template utility usage (useSlots, useAttrs, useTemplateRef)
fn collect_template_util_usage<'a>(
    call: &CallExpression<'a>,
    kind: VueApiKind,
    ctx: &ScriptParseContext<'a>,
    usage_ctx: &mut UsageCollector<'a>,
    binding_span: Option<Span>,
    span: Span,
) {
    let ref_name_span = if kind == VueApiKind::UseTemplateRef {
        // useTemplateRef('refName')
        call.arguments
            .first()
            .and_then(|a| a.as_expression())
            .and_then(|e| {
                if let Expression::StringLiteral(s) = e {
                    Some(Span::from(s.span))
                } else {
                    None
                }
            })
    } else {
        None
    };

    usage_ctx.record_template_util(TemplateUtilUsage {
        span,
        kind,
        binding_span,
        ref_name_span,
    });
}

/// Collect instance access API usage (getCurrentInstance)
///
/// This tracks calls to APIs that require synchronous setup/lifecycle context.
/// The context (before/after await) is determined by comparing span positions.
fn collect_instance_access_usage(
    _ctx: &ScriptParseContext<'_>,
    usage_ctx: &mut UsageCollector<'_>,
    binding_span: Option<Span>,
    span: Span,
) {
    // Determine call site context based on await tracking
    let (context, preceding_await_span) = if usage_ctx.has_await_before(span.start) {
        (CallSiteContext::AfterAwait, usage_ctx.first_await_span())
    } else {
        (CallSiteContext::BeforeAwait, None)
    };

    usage_ctx.record_sync_context_usage(SyncContextUsage {
        span,
        kind: VueApiKind::GetCurrentInstance,
        context,
        binding_span,
        preceding_await_span,
    });
}
