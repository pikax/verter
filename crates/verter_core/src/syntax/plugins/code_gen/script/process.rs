use crate::{
    code_transform,
    cursor::ScriptLanguage,
    syntax::{
        plugins::code_gen::{
            script::{
                macros::process_macro,
                sections::{emit_emits_section, emit_props_section},
            },
            types::ScriptSetupImportDependencies,
        },
        types::OxcScript,
    },
    utils::oxc::vue::{ScriptItem, ScriptMacro, TypeDeclarationKind, VueMacroKind},
};

pub struct ProcessScriptOptions<'alloc> {
    pub is_production: bool,
    pub source: &'alloc str,
    pub component_name: &'alloc str,

    pub inline_template: bool,
    pub keep_ts_types: bool,
    pub is_vapor: bool,
}

pub struct ProcessedScript {
    pub imports: ScriptSetupImportDependencies,
    pub diagnostics: Vec<super::macros::types::MacroDiagnostic>,
    /// Deferred closing text for inline template mode.
    /// When set, this must be emitted AFTER the template content to close
    /// setup() and the component definition.
    pub deferred_closing: Option<String>,
}

pub fn process_script_event<'alloc>(
    script: &OxcScript<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
    opts: ProcessScriptOptions<'alloc>,
) -> ProcessedScript {
    let mut imports = ScriptSetupImportDependencies::default();
    let mut diagnostics = Vec::new();
    // not in setup
    if script.setup.is_none() {
        // Regular <script> (no setup): strip the <script> and </script> tags
        // but keep the content between them.
        code_transform.remove(script.tag_open_start, script.tag_open_end);
        code_transform.remove(script.tag_close_start, script.tag_close_end);

        // TODO
        return ProcessedScript {
            imports,
            diagnostics,
            deferred_closing: None,
        };
    }

    // setup

    let is_typescript = matches!(script.lang, Some(lang) if lang == ScriptLanguage::TypeScript || lang == ScriptLanguage::TSX);

    let mut returned: Vec<&'alloc str> = Vec::with_capacity(script.result.bindings.len());
    let mut prop = None;
    let mut options = None;
    let mut expose = None;
    let mut emit = None;
    let mut models = Vec::new();

    let mut has_emit_declarator = false;

    // Opening tag: emit "const __sfc__ = /*@__PURE__*/".
    // The actual `export default __sfc__` is appended by ScriptGeneratorPlugin::end(),
    // which also handles __scopeId for scoped styles. Using a variable here instead of
    // `export default` avoids string-based detection issues (e.g., "export default" in
    // comments/strings being mistakenly matched by downstream processors).
    code_transform.overwrite(
        script.tag_open_start,
        script.tag_open_end,
        "const __sfc__ = /*@__PURE__*/",
    );

    // Strip TypeScript type annotations from declarations BEFORE processing items.
    // This handles inline type annotations (`: number`, `as Foo`, etc.) and
    // per-specifier type imports that process_script_event doesn't handle.
    // We walk the program AST directly, skipping imports (handled below) and
    // expression statements (which may contain Vue macros like defineProps<...>()).
    // Program AST spans are SFC-absolute (adjusted by oxc_parser).
    if !opts.keep_ts_types && is_typescript {
        strip_ts_declarations(&script.program, code_transform, opts.source);
    }

    // Process each script item
    for item in script.result.items.iter() {
        match item {
            ScriptItem::Import(event) => {
                if event.is_type_only {
                    // Strip type-only imports (`import type { ... }`) — invalid in JS output
                    code_transform.remove(event.span.start, event.span.end);
                } else {
                    // Strip per-specifier type imports BEFORE moving.
                    // Moved chunks are skipped by subsequent overwrite/remove operations,
                    // so we must strip `type` keywords before the move.
                    if !opts.keep_ts_types {
                        strip_import_type_specifiers(event, &script.program, code_transform);
                    }
                    code_transform.move_with_suffix(
                        event.span.start,
                        event.span.end,
                        script.tag_open_start,
                        "\n",
                    );
                    // Include imported bindings in __returned__ so they're accessible
                    // via $setup in the render function (components, helpers, constants).
                    // Skip per-specifier type imports (`import { type Foo }`) — they have no runtime value.
                    for binding in &event.bindings {
                        if !binding.is_type_only {
                            returned.push(binding.name);
                        }
                    }
                }
            }
            ScriptItem::Declaration(decl) => {
                // Track declarations by name to include in return statement
                // Use name instead of span to avoid including function bodies
                if let Some(name) = decl.name {
                    returned.push(name);
                }
            }
            ScriptItem::TypeDeclaration(type_decl) => {
                if opts.keep_ts_types {
                    // Move TypeScript declarations outside the component (to where imports go)
                    // This ensures interfaces/types are at module scope, not inside setup()
                    code_transform.move_with_suffix(
                        type_decl.span.start,
                        type_decl.span.end,
                        script.tag_open_start,
                        "\n",
                    );
                } else {
                    match type_decl.kind {
                        TypeDeclarationKind::Enum => {
                            // TODO convert to JS enum-like object instead of removing
                            code_transform.remove(type_decl.span.start, type_decl.span.end);
                        }
                        _ => {
                            // Remove interfaces, type aliases, namespaces
                            code_transform.remove(type_decl.span.start, type_decl.span.end);
                        }
                    }
                }
            }
            ScriptItem::Async(e) => {
                // Wrap top-level await with async context helper
                code_transform.overwrite(
                    e.span.start,
                    e.span.start + 5,
                    r#"
;(([__temp,__restore]=_withAsyncContext(()=>"#,
                );

                code_transform.prepend_left(
                    e.span.end,
                    r#")),await __temp,__restore())
"#,
                );
            }
            ScriptItem::Macro(macro_item) => {
                if let ScriptMacro::DefineEmits { declarator, .. } = macro_item {
                    if declarator.is_some() {
                        has_emit_declarator = true;
                    }
                };

                // Track defineExpose before processing (it returns None but we need to track it)
                if matches!(macro_item.kind(), VueMacroKind::DefineExpose) {
                    expose = Some(true);
                }

                let mut result = process_macro(
                    script,
                    macro_item,
                    code_transform,
                    opts.source,
                    opts.is_production,
                );

                // Collect diagnostics from macro processing
                if let Some(ref mut r) = result {
                    if let Some(d) = r.diagnostic.take() {
                        diagnostics.push(d);
                    }
                }

                // Handle macros that don't return a result
                if result.is_none() {
                    // DefineSlots needs import even though it returns None
                    if matches!(macro_item.kind(), VueMacroKind::DefineSlots) {
                        imports.add(ScriptSetupImportDependencies::USE_SLOTS);
                    }
                    continue;
                }

                match macro_item.kind() {
                    VueMacroKind::DefineProps => {
                        prop = Some(result);
                    }
                    VueMacroKind::WithDefaults => {
                        prop = Some(result);
                    }
                    VueMacroKind::DefineOptions => {
                        options = Some(result);
                    }
                    VueMacroKind::DefineModel => {
                        imports.add(ScriptSetupImportDependencies::USE_MODEL);
                        models.push(result);
                    }
                    VueMacroKind::DefineExpose => {
                        // Already tracked above
                    }
                    VueMacroKind::DefineEmits => {
                        emit = Some(result);
                    }
                    VueMacroKind::DefineSlots => {
                        // Import already added above
                    }
                }
            }
            _ => {}
        }
    }

    let needs_processing = if let Some(Some(opt)) = options {
        if let Some(span) = opt.move_span {
            code_transform.move_wrapped(
                span.start,
                span.end,
                script.tag_open_end,
                if is_typescript {
                    imports.add(ScriptSetupImportDependencies::DEFINE_COMPONENT);
                    "_defineComponent({..."
                } else {
                    "Object.assign("
                },
                if is_typescript { "," } else { ",{" },
            );
            false
        } else {
            true
        }
    } else {
        true
    };
    if needs_processing {
        if is_typescript {
            imports.add(ScriptSetupImportDependencies::DEFINE_COMPONENT);
            code_transform.prepend_left(script.tag_open_end, "_defineComponent({\n");
        } else {
            code_transform.prepend_left(script.tag_open_end, "{\n");
        }
    }
    {
        let mut buf = String::with_capacity(opts.component_name.len() + 32);
        buf.push_str("__name: '");
        buf.push_str(opts.component_name);
        buf.push_str("',");
        if opts.is_vapor {
            buf.push_str("__vapor: true,");
        }
        code_transform.prepend_left(script.tag_open_end, &buf);
    }

    // Process props and emits sections
    let insert_pos = script.tag_open_end;
    let props_needs_merge = emit_props_section(code_transform, prop, &models, insert_pos);
    let emits_needs_merge = emit_emits_section(code_transform, emit, models, insert_pos);

    // Add mergeModels import if needed by either section
    if props_needs_merge || emits_needs_merge {
        imports.add(ScriptSetupImportDependencies::MERGE_MODELS);
    }

    // Production mode: minimal setup signature unless expose/emit needed
    // Development mode: full signature with expose for devtools
    let needs_expose_in_signature = !opts.is_production || expose.is_some();
    let needs_emit_in_signature = has_emit_declarator;

    if needs_expose_in_signature || needs_emit_in_signature {
        let mut buf = String::with_capacity(48);
        buf.push_str("setup(__props,{");
        if needs_expose_in_signature {
            buf.push_str("expose:__expose");
        }
        if needs_emit_in_signature {
            if needs_expose_in_signature {
                buf.push_str(",emit:__emit");
            } else {
                buf.push_str("emit:__emit");
            }
        }
        buf.push_str("}){");
        code_transform.prepend_left(script.tag_open_end, &buf);
    } else {
        // Minimal signature for production
        code_transform.prepend_left(script.tag_open_end, "setup(__props){");
    }

    // Auto-call __expose() only in development mode (when expose is in signature)
    if !opts.is_production && expose.is_none() {
        code_transform.prepend_left(script.tag_open_end, "__expose();");
    }

    // Replace </script> closing tag
    let closing_paren = if is_typescript || !needs_processing {
        ")"
    } else {
        ""
    };

    // Build closing text and apply it.
    // When inline_template is true, setup() is left open for the template to provide
    // the return value (the arrow render function). The deferred_closing is emitted
    // AFTER the template content by ScriptGeneratorPlugin::end().
    let (closing_text, deferred_closing) = if opts.inline_template {
        // Leave setup() open — template will provide `return (_ctx,_cache) => { ... }`
        // The deferred closing will close setup() and the component definition.
        let mut deferred = String::with_capacity(8);
        deferred.push_str("\n}}");
        deferred.push_str(closing_paren);
        deferred.push(';');
        ("\n".to_string(), Some(deferred))
    } else {
        let joined = returned.join(", ");
        let mut buf = String::with_capacity(joined.len() + 128);
        if opts.is_production {
            buf.push_str("\nreturn {");
            buf.push_str(&joined);
            buf.push_str("}\n}}");
            buf.push_str(closing_paren);
            buf.push_str(";\n");
        } else {
            buf.push_str("\nconst __returned__={");
            buf.push_str(&joined);
            buf.push_str("}\nObject.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })\nreturn __returned__\n}}");
            buf.push_str(closing_paren);
            buf.push(';');
        }
        (buf, None)
    };

    code_transform.overwrite(script.tag_close_start, script.tag_close_end, &closing_text);

    // TypeScript type annotations in declarations (variable type annotations,
    // function signatures) were already stripped by strip_ts_declarations() above.
    // Import type specifiers were stripped by strip_import_type_specifiers() before
    // imports were moved. Macro type parameters are handled by macro processing.

    ProcessedScript {
        imports,
        diagnostics,
        deferred_closing,
    }
}

/// Strip TypeScript type annotations from declarations in the program body.
///
/// This handles variable type annotations, function signatures, and other
/// declaration-level TypeScript syntax. It deliberately skips:
/// - Import declarations (handled separately by process_script_event)
/// - Expression statements (may contain Vue macros like defineProps<...>())
///
/// The program AST spans must be SFC-absolute (adjusted by oxc_parser).
fn strip_ts_declarations<'alloc>(
    program: &oxc_ast::ast::Program<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
    source: &str,
) {
    use oxc_ast::ast::*;

    for stmt in &program.body {
        match stmt {
            // Skip imports — handled by process_script_event
            Statement::ImportDeclaration(_) => {}
            // Skip expression statements — may contain Vue macros
            Statement::ExpressionStatement(_) => {}
            // Skip TS declarations — handled by process_script_event (TypeDeclaration item)
            Statement::TSTypeAliasDeclaration(_)
            | Statement::TSInterfaceDeclaration(_)
            | Statement::TSModuleDeclaration(_)
            | Statement::TSEnumDeclaration(_) => {}
            // Strip type annotations from variable declarations
            Statement::VariableDeclaration(var_decl) => {
                if var_decl.declare {
                    continue;
                }
                for declarator in &var_decl.declarations {
                    if let Some(ta) = &declarator.type_annotation {
                        code_transform.remove(ta.span.start, ta.span.end);
                    }
                    if let Some(init) = &declarator.init {
                        strip_ts_expression(init, code_transform);
                    }
                }
            }
            Statement::FunctionDeclaration(func) => {
                // `declare function` is removed entirely by the TypeDeclaration item handler
                if !func.declare {
                    strip_ts_function(func, code_transform);
                }
            }
            Statement::ClassDeclaration(class) => {
                // `declare class` is removed entirely by the TypeDeclaration item handler
                if !class.declare {
                    strip_ts_class(class, code_transform, source);
                }
            }
            // Export declarations may wrap type-annotated declarations
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::VariableDeclaration(var_decl) => {
                            if !var_decl.declare {
                                for declarator in &var_decl.declarations {
                                    if let Some(ta) = &declarator.type_annotation {
                                        code_transform.remove(ta.span.start, ta.span.end);
                                    }
                                    if let Some(init) = &declarator.init {
                                        strip_ts_expression(init, code_transform);
                                    }
                                }
                            }
                        }
                        Declaration::FunctionDeclaration(func) => {
                            strip_ts_function(func, code_transform);
                        }
                        Declaration::ClassDeclaration(class) => {
                            strip_ts_class(class, code_transform, source);
                        }
                        _ => {}
                    }
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    strip_ts_function(f, code_transform);
                }
                ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    strip_ts_class(c, code_transform, source);
                }
                _ => {
                    if let Some(expr) = export.declaration.as_expression() {
                        strip_ts_expression(expr, code_transform);
                    }
                }
            },
            _ => {}
        }
    }
}

/// Strip TypeScript type annotations from a function declaration/expression.
fn strip_ts_function<'alloc>(
    func: &oxc_ast::ast::Function<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
) {
    if let Some(tp) = &func.type_parameters {
        code_transform.remove(tp.span.start, tp.span.end);
    }
    if let Some(rt) = &func.return_type {
        code_transform.remove(rt.span.start, rt.span.end);
    }
    // Strip parameter type annotations
    for param in &func.params.items {
        if let Some(ta) = &param.type_annotation {
            code_transform.remove(ta.span.start, ta.span.end);
        }
    }
    if let Some(rest) = &func.params.rest {
        if let Some(ta) = &rest.type_annotation {
            code_transform.remove(ta.span.start, ta.span.end);
        }
    }
    // Recurse into function body
    if let Some(body) = &func.body {
        for stmt in &body.statements {
            strip_ts_statement(stmt, code_transform);
        }
    }
}

/// Strip TypeScript type annotations from expressions (as, satisfies, non-null, etc.)
fn strip_ts_expression<'alloc>(
    expr: &oxc_ast::ast::Expression<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
) {
    use oxc_ast::ast::*;
    use oxc_span::GetSpan;

    match expr {
        Expression::TSAsExpression(e) => {
            code_transform.remove(e.expression.span().end, e.span.end);
            strip_ts_expression(&e.expression, code_transform);
        }
        Expression::TSSatisfiesExpression(e) => {
            code_transform.remove(e.expression.span().end, e.span.end);
            strip_ts_expression(&e.expression, code_transform);
        }
        Expression::TSNonNullExpression(e) => {
            code_transform.remove(e.expression.span().end, e.span.end);
            strip_ts_expression(&e.expression, code_transform);
        }
        Expression::TSTypeAssertion(e) => {
            code_transform.remove(e.span.start, e.expression.span().start);
            strip_ts_expression(&e.expression, code_transform);
        }
        Expression::TSInstantiationExpression(e) => {
            code_transform.remove(e.expression.span().end, e.span.end);
            strip_ts_expression(&e.expression, code_transform);
        }
        // Note: We do NOT strip type arguments from call expressions here,
        // because Vue macros (defineProps, defineEmits, etc.) handle their own
        // type parameters. Regular TS call type args like `fn<T>()` would need
        // stripping, but those are rare in <script setup> and can be added later.
        Expression::ArrowFunctionExpression(arrow) => {
            if let Some(tp) = &arrow.type_parameters {
                code_transform.remove(tp.span.start, tp.span.end);
            }
            if let Some(rt) = &arrow.return_type {
                code_transform.remove(rt.span.start, rt.span.end);
            }
            for param in &arrow.params.items {
                if let Some(ta) = &param.type_annotation {
                    code_transform.remove(ta.span.start, ta.span.end);
                }
            }
            for stmt in &arrow.body.statements {
                strip_ts_statement(stmt, code_transform);
            }
        }
        Expression::FunctionExpression(func) => {
            strip_ts_function(func, code_transform);
        }
        // Recurse into container expressions
        Expression::ParenthesizedExpression(p) => {
            strip_ts_expression(&p.expression, code_transform);
        }
        Expression::ConditionalExpression(c) => {
            strip_ts_expression(&c.test, code_transform);
            strip_ts_expression(&c.consequent, code_transform);
            strip_ts_expression(&c.alternate, code_transform);
        }
        Expression::AssignmentExpression(a) => {
            strip_ts_expression(&a.right, code_transform);
        }
        Expression::SequenceExpression(s) => {
            for e in &s.expressions {
                strip_ts_expression(e, code_transform);
            }
        }
        _ => {}
    }
}

/// Strip TypeScript from a statement (recursive helper for function/block bodies).
fn strip_ts_statement<'alloc>(
    stmt: &oxc_ast::ast::Statement<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
) {
    use oxc_ast::ast::*;

    match stmt {
        Statement::VariableDeclaration(var_decl) => {
            if var_decl.declare {
                code_transform.remove(var_decl.span.start, var_decl.span.end);
                return;
            }
            for declarator in &var_decl.declarations {
                if let Some(ta) = &declarator.type_annotation {
                    code_transform.remove(ta.span.start, ta.span.end);
                }
                if let Some(init) = &declarator.init {
                    strip_ts_expression(init, code_transform);
                }
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            strip_ts_expression(&expr_stmt.expression, code_transform);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                strip_ts_expression(arg, code_transform);
            }
        }
        Statement::FunctionDeclaration(func) => {
            strip_ts_function(func, code_transform);
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                strip_ts_statement(s, code_transform);
            }
        }
        Statement::IfStatement(if_stmt) => {
            strip_ts_expression(&if_stmt.test, code_transform);
            strip_ts_statement(&if_stmt.consequent, code_transform);
            if let Some(alt) = &if_stmt.alternate {
                strip_ts_statement(alt, code_transform);
            }
        }
        Statement::ForStatement(for_stmt) => {
            if let Some(ForStatementInit::VariableDeclaration(var_decl)) = &for_stmt.init {
                for declarator in &var_decl.declarations {
                    if let Some(ta) = &declarator.type_annotation {
                        code_transform.remove(ta.span.start, ta.span.end);
                    }
                }
            }
            strip_ts_statement(&for_stmt.body, code_transform);
        }
        Statement::ForInStatement(s) => {
            strip_ts_statement(&s.body, code_transform);
        }
        Statement::ForOfStatement(s) => {
            strip_ts_statement(&s.body, code_transform);
        }
        Statement::WhileStatement(s) => {
            strip_ts_statement(&s.body, code_transform);
        }
        Statement::TryStatement(t) => {
            for s in &t.block.body {
                strip_ts_statement(s, code_transform);
            }
            if let Some(handler) = &t.handler {
                for s in &handler.body.body {
                    strip_ts_statement(s, code_transform);
                }
            }
            if let Some(finalizer) = &t.finalizer {
                for s in &finalizer.body {
                    strip_ts_statement(s, code_transform);
                }
            }
        }
        // TS declarations inside function bodies
        Statement::TSTypeAliasDeclaration(d) => {
            code_transform.remove(d.span.start, d.span.end);
        }
        Statement::TSInterfaceDeclaration(d) => {
            code_transform.remove(d.span.start, d.span.end);
        }
        _ => {}
    }
}

/// Strip TypeScript-specific syntax from a class declaration.
fn strip_ts_class<'alloc>(
    _class: &oxc_ast::ast::Class<'alloc>,
    _code_transform: &mut code_transform::CodeTransform<'alloc>,
    _source: &str,
) {
    // Class TS stripping is complex (implements, type parameters, accessibility modifiers).
    // For now, this is a stub — full class stripping can be added when needed.
    // Most script setup code doesn't define classes inline.
}

/// Strip per-specifier `type` keywords from a non-type-only import before it is moved.
///
/// For `import { type Ref, ref } from 'vue'`, removes the `type Ref, ` part
/// so the moved import becomes `import { ref } from 'vue'`.
fn strip_import_type_specifiers<'alloc>(
    event: &crate::utils::oxc::vue::ScriptImport<'alloc>,
    program: &oxc_ast::ast::Program<'alloc>,
    code_transform: &mut code_transform::CodeTransform<'alloc>,
) {
    use oxc_ast::ast::*;
    use oxc_span::GetSpan;

    // Check if this import has any type-only bindings
    let has_type_specifiers = event.bindings.iter().any(|b| b.is_type_only);
    if !has_type_specifiers {
        return;
    }

    // If ALL specifiers are type-only, remove the entire import
    if event.bindings.iter().all(|b| b.is_type_only) {
        code_transform.remove(event.span.start, event.span.end);
        return;
    }

    // Find the matching import declaration in the program AST by span
    for stmt in &program.body {
        if let Statement::ImportDeclaration(import) = stmt {
            if import.span.start != event.span.start {
                continue;
            }
            // Found matching import — strip type specifiers using AST spans
            if let Some(specifiers) = &import.specifiers {
                let type_indices: Vec<usize> = specifiers
                    .iter()
                    .enumerate()
                    .filter_map(|(i, spec)| {
                        if let ImportDeclarationSpecifier::ImportSpecifier(s) = spec {
                            if s.import_kind.is_type() {
                                return Some(i);
                            }
                        }
                        None
                    })
                    .collect();

                // Remove type specifiers in reverse order to avoid span invalidation
                for &idx in type_indices.iter().rev() {
                    let spec_span = specifiers[idx].span();
                    if idx + 1 < specifiers.len() {
                        // Remove from this specifier to the start of the next
                        let next_span = specifiers[idx + 1].span();
                        code_transform.remove(spec_span.start, next_span.start);
                    } else if idx > 0 {
                        // Last specifier: remove from end of previous to end of this
                        let prev_span = specifiers[idx - 1].span();
                        code_transform.remove(prev_span.end, spec_span.end);
                    }
                }
            }
            break;
        }
    }
}
