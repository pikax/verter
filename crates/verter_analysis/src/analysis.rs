use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::classify::{is_lifecycle_api, is_reactivity_api, is_watcher_api};
use crate::exports::extract_export_signatures_from_program;
use crate::imports::analyze_import_declaration;

use crate::macros::{
    collect_type_references, try_extract_macro_from_expr, try_extract_macro_from_var_decl,
};
use crate::scope::AnalysisScope;
use crate::types::*;

/// O(1) lookup map from local binding name → (import source index, vue_api).
/// The source index refers to the `imports` Vec for zero-copy source access.
struct ImportBindingMap {
    /// local_name → (index into imports Vec, vue_api classification)
    map: FxHashMap<String, (usize, Option<VueApiClassification>)>,
}

impl ImportBindingMap {
    fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
    }

    /// Register all bindings from an import at the given index.
    fn register(&mut self, import_idx: usize, import: &AnalyzedImport) {
        for b in &import.bindings {
            self.map.insert(b.name.clone(), (import_idx, b.vue_api));
        }
    }

    /// Lookup the import source for a local binding name.
    fn source<'a>(&self, imports: &'a [AnalyzedImport], name: &str) -> Option<&'a str> {
        self.map
            .get(name)
            .map(|(idx, _)| imports[*idx].source.as_str())
    }

    /// Lookup the Vue API classification for a local binding name.
    fn vue_api(&self, name: &str) -> Option<VueApiClassification> {
        self.map.get(name).and_then(|(_, api)| *api)
    }
}

/// Build a comprehensive script analysis from source content.
///
/// Performs a single OXC parse and walks the AST to collect imports, bindings,
/// macros, and cross-reference information. Lives in `verter_analysis` so it's
/// reusable by `verter_core`, linters, and other tools.
///
/// Note: Import path resolution (relative → absolute) happens in the caller
/// (e.g., `verter_host`), not here. This function is path-resolution-agnostic.
pub fn build_script_analysis(
    content: &str,
    source_type: SourceType,
    allocator: &Allocator,
) -> ScriptAnalysisSnapshot {
    build_script_analysis_with_scope(content, source_type, allocator, AnalysisScope::all())
}

/// Build script analysis with specific scope flags controlling which passes run.
///
/// When `AnalysisScope::FUNC_RETURNS` is set, also walks exported function bodies
/// to extract return reactivity and composable info for non-SFC file analysis.
pub fn build_script_analysis_with_scope(
    content: &str,
    source_type: SourceType,
    allocator: &Allocator,
    scope: AnalysisScope,
) -> ScriptAnalysisSnapshot {
    let parser = Parser::new(allocator, content, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return ScriptAnalysisSnapshot::default();
    }

    let program = &result.program;

    // ── Single-pass collection ──
    // Imports always precede declarations in valid ESM, so the import list is
    // complete when we encounter variable/function/class declarations.
    let mut imports = Vec::new();
    let mut import_map = ImportBindingMap::new();
    let mut macros = Vec::new();
    let mut bindings = Vec::new();
    let mut has_top_level_await = false;
    // Track local type → referenced type names (from extends and intersection)
    // e.g., `interface Local extends Base {}` → { "Local": ["Base"] }
    // e.g., `type Local = Base & { own: string }` → { "Local": ["Base"] }
    let mut local_type_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                let analyzed = analyze_import_declaration(decl);
                import_map.register(imports.len(), &analyzed);
                imports.push(analyzed);
            }

            Statement::ExpressionStatement(expr_stmt) => {
                try_extract_macro_from_expr(&expr_stmt.expression, &mut macros);
                if !has_top_level_await && expr_contains_await(&expr_stmt.expression) {
                    has_top_level_await = true;
                }
            }

            Statement::VariableDeclaration(var_decl) => {
                let kind = match var_decl.kind {
                    VariableDeclarationKind::Const => AnalyzedBindingKind::Const,
                    VariableDeclarationKind::Let => AnalyzedBindingKind::Let,
                    VariableDeclarationKind::Var => AnalyzedBindingKind::Var,
                    VariableDeclarationKind::Using | VariableDeclarationKind::AwaitUsing => {
                        AnalyzedBindingKind::Const
                    }
                };

                for decl in &var_decl.declarations {
                    try_extract_macro_from_var_decl(decl, &mut macros);

                    if let BindingPattern::BindingIdentifier(id) = &decl.id {
                        let (initializer, is_reactive, mut reactivity_kind) =
                            if let Some(ref init) = decl.init {
                                classify_initializer(init, &imports, &import_map)
                            } else {
                                (None, false, ReactivityKind::None)
                            };
                        // let bindings are mutable regardless of initializer
                        if kind == AnalyzedBindingKind::Let {
                            reactivity_kind = ReactivityKind::Mutable;
                        }
                        let type_annotation = decl.type_annotation.as_ref().map(|ann| {
                            content[ann.type_annotation.span().start as usize
                                ..ann.type_annotation.span().end as usize]
                                .to_string()
                        });
                        bindings.push(AnalyzedBinding {
                            name: id.name.to_string(),
                            kind,
                            is_reactive,
                            reactivity_kind,
                            type_annotation,
                            initializer,
                            span_start: id.span.start,
                            span_end: id.span.end,
                        });
                    }

                    if !has_top_level_await {
                        if let Some(ref init) = decl.init {
                            if expr_contains_await(init) {
                                has_top_level_await = true;
                            }
                        }
                    }
                }
            }

            Statement::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id {
                    bindings.push(AnalyzedBinding {
                        name: id.name.to_string(),
                        kind: if func.r#async {
                            AnalyzedBindingKind::AsyncFunction
                        } else {
                            AnalyzedBindingKind::Function
                        },
                        is_reactive: false,
                        reactivity_kind: ReactivityKind::None,
                        type_annotation: None,
                        initializer: None,
                        span_start: id.span.start,
                        span_end: id.span.end,
                    });
                }
            }

            Statement::ClassDeclaration(cls) => {
                if let Some(ref id) = cls.id {
                    bindings.push(AnalyzedBinding {
                        name: id.name.to_string(),
                        kind: AnalyzedBindingKind::Class,
                        is_reactive: false,
                        reactivity_kind: ReactivityKind::None,
                        type_annotation: None,
                        initializer: None,
                        span_start: id.span.start,
                        span_end: id.span.end,
                    });
                }
            }

            // Collect local type inheritance for transitive dep discovery
            Statement::TSInterfaceDeclaration(iface) => {
                let mut bases = Vec::new();
                for heritage in &iface.extends {
                    if let Expression::Identifier(id) = &heritage.expression {
                        bases.push(id.name.to_string());
                    }
                }
                if !bases.is_empty() {
                    local_type_deps.insert(iface.id.name.to_string(), bases);
                }
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let refs = collect_type_references(&alias.type_annotation);
                if !refs.is_empty() {
                    local_type_deps.insert(alias.id.name.to_string(), refs);
                }
            }

            // Handle exported type declarations too
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::TSInterfaceDeclaration(iface) => {
                            let mut bases = Vec::new();
                            for heritage in &iface.extends {
                                if let Expression::Identifier(id) = &heritage.expression {
                                    bases.push(id.name.to_string());
                                }
                            }
                            if !bases.is_empty() {
                                local_type_deps.insert(iface.id.name.to_string(), bases);
                            }
                        }
                        Declaration::TSTypeAliasDeclaration(alias) => {
                            let refs = collect_type_references(&alias.type_annotation);
                            if !refs.is_empty() {
                                local_type_deps.insert(alias.id.name.to_string(), refs);
                            }
                        }
                        _ => {}
                    }
                }
            }

            Statement::ForOfStatement(for_of) if for_of.r#await => {
                has_top_level_await = true;
            }

            _ => {}
        }
    }

    // ── Derive: macro type deps ──
    let macro_type_deps = derive_macro_type_deps(&macros, &imports, &import_map, &local_type_deps);

    // ── Derive: flags ──
    let mut flags = derive_flags(&imports, &macros, &bindings, &macro_type_deps);
    if has_top_level_await {
        flags |= AnalysisFlags::ASYNC_SETUP;
    }

    // ── Exported function analysis (when FUNC_RETURNS scope is active) ──
    let exported_functions = if scope.contains(AnalysisScope::FUNC_RETURNS) {
        analyze_exported_functions(content, program, &import_map)
    } else {
        Vec::new()
    };

    ScriptAnalysisSnapshot {
        imports,
        bindings,
        macros,
        macro_type_deps,
        flags,
        exported_functions,
        type_enhancements: None,
    }
}

/// Build export signatures from source content using a single OXC parse.
pub fn build_export_signatures(
    content: &str,
    source_type: SourceType,
    allocator: &Allocator,
) -> Vec<ExportSignature> {
    let parser = Parser::new(allocator, content, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return Vec::new();
    }

    extract_export_signatures_from_program(content, &result.program)
}

/// Classify an initializer expression.
/// Returns (initializer, is_reactive, reactivity_kind).
fn classify_initializer(
    expr: &Expression<'_>,
    imports: &[AnalyzedImport],
    import_map: &ImportBindingMap,
) -> (Option<BindingInitializer>, bool, ReactivityKind) {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(callee_name) = call_callee_name(&call.callee) {
                let callee_import_source = import_map
                    .source(imports, callee_name)
                    .map(|s| s.to_string());
                let vue_api = import_map.vue_api(callee_name);
                let is_reactive = vue_api.map(is_reactivity_api).unwrap_or(false);
                let reactivity_kind = classify_reactivity_kind(vue_api, callee_name);

                return (
                    Some(BindingInitializer::FunctionCall {
                        callee: callee_name.to_string(),
                        callee_import_source,
                        vue_api,
                    }),
                    is_reactive,
                    reactivity_kind,
                );
            }
            (Some(BindingInitializer::Other), false, ReactivityKind::None)
        }
        Expression::Identifier(id) => (
            Some(BindingInitializer::Reference {
                name: id.name.to_string(),
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::String,
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::NumericLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Number,
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::BooleanLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Boolean,
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::NullLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Null,
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::ArrayExpression(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Array,
            }),
            false,
            ReactivityKind::None,
        ),
        Expression::ObjectExpression(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Object,
            }),
            false,
            ReactivityKind::None,
        ),
        _ => (Some(BindingInitializer::Other), false, ReactivityKind::None),
    }
}

/// Map Vue API classification and callee name to a granular `ReactivityKind`.
fn classify_reactivity_kind(
    vue_api: Option<VueApiClassification>,
    callee_name: &str,
) -> ReactivityKind {
    match vue_api {
        Some(
            VueApiClassification::Ref
            | VueApiClassification::ShallowRef
            | VueApiClassification::CustomRef
            | VueApiClassification::ToRef,
        ) => ReactivityKind::Ref,
        Some(VueApiClassification::Computed) => ReactivityKind::Computed,
        Some(VueApiClassification::Reactive | VueApiClassification::ShallowReactive) => {
            ReactivityKind::Reactive
        }
        _ => {
            // Composable convention: useXxx() → MaybeRef
            if callee_name.starts_with("use")
                && callee_name.len() > 3
                && callee_name.as_bytes()[3].is_ascii_uppercase()
            {
                ReactivityKind::MaybeRef
            } else {
                ReactivityKind::None
            }
        }
    }
}

fn call_callee_name<'a>(callee: &'a Expression<'a>) -> Option<&'a str> {
    match callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

// =============================================================================
// Phase 1c: Exported Function Analysis
// =============================================================================

/// Walk top-level statements to find exported functions and analyze them.
/// Handles: `export function foo()`, `export default function()`,
/// `export const foo = () => {}`, `export const foo = function() {}`.
fn analyze_exported_functions(
    content: &str,
    program: &Program<'_>,
    import_map: &ImportBindingMap,
) -> Vec<AnalyzedExportedFunction> {
    let mut out = Vec::new();

    for stmt in &program.body {
        match stmt {
            // export function foo() { ... }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(decl) = &export.declaration {
                    match decl {
                        Declaration::FunctionDeclaration(func) => {
                            if let Some(ref id) = func.id {
                                out.push(analyze_single_function(
                                    content,
                                    &id.name,
                                    false,
                                    func.r#async,
                                    &func.params,
                                    func.return_type.as_deref(),
                                    func.body.as_deref(),
                                    import_map,
                                ));
                            }
                        }
                        // export const foo = () => { ... }
                        Declaration::VariableDeclaration(var_decl) => {
                            for decl in &var_decl.declarations {
                                if let BindingPattern::BindingIdentifier(id) = &decl.id {
                                    if let Some(init) = &decl.init {
                                        if let Some(func) =
                                            extract_function_from_expr(content, init, import_map)
                                        {
                                            out.push(AnalyzedExportedFunction {
                                                name: id.name.to_string(),
                                                is_default: false,
                                                ..func
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // export default function() { ... }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    let name = func
                        .id
                        .as_ref()
                        .map(|id| id.name.to_string())
                        .unwrap_or_else(|| "default".to_string());
                    out.push(analyze_single_function(
                        content,
                        &name,
                        true,
                        func.r#async,
                        &func.params,
                        func.return_type.as_deref(),
                        func.body.as_deref(),
                        import_map,
                    ));
                }
                ExportDefaultDeclarationKind::ArrowFunctionExpression(arrow) => {
                    out.push(analyze_arrow_function(
                        content, "default", true, arrow, import_map,
                    ));
                }
                _ => {}
            },
            _ => {}
        }
    }

    out
}

/// Analyze a single named function (either `function` declaration or expression).
#[allow(clippy::too_many_arguments)]
fn analyze_single_function(
    content: &str,
    name: &str,
    is_default: bool,
    is_async: bool,
    params: &FormalParameters<'_>,
    return_type: Option<&oxc_ast::ast::TSTypeAnnotation<'_>>,
    body: Option<&FunctionBody<'_>>,
    import_map: &ImportBindingMap,
) -> AnalyzedExportedFunction {
    let extracted_params = extract_function_params(content, params);
    let return_type_annotation = return_type.map(|rt| {
        content[rt.type_annotation.span().start as usize..rt.type_annotation.span().end as usize]
            .to_string()
    });

    let return_reactivity = if let Some(annotation) = &return_type_annotation {
        classify_return_type_annotation(annotation)
    } else if let Some(body) = body {
        classify_return_reactivity_from_body(body, import_map)
    } else {
        ReturnReactivity::Unknown
    };

    let composable = if is_composable_name(name) {
        Some(build_composable_info(name, body, import_map))
    } else {
        None
    };

    AnalyzedExportedFunction {
        name: name.to_string(),
        is_default,
        params: extracted_params,
        return_type_annotation,
        return_reactivity,
        is_async,
        composable,
    }
}

/// Analyze an arrow function expression.
fn analyze_arrow_function(
    content: &str,
    name: &str,
    is_default: bool,
    arrow: &ArrowFunctionExpression<'_>,
    import_map: &ImportBindingMap,
) -> AnalyzedExportedFunction {
    let extracted_params = extract_function_params(content, &arrow.params);
    let return_type_annotation = arrow.return_type.as_ref().map(|rt| {
        content[rt.type_annotation.span().start as usize..rt.type_annotation.span().end as usize]
            .to_string()
    });

    // Arrow functions in OXC always have a FunctionBody.
    // Expression arrows have a single ExpressionStatement in the body.
    let body: &FunctionBody<'_> = &arrow.body;
    let return_reactivity = if let Some(annotation) = &return_type_annotation {
        classify_return_type_annotation(annotation)
    } else if arrow.expression {
        // Arrow with expression body: `() => ref(0)` — single expression statement
        if let Some(Statement::ExpressionStatement(expr_stmt)) = body.statements.first() {
            classify_single_return_expr(&expr_stmt.expression, import_map)
        } else {
            ReturnReactivity::Unknown
        }
    } else {
        classify_return_reactivity_from_body(body, import_map)
    };

    let composable = if is_composable_name(name) {
        Some(build_composable_info(name, Some(body), import_map))
    } else {
        None
    };

    AnalyzedExportedFunction {
        name: name.to_string(),
        is_default,
        params: extracted_params,
        return_type_annotation,
        return_reactivity,
        is_async: arrow.r#async,
        composable,
    }
}

/// Try to extract a function from an expression (arrow function or function expression).
fn extract_function_from_expr(
    content: &str,
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
) -> Option<AnalyzedExportedFunction> {
    match expr {
        Expression::ArrowFunctionExpression(arrow) => Some(analyze_arrow_function(
            content, "", false, arrow, import_map,
        )),
        Expression::FunctionExpression(func) => {
            let name = func
                .id
                .as_ref()
                .map(|id| id.name.to_string())
                .unwrap_or_default();
            Some(analyze_single_function(
                content,
                &name,
                false,
                func.r#async,
                &func.params,
                func.return_type.as_deref(),
                func.body.as_deref(),
                import_map,
            ))
        }
        _ => None,
    }
}

/// Extract function parameters with type annotations.
fn extract_function_params(content: &str, params: &FormalParameters<'_>) -> Vec<FunctionParam> {
    let mut out = Vec::new();
    for param in &params.items {
        let name = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => id.name.to_string(),
            _ => "_".to_string(), // destructured params
        };
        let type_annotation = param.type_annotation.as_ref().map(|ann| {
            content
                [ann.type_annotation.span().start as usize..ann.type_annotation.span().end as usize]
                .to_string()
        });
        let is_optional = param.optional;
        let has_default = param.initializer.is_some();

        out.push(FunctionParam {
            name,
            type_annotation,
            is_optional,
            has_default,
        });
    }
    out
}

/// Check if a name follows the composable convention (`useXxx`).
fn is_composable_name(name: &str) -> bool {
    name.starts_with("use") && name.len() > 3 && name.as_bytes()[3].is_ascii_uppercase()
}

/// Classify return reactivity from a TS return type annotation string.
/// Checks for well-known Vue type wrappers.
fn classify_return_type_annotation(annotation: &str) -> ReturnReactivity {
    let trimmed = annotation.trim();
    if trimmed.starts_with("Ref<")
        || trimmed.starts_with("ShallowRef<")
        || trimmed.starts_with("ComputedRef<")
    {
        ReturnReactivity::Ref
    } else if trimmed.starts_with("Reactive<") || trimmed.starts_with("ShallowReactive<") {
        ReturnReactivity::Reactive
    } else if trimmed == "void" || trimmed == "undefined" || trimmed == "never" {
        ReturnReactivity::Plain
    } else {
        // Cannot determine from annotation alone — could be object, union, etc.
        ReturnReactivity::Unknown
    }
}

/// Classify return reactivity by walking return statements in a function body (heuristic).
fn classify_return_reactivity_from_body(
    body: &FunctionBody<'_>,
    import_map: &ImportBindingMap,
) -> ReturnReactivity {
    let mut return_kinds = Vec::new();
    collect_return_expressions(body, import_map, &mut return_kinds);

    if return_kinds.is_empty() {
        return ReturnReactivity::Plain;
    }
    if return_kinds.len() == 1 {
        return return_kinds.into_iter().next().unwrap();
    }

    // Multiple return paths: check if they're all the same
    let first = &return_kinds[0];
    if return_kinds.iter().all(|k| k == first) {
        return_kinds.into_iter().next().unwrap()
    } else {
        ReturnReactivity::Unknown
    }
}

/// Collect return reactivity from all return statements in a function body.
/// Does NOT recurse into nested function/arrow boundaries.
fn collect_return_expressions(
    body: &FunctionBody<'_>,
    import_map: &ImportBindingMap,
    out: &mut Vec<ReturnReactivity>,
) {
    for stmt in &body.statements {
        collect_returns_from_stmt(stmt, import_map, out);
    }
}

/// Recursively walk statements for return expressions, stopping at function boundaries.
fn collect_returns_from_stmt(
    stmt: &Statement<'_>,
    import_map: &ImportBindingMap,
    out: &mut Vec<ReturnReactivity>,
) {
    match stmt {
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                out.push(classify_single_return_expr(arg, import_map));
            } else {
                out.push(ReturnReactivity::Plain);
            }
        }
        // Recurse into blocks
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_returns_from_stmt(s, import_map, out);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_returns_from_stmt(&if_stmt.consequent, import_map, out);
            if let Some(alt) = &if_stmt.alternate {
                collect_returns_from_stmt(alt, import_map, out);
            }
        }
        Statement::TryStatement(try_stmt) => {
            for s in &try_stmt.block.body {
                collect_returns_from_stmt(s, import_map, out);
            }
            if let Some(catch) = &try_stmt.handler {
                for s in &catch.body.body {
                    collect_returns_from_stmt(s, import_map, out);
                }
            }
            if let Some(fin) = &try_stmt.finalizer {
                for s in &fin.body {
                    collect_returns_from_stmt(s, import_map, out);
                }
            }
        }
        Statement::SwitchStatement(switch) => {
            for case in &switch.cases {
                for s in &case.consequent {
                    collect_returns_from_stmt(s, import_map, out);
                }
            }
        }
        // Do NOT recurse into function/arrow boundaries
        Statement::FunctionDeclaration(_) => {}
        _ => {}
    }
}

/// Classify a single return expression.
fn classify_single_return_expr(
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
) -> ReturnReactivity {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(callee_name) = call_callee_name(&call.callee) {
                let vue_api = import_map.vue_api(callee_name);
                match vue_api {
                    Some(
                        VueApiClassification::Ref
                        | VueApiClassification::ShallowRef
                        | VueApiClassification::CustomRef
                        | VueApiClassification::ToRef
                        | VueApiClassification::Computed,
                    ) => return ReturnReactivity::Ref,
                    Some(
                        VueApiClassification::Reactive | VueApiClassification::ShallowReactive,
                    ) => return ReturnReactivity::Reactive,
                    _ => {}
                }
            }
            ReturnReactivity::Unknown
        }
        Expression::ObjectExpression(obj) => {
            // Check if the object has reactive fields
            let mut fields = Vec::new();
            let mut has_any_reactive = false;
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(p) = prop {
                    if let Some(name) = property_key_name(&p.key) {
                        let kind = classify_value_reactivity(&p.value, import_map);
                        if kind != ReactivityKind::None {
                            has_any_reactive = true;
                        }
                        fields.push((name.to_string(), kind));
                    }
                }
            }
            if has_any_reactive {
                ReturnReactivity::ObjectWithReactiveFields(fields)
            } else {
                ReturnReactivity::Plain
            }
        }
        // Literals → Plain
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => ReturnReactivity::Plain,
        // Parenthesized → unwrap
        Expression::ParenthesizedExpression(p) => {
            classify_single_return_expr(&p.expression, import_map)
        }
        // TS assertions → unwrap
        Expression::TSAsExpression(e) => classify_single_return_expr(&e.expression, import_map),
        Expression::TSSatisfiesExpression(e) => {
            classify_single_return_expr(&e.expression, import_map)
        }
        _ => ReturnReactivity::Unknown,
    }
}

/// Classify the reactivity of a value expression (for object return fields).
fn classify_value_reactivity(
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
) -> ReactivityKind {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(callee_name) = call_callee_name(&call.callee) {
                let vue_api = import_map.vue_api(callee_name);
                return classify_reactivity_kind(vue_api, callee_name);
            }
            ReactivityKind::None
        }
        Expression::ParenthesizedExpression(p) => {
            classify_value_reactivity(&p.expression, import_map)
        }
        _ => ReactivityKind::None,
    }
}

/// Extract property key name from an object property key.
fn property_key_name<'a>(key: &'a PropertyKey<'a>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.as_str()),
        PropertyKey::StringLiteral(s) => Some(s.value.as_str()),
        _ => None,
    }
}

/// Build composable info by scanning the function body for Vue API usage.
fn build_composable_info(
    name: &str,
    body: Option<&FunctionBody<'_>>,
    import_map: &ImportBindingMap,
) -> ComposableInfo {
    let mut lifecycle_hooks = Vec::new();
    let mut has_provide = false;
    let mut has_inject = false;
    let mut has_watchers = false;
    let mut internal_reactive_state = Vec::new();

    if let Some(body) = body {
        scan_body_for_vue_apis(
            body,
            import_map,
            &mut lifecycle_hooks,
            &mut has_provide,
            &mut has_inject,
            &mut has_watchers,
            &mut internal_reactive_state,
        );
    }

    let return_shape = if let Some(body) = body {
        detect_composable_return_shape(body, import_map)
    } else {
        ComposableReturn::Unknown
    };

    ComposableInfo {
        name: name.to_string(),
        lifecycle_hooks,
        has_provide,
        has_inject,
        has_watchers,
        internal_reactive_state,
        return_shape,
    }
}

/// Scan a function body for Vue API calls (lifecycle hooks, provide, inject, watchers, reactive state).
#[allow(clippy::too_many_arguments)]
fn scan_body_for_vue_apis(
    body: &FunctionBody<'_>,
    import_map: &ImportBindingMap,
    lifecycle_hooks: &mut Vec<VueApiClassification>,
    has_provide: &mut bool,
    has_inject: &mut bool,
    has_watchers: &mut bool,
    internal_reactive_state: &mut Vec<(String, ReactivityKind)>,
) {
    for stmt in &body.statements {
        scan_stmt_for_vue_apis(
            stmt,
            import_map,
            lifecycle_hooks,
            has_provide,
            has_inject,
            has_watchers,
            internal_reactive_state,
        );
    }
}

/// Recursively scan a statement for Vue API calls, stopping at function boundaries.
#[allow(clippy::too_many_arguments)]
fn scan_stmt_for_vue_apis(
    stmt: &Statement<'_>,
    import_map: &ImportBindingMap,
    lifecycle_hooks: &mut Vec<VueApiClassification>,
    has_provide: &mut bool,
    has_inject: &mut bool,
    has_watchers: &mut bool,
    internal_reactive_state: &mut Vec<(String, ReactivityKind)>,
) {
    match stmt {
        Statement::ExpressionStatement(expr_stmt) => {
            scan_expr_for_vue_call(
                &expr_stmt.expression,
                import_map,
                lifecycle_hooks,
                has_provide,
                has_inject,
                has_watchers,
            );
        }
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(init) = &decl.init {
                    // Check for reactive state creation
                    if let Expression::CallExpression(call) = init {
                        if let Some(callee_name) = call_callee_name(&call.callee) {
                            let vue_api = import_map.vue_api(callee_name);
                            let kind = classify_reactivity_kind(vue_api, callee_name);
                            if kind != ReactivityKind::None {
                                let name = match &decl.id {
                                    BindingPattern::BindingIdentifier(id) => id.name.to_string(),
                                    _ => "_".to_string(),
                                };
                                internal_reactive_state.push((name, kind));
                            }
                            // Also check for API calls
                            scan_expr_for_vue_call(
                                init,
                                import_map,
                                lifecycle_hooks,
                                has_provide,
                                has_inject,
                                has_watchers,
                            );
                        }
                    }
                }
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                scan_stmt_for_vue_apis(
                    s,
                    import_map,
                    lifecycle_hooks,
                    has_provide,
                    has_inject,
                    has_watchers,
                    internal_reactive_state,
                );
            }
        }
        // Don't recurse into nested functions
        Statement::FunctionDeclaration(_) => {}
        _ => {}
    }
}

/// Scan an expression for Vue API function calls.
fn scan_expr_for_vue_call(
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
    lifecycle_hooks: &mut Vec<VueApiClassification>,
    has_provide: &mut bool,
    has_inject: &mut bool,
    has_watchers: &mut bool,
) {
    if let Expression::CallExpression(call) = expr {
        if let Some(callee_name) = call_callee_name(&call.callee) {
            if let Some(api) = import_map.vue_api(callee_name) {
                if is_lifecycle_api(api) {
                    lifecycle_hooks.push(api);
                }
                if matches!(api, VueApiClassification::Provide) {
                    *has_provide = true;
                }
                if matches!(api, VueApiClassification::Inject) {
                    *has_inject = true;
                }
                if is_watcher_api(api) {
                    *has_watchers = true;
                }
            }
        }
    }
}

/// Detect the return shape of a composable function.
fn detect_composable_return_shape(
    body: &FunctionBody<'_>,
    import_map: &ImportBindingMap,
) -> ComposableReturn {
    // Find the last return statement's expression
    let mut returns = Vec::new();
    collect_return_expressions(body, import_map, &mut returns);

    if returns.is_empty() {
        return ComposableReturn::Unknown;
    }
    if returns.len() == 1 {
        return match &returns[0] {
            ReturnReactivity::Ref | ReturnReactivity::Reactive => {
                ComposableReturn::Single(match &returns[0] {
                    ReturnReactivity::Ref => ReactivityKind::Ref,
                    ReturnReactivity::Reactive => ReactivityKind::Reactive,
                    _ => ReactivityKind::None,
                })
            }
            ReturnReactivity::ObjectWithReactiveFields(fields) => {
                ComposableReturn::Object(
                    fields
                        .iter()
                        .map(|(name, kind)| ComposableReturnField {
                            name: name.clone(),
                            reactivity: *kind,
                            is_function: false, // Cannot determine from heuristic
                        })
                        .collect(),
                )
            }
            ReturnReactivity::Plain => ComposableReturn::Single(ReactivityKind::None),
            ReturnReactivity::Unknown => ComposableReturn::Unknown,
        };
    }

    ComposableReturn::Unknown
}

/// Check if an expression contains an `await` expression at any nesting depth.
/// Walks the expression tree but stops at function/arrow boundaries (those
/// create their own async context and don't make the *setup* async).
fn expr_contains_await(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::AwaitExpression(_) => true,
        // Stop at function boundaries — await inside these doesn't make setup async
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => false,
        // Recurse into call arguments
        Expression::CallExpression(call) => {
            call.arguments
                .iter()
                .any(|arg| arg.as_expression().is_some_and(expr_contains_await))
                || expr_contains_await(&call.callee)
        }
        // Recurse into array elements
        Expression::ArrayExpression(arr) => arr.elements.iter().any(|elem| match elem {
            oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) => {
                expr_contains_await(&s.argument)
            }
            oxc_ast::ast::ArrayExpressionElement::Elision(_) => false,
            _ => elem.as_expression().is_some_and(expr_contains_await),
        }),
        // Recurse into object properties
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| match prop {
            oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => expr_contains_await(&p.value),
            oxc_ast::ast::ObjectPropertyKind::SpreadProperty(s) => expr_contains_await(&s.argument),
        }),
        // Recurse into conditionals
        Expression::ConditionalExpression(cond) => {
            expr_contains_await(&cond.test)
                || expr_contains_await(&cond.consequent)
                || expr_contains_await(&cond.alternate)
        }
        // Recurse into binary/logical
        Expression::BinaryExpression(bin) => {
            expr_contains_await(&bin.left) || expr_contains_await(&bin.right)
        }
        Expression::LogicalExpression(log) => {
            expr_contains_await(&log.left) || expr_contains_await(&log.right)
        }
        // Recurse into assignment
        Expression::AssignmentExpression(assign) => expr_contains_await(&assign.right),
        // Recurse into sequence (comma operator)
        Expression::SequenceExpression(seq) => seq.expressions.iter().any(expr_contains_await),
        // Recurse into unary/update
        Expression::UnaryExpression(un) => expr_contains_await(&un.argument),
        // Recurse into template literal expressions
        Expression::TemplateLiteral(tpl) => tpl.expressions.iter().any(expr_contains_await),
        // Recurse into tagged template
        Expression::TaggedTemplateExpression(tagged) => {
            expr_contains_await(&tagged.tag)
                || tagged.quasi.expressions.iter().any(expr_contains_await)
        }
        // Recurse into member expressions
        Expression::ComputedMemberExpression(m) => {
            expr_contains_await(&m.object) || expr_contains_await(&m.expression)
        }
        Expression::StaticMemberExpression(m) => expr_contains_await(&m.object),
        // Recurse into parenthesized
        Expression::ParenthesizedExpression(p) => expr_contains_await(&p.expression),
        // Recurse into spread-like (yield, etc)
        Expression::YieldExpression(y) => {
            y.argument.as_ref().is_some_and(|a| expr_contains_await(a))
        }
        // TS non-null / as / satisfies
        Expression::TSNonNullExpression(e) => expr_contains_await(&e.expression),
        Expression::TSAsExpression(e) => expr_contains_await(&e.expression),
        Expression::TSSatisfiesExpression(e) => expr_contains_await(&e.expression),
        Expression::TSTypeAssertion(e) => expr_contains_await(&e.expression),
        // All other expressions (literals, identifiers, etc.) — no await
        _ => false,
    }
}

/// Match macro type references against import bindings to produce dependency entries.
/// Also follows local type chains (extends/intersection) to discover transitive deps.
fn derive_macro_type_deps(
    macros: &[AnalyzedMacro],
    imports: &[AnalyzedImport],
    import_map: &ImportBindingMap,
    local_type_deps: &FxHashMap<String, Vec<String>>,
) -> Vec<MacroTypeDep> {
    let mut deps = Vec::new();
    let mut seen_deps = FxHashSet::default();

    for m in macros {
        if !m.is_type_based {
            continue;
        }
        for type_ref_name in &m.type_references {
            // Direct import match
            if let Some(source) = import_map.source(imports, type_ref_name) {
                if seen_deps.insert((type_ref_name.clone(), m.kind)) {
                    deps.push(MacroTypeDep {
                        type_name: type_ref_name.clone(),
                        import_source: source.to_string(),
                        macro_kind: m.kind,
                    });
                }
            } else {
                // Follow local type chains to find transitive imported deps
                collect_transitive_deps(
                    type_ref_name,
                    m.kind,
                    imports,
                    import_map,
                    local_type_deps,
                    &mut deps,
                    &mut seen_deps,
                    &mut FxHashSet::default(),
                );
            }
        }
    }

    deps
}

/// Recursively follow local type extends/refs to find transitively imported types.
#[allow(clippy::too_many_arguments)]
fn collect_transitive_deps(
    type_name: &str,
    macro_kind: AnalyzedMacroKind,
    imports: &[AnalyzedImport],
    import_map: &ImportBindingMap,
    local_type_deps: &FxHashMap<String, Vec<String>>,
    deps: &mut Vec<MacroTypeDep>,
    seen_deps: &mut FxHashSet<(String, AnalyzedMacroKind)>,
    visited: &mut FxHashSet<String>,
) {
    if !visited.insert(type_name.to_string()) {
        return; // Avoid cycles
    }

    if let Some(base_refs) = local_type_deps.get(type_name) {
        for base_name in base_refs {
            if let Some(source) = import_map.source(imports, base_name) {
                // Found an imported base type
                if seen_deps.insert((base_name.clone(), macro_kind)) {
                    deps.push(MacroTypeDep {
                        type_name: base_name.clone(),
                        import_source: source.to_string(),
                        macro_kind,
                    });
                }
            } else {
                // Base is also local — recurse
                collect_transitive_deps(
                    base_name,
                    macro_kind,
                    imports,
                    import_map,
                    local_type_deps,
                    deps,
                    seen_deps,
                    visited,
                );
            }
        }
    }
}

fn derive_flags(
    imports: &[AnalyzedImport],
    macros: &[AnalyzedMacro],
    bindings: &[AnalyzedBinding],
    macro_type_deps: &[MacroTypeDep],
) -> AnalysisFlags {
    let mut flags = AnalysisFlags::empty();

    // Macro flags
    for m in macros {
        match m.kind {
            AnalyzedMacroKind::DefineProps => {
                flags |= AnalysisFlags::HAS_DEFINE_PROPS;
                if m.is_type_based {
                    flags |= AnalysisFlags::HAS_TYPE_BASED_PROPS;
                }
            }
            AnalyzedMacroKind::DefineEmits => {
                flags |= AnalysisFlags::HAS_DEFINE_EMITS;
                if m.is_type_based {
                    flags |= AnalysisFlags::HAS_TYPE_BASED_EMITS;
                }
            }
            AnalyzedMacroKind::DefineModel => {
                flags |= AnalysisFlags::HAS_DEFINE_MODEL;
                if m.is_type_based {
                    flags |= AnalysisFlags::HAS_TYPE_BASED_MODEL;
                }
            }
            AnalyzedMacroKind::DefineExpose => flags |= AnalysisFlags::HAS_DEFINE_EXPOSE,
            AnalyzedMacroKind::DefineOptions => flags |= AnalysisFlags::HAS_DEFINE_OPTIONS,
            AnalyzedMacroKind::DefineSlots => flags |= AnalysisFlags::HAS_DEFINE_SLOTS,
            AnalyzedMacroKind::WithDefaults => flags |= AnalysisFlags::HAS_WITH_DEFAULTS,
        }
    }

    // Import-based flags
    for imp in imports {
        for b in &imp.bindings {
            if let Some(api) = b.vue_api {
                if is_reactivity_api(api) {
                    flags |= AnalysisFlags::HAS_REACTIVE_STATE;
                }
                if matches!(api, VueApiClassification::Computed) {
                    flags |= AnalysisFlags::HAS_COMPUTED;
                }
                if is_watcher_api(api) {
                    flags |= AnalysisFlags::HAS_WATCHERS;
                }
                if is_lifecycle_api(api) {
                    flags |= AnalysisFlags::HAS_LIFECYCLE_HOOKS;
                }
                if matches!(api, VueApiClassification::Provide) {
                    flags |= AnalysisFlags::HAS_PROVIDE;
                }
                if matches!(api, VueApiClassification::Inject) {
                    flags |= AnalysisFlags::HAS_INJECT;
                }
            }
        }
    }

    // Binding-based flags
    for b in bindings {
        if b.is_reactive {
            flags |= AnalysisFlags::HAS_REACTIVE_STATE;
        }
    }

    // External type dep flag
    if !macro_type_deps.is_empty() {
        flags |= AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS;
    }

    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(code: &str) -> ScriptAnalysisSnapshot {
        let alloc = Allocator::new();
        build_script_analysis(code, SourceType::ts(), &alloc)
    }

    /// @ai-generated - Vue API imports are classified
    #[test]
    fn vue_imports_classified() {
        let result = analyze("import { ref, MyType } from 'vue';");
        assert_eq!(result.imports.len(), 1);
        assert_eq!(
            result.imports[0].bindings[0].vue_api,
            Some(VueApiClassification::Ref)
        );
        assert_eq!(
            result.imports[0].bindings[1].vue_api,
            Some(VueApiClassification::Other)
        );
    }

    /// @ai-generated - Binding with ref() initializer is reactive
    #[test]
    fn ref_binding_is_reactive() {
        let result = analyze("import { ref } from 'vue';\nconst count = ref(0);");
        assert!(result
            .bindings
            .iter()
            .any(|b| b.name == "count" && b.is_reactive));
        assert!(result.flags.contains(AnalysisFlags::HAS_REACTIVE_STATE));
    }

    /// @ai-generated - defineProps with type params detected
    #[test]
    fn define_props_type_based() {
        let code = r#"
import type { MyType } from './types';
defineProps<{foo: MyType}>();
"#;
        let result = analyze(code);
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_PROPS));
        assert!(result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_PROPS));
        assert!(result.flags.contains(AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS));

        assert_eq!(result.macro_type_deps.len(), 1);
        assert_eq!(result.macro_type_deps[0].type_name, "MyType");
        assert_eq!(result.macro_type_deps[0].import_source, "./types");
        assert_eq!(
            result.macro_type_deps[0].macro_kind,
            AnalyzedMacroKind::DefineProps
        );
    }

    /// @ai-generated - defineProps with runtime syntax (no type deps)
    #[test]
    fn define_props_runtime() {
        let result = analyze("defineProps({foo: String});");
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_PROPS));
        assert!(!result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_PROPS));
        assert!(result.macro_type_deps.is_empty());
    }

    /// @ai-generated - Non-function-call binding initializer
    #[test]
    fn literal_binding() {
        let result = analyze("const x = 42;");
        assert_eq!(result.bindings.len(), 1);
        assert_eq!(result.bindings[0].name, "x");
        assert!(!result.bindings[0].is_reactive);
        assert!(matches!(
            result.bindings[0].initializer,
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Number
            })
        ));
    }

    /// @ai-generated - Function call binding without Vue API
    #[test]
    fn non_vue_function_call() {
        let result = analyze("const data = fetchData();");
        assert_eq!(result.bindings.len(), 1);
        assert!(!result.bindings[0].is_reactive);
        assert!(matches!(
            result.bindings[0].initializer,
            Some(BindingInitializer::FunctionCall { ref callee, vue_api: None, .. }) if callee == "fetchData"
        ));
    }

    /// @ai-generated - Lifecycle hooks detected via flags
    #[test]
    fn lifecycle_hooks_flag() {
        let result = analyze("import { onMounted } from 'vue';");
        assert!(result.flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS));
    }

    /// @ai-generated - Watchers detected via flags
    #[test]
    fn watcher_flag() {
        let result = analyze("import { watch, watchEffect } from 'vue';");
        assert!(result.flags.contains(AnalysisFlags::HAS_WATCHERS));
    }

    /// @ai-generated - Provide/inject flags
    #[test]
    fn provide_inject_flags() {
        let result = analyze("import { provide, inject } from 'vue';");
        assert!(result.flags.contains(AnalysisFlags::HAS_PROVIDE));
        assert!(result.flags.contains(AnalysisFlags::HAS_INJECT));
    }

    /// @ai-generated - Multiple macros in one script
    #[test]
    fn multiple_macros() {
        let code = r#"
import type { Props } from './types';
const props = defineProps<Props>();
const emit = defineEmits<{(e: 'click'): void}>();
defineExpose({ props });
"#;
        let result = analyze(code);
        assert_eq!(result.macros.len(), 3);
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_PROPS));
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_EMITS));
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_EXPOSE));
    }

    /// @ai-generated - Empty / parse-error content returns default
    #[test]
    fn empty_content() {
        let result = analyze("");
        assert!(result.imports.is_empty());
        assert!(result.macros.is_empty());
        assert!(result.bindings.is_empty());
        assert_eq!(result.flags, AnalysisFlags::empty());
    }

    /// @ai-generated - Function and class bindings
    #[test]
    fn function_and_class_bindings() {
        let result = analyze("function helper() {}\nclass MyClass {}");
        let names: Vec<&str> = result.bindings.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"helper"));
        assert!(names.contains(&"MyClass"));
    }

    /// @ai-generated - Type reference from non-relative import (node_modules)
    #[test]
    fn type_from_bare_specifier() {
        let code = r#"
import type { PropType } from 'vue';
defineProps<{foo: PropType<string>}>();
"#;
        let result = analyze(code);
        assert!(result.flags.contains(AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS));
        assert!(result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "PropType" && d.import_source == "vue"));
    }

    /// @ai-generated - defineModel with type param
    #[test]
    fn define_model_type_based() {
        let result = analyze("const model = defineModel<string>();");
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_MODEL));
        assert!(result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_MODEL));
    }

    /// @ai-generated - Export signatures from build_export_signatures
    #[test]
    fn export_signatures() {
        let alloc = Allocator::new();
        let sigs = build_export_signatures(
            "export interface MyType { foo: string }\nexport const X = 1;",
            SourceType::ts(),
            &alloc,
        );
        assert_eq!(sigs.len(), 2);
        let names: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyType"));
        assert!(names.contains(&"X"));
    }

    /// @ai-generated - Aliased ref import: binding initialized with alias is still reactive
    #[test]
    fn aliased_ref_binding_is_reactive() {
        let result =
            analyze("import { ref as createRef } from 'vue';\nconst count = createRef(0);");
        assert!(
            result
                .bindings
                .iter()
                .any(|b| b.name == "count" && b.is_reactive),
            "binding initialized with aliased ref should still be detected as reactive"
        );
        assert!(result.flags.contains(AnalysisFlags::HAS_REACTIVE_STATE));
    }

    /// @ai-generated - withDefaults(defineProps<>(), {}) should detect both macros
    #[test]
    fn with_defaults_nested_define_props() {
        let code = r#"
import type { MyType } from './types';
const props = withDefaults(defineProps<{foo: MyType}>(), { foo: 'bar' });
"#;
        let result = analyze(code);
        assert!(
            result.flags.contains(AnalysisFlags::HAS_WITH_DEFAULTS),
            "should detect withDefaults"
        );
        assert!(
            result.flags.contains(AnalysisFlags::HAS_DEFINE_PROPS),
            "should detect nested defineProps"
        );
        assert!(
            result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_PROPS),
            "should detect type-based props from nested defineProps"
        );
        // Should have both macros: withDefaults AND defineProps
        assert!(
            result.macros.len() >= 2,
            "should have at least 2 macros (withDefaults + defineProps), got {}",
            result.macros.len()
        );
        assert!(
            result
                .macros
                .iter()
                .any(|m| m.kind == AnalyzedMacroKind::DefineProps),
            "should have a DefineProps macro entry"
        );
        // The nested defineProps type refs should produce macro_type_deps
        assert!(
            result.flags.contains(AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS),
            "should detect external type deps from nested defineProps"
        );
    }

    /// @ai-generated - ASYNC_SETUP flag set for top-level await in variable initializer
    #[test]
    fn async_setup_flag_for_top_level_await() {
        let result = analyze("const data = await fetchData();");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "top-level await in variable initializer should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP flag set for top-level await expression statement
    #[test]
    fn async_setup_flag_for_await_expression_statement() {
        let result = analyze("await someAsyncCall();");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "top-level await expression statement should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - Destructured bindings are not captured as individual bindings
    #[test]
    fn destructured_bindings_not_captured() {
        let result = analyze("const { a, b } = someObject;");
        // Destructured bindings are not captured individually since we only
        // handle BindingIdentifier patterns, not ObjectPattern/ArrayPattern
        assert_eq!(
            result.bindings.len(),
            0,
            "destructured bindings should not be captured as individual bindings"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Transitive type dep discovery tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Local interface extending imported type creates transitive dep
    #[test]
    fn transitive_dep_interface_extends_imported() {
        let code = r#"
import type { Base } from './types';
interface Local extends Base { own: string }
defineProps<Local>();
"#;
        let result = analyze(code);
        assert!(result.flags.contains(AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS));
        assert!(
            result
                .macro_type_deps
                .iter()
                .any(|d| d.type_name == "Base" && d.import_source == "./types"),
            "should discover Base as transitive dep via Local extends Base, got: {:?}",
            result.macro_type_deps
        );
    }

    /// @ai-generated - Local interface extending multiple imported types
    #[test]
    fn transitive_dep_multiple_extends() {
        let code = r#"
import type { A } from './a';
import type { B } from './b';
interface Local extends A, B { own: string }
defineProps<Local>();
"#;
        let result = analyze(code);
        assert_eq!(
            result.macro_type_deps.len(),
            2,
            "should have 2 transitive deps (A and B), got: {:?}",
            result.macro_type_deps
        );
        assert!(result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "A" && d.import_source == "./a"));
        assert!(result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "B" && d.import_source == "./b"));
    }

    /// @ai-generated - Type alias with intersection referencing imported type
    #[test]
    fn transitive_dep_type_alias_intersection() {
        let code = r#"
import type { Base } from './types';
type Local = Base & { extra: string };
defineProps<Local>();
"#;
        let result = analyze(code);
        assert!(
            result
                .macro_type_deps
                .iter()
                .any(|d| d.type_name == "Base" && d.import_source == "./types"),
            "should discover Base via type alias intersection, got: {:?}",
            result.macro_type_deps
        );
    }

    /// @ai-generated - Deep transitive chain: C extends B extends imported A
    #[test]
    fn transitive_dep_deep_chain() {
        let code = r#"
import type { A } from './types';
interface B extends A { b: number }
interface C extends B { c: boolean }
defineProps<C>();
"#;
        let result = analyze(code);
        assert!(
            result
                .macro_type_deps
                .iter()
                .any(|d| d.type_name == "A" && d.import_source == "./types"),
            "should discover A via C -> B -> A chain, got: {:?}",
            result.macro_type_deps
        );
    }

    /// @ai-generated - ASYNC_SETUP flag for nested await in function call argument
    #[test]
    fn async_setup_nested_await_in_call_arg() {
        let result = analyze("const data = bar(await fetchData());");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "nested await in function call argument should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP flag for await in array expression
    #[test]
    fn async_setup_await_in_array() {
        let result = analyze("const data = [await a(), await b()];");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "await in array expression should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP flag for await in ternary
    #[test]
    fn async_setup_await_in_ternary() {
        let result = analyze("const data = cond ? await fetchA() : fallback;");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "await in ternary expression should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP should NOT be set for await inside arrow function
    #[test]
    fn async_setup_not_set_for_await_in_arrow() {
        let result = analyze("const fn = async () => await fetchData();");
        assert!(
            !result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "await inside arrow function body should NOT set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP should NOT be set for await inside function expression
    #[test]
    fn async_setup_not_set_for_await_in_function_expr() {
        let result = analyze("const fn = async function() { await fetchData(); };");
        assert!(
            !result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "await inside function expression body should NOT set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - ASYNC_SETUP for await in object property value
    #[test]
    fn async_setup_await_in_object_value() {
        let result = analyze("const data = { result: await fetch() };");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "await in object property value should set ASYNC_SETUP flag"
        );
    }

    /// @ai-generated - Diamond inheritance: D extends B, C; B extends A; C extends A
    /// Should find A only once despite two paths
    #[test]
    fn transitive_dep_diamond_inheritance() {
        let code = r#"
import type { A } from './types';
interface B extends A { b: number }
interface C extends A { c: string }
interface D extends B, C { d: boolean }
defineProps<D>();
"#;
        let result = analyze(code);
        assert!(result.flags.contains(AnalysisFlags::HAS_EXTERNAL_TYPE_DEPS));
        // A should appear exactly once despite diamond
        let a_deps: Vec<_> = result
            .macro_type_deps
            .iter()
            .filter(|d| d.type_name == "A")
            .collect();
        assert_eq!(
            a_deps.len(),
            1,
            "A should appear exactly once in deps despite diamond inheritance, got: {:?}",
            result.macro_type_deps
        );
    }

    /// @ai-generated - Multiple defineModel() calls
    #[test]
    fn multiple_define_model_calls() {
        let code = r#"
const model1 = defineModel<string>();
const model2 = defineModel<number>('count');
"#;
        let result = analyze(code);
        assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_MODEL));
        let model_macros: Vec<_> = result
            .macros
            .iter()
            .filter(|m| m.kind == AnalyzedMacroKind::DefineModel)
            .collect();
        assert_eq!(
            model_macros.len(),
            2,
            "should detect both defineModel calls"
        );
    }

    /// @ai-generated - ASYNC_SETUP flag for `for await...of` statement
    #[test]
    fn async_setup_for_await_of() {
        let result = analyze("for await (const item of asyncIterable) {}");
        assert!(
            result.flags.contains(AnalysisFlags::ASYNC_SETUP),
            "for await...of should set ASYNC_SETUP flag"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 1a: Span information tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Binding span matches the identifier position in source
    #[test]
    fn binding_span_matches_source_position() {
        let code = "const count = ref(0);";
        let result = analyze(code);
        assert_eq!(result.bindings.len(), 1);
        let b = &result.bindings[0];
        // "count" starts at position 6, ends at 11
        assert_eq!(&code[b.span_start as usize..b.span_end as usize], "count");
    }

    /// @ai-generated - Import span covers full declaration
    #[test]
    fn import_span_covers_full_declaration() {
        let code = "import { ref } from 'vue';";
        let result = analyze(code);
        assert_eq!(result.imports.len(), 1);
        let imp = &result.imports[0];
        assert_eq!(
            &code[imp.span_start as usize..imp.span_end as usize],
            "import { ref } from 'vue';"
        );
    }

    /// @ai-generated - Import binding span covers the specifier name
    #[test]
    fn import_binding_span_covers_specifier() {
        let code = "import { ref, computed } from 'vue';";
        let result = analyze(code);
        let bindings = &result.imports[0].bindings;
        assert_eq!(bindings.len(), 2);
        assert_eq!(
            &code[bindings[0].span_start as usize..bindings[0].span_end as usize],
            "ref"
        );
        assert_eq!(
            &code[bindings[1].span_start as usize..bindings[1].span_end as usize],
            "computed"
        );
    }

    /// @ai-generated - Macro span covers the call expression
    #[test]
    fn macro_span_covers_call_expression() {
        let code = "defineProps<{msg: string}>();";
        let result = analyze(code);
        assert_eq!(result.macros.len(), 1);
        let m = &result.macros[0];
        // The call span should cover the entire call expression
        let span_text = &code[m.span_start as usize..m.span_end as usize];
        assert!(
            span_text.starts_with("defineProps"),
            "macro span should start with defineProps, got: {}",
            span_text
        );
        assert!(
            span_text.ends_with("()"),
            "macro span should end with (), got: {}",
            span_text
        );
    }

    /// @ai-generated - Function declaration span covers the function name
    #[test]
    fn function_binding_span_covers_name() {
        let code = "function handleClick() {}";
        let result = analyze(code);
        assert_eq!(result.bindings.len(), 1);
        let b = &result.bindings[0];
        assert_eq!(
            &code[b.span_start as usize..b.span_end as usize],
            "handleClick"
        );
    }

    /// @ai-generated - Class declaration span covers the class name
    #[test]
    fn class_binding_span_covers_name() {
        let code = "class MyService {}";
        let result = analyze(code);
        assert_eq!(result.bindings.len(), 1);
        let b = &result.bindings[0];
        assert_eq!(
            &code[b.span_start as usize..b.span_end as usize],
            "MyService"
        );
    }

    /// @ai-generated - Multiple bindings have distinct non-overlapping spans
    #[test]
    fn multiple_binding_spans_distinct() {
        let code = "const a = 1;\nconst b = 2;";
        let result = analyze(code);
        assert_eq!(result.bindings.len(), 2);
        let a = &result.bindings[0];
        let b = &result.bindings[1];
        assert_eq!(&code[a.span_start as usize..a.span_end as usize], "a");
        assert_eq!(&code[b.span_start as usize..b.span_end as usize], "b");
        // Ensure spans don't overlap
        assert!(a.span_end <= b.span_start);
    }

    /// @ai-generated - Import resolved_canonical_id is None by default
    #[test]
    fn import_resolved_canonical_id_none_by_default() {
        let result = analyze("import { ref } from 'vue';");
        assert!(result.imports[0].resolved_canonical_id.is_none());
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 1b: ReactivityKind classification tests
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - ref() classified as ReactivityKind::Ref
    #[test]
    fn ref_classified_as_ref_kind() {
        let result = analyze("import { ref } from 'vue';\nconst count = ref(0);");
        let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
    }

    /// @ai-generated - computed() classified as ReactivityKind::Computed
    #[test]
    fn computed_classified_as_computed_kind() {
        let result = analyze("import { computed } from 'vue';\nconst doubled = computed(() => 2);");
        let b = result
            .bindings
            .iter()
            .find(|b| b.name == "doubled")
            .unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Computed);
    }

    /// @ai-generated - reactive() classified as ReactivityKind::Reactive
    #[test]
    fn reactive_classified_as_reactive_kind() {
        let result = analyze("import { reactive } from 'vue';\nconst state = reactive({ x: 1 });");
        let b = result.bindings.iter().find(|b| b.name == "state").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Reactive);
    }

    /// @ai-generated - shallowRef() classified as ReactivityKind::Ref
    #[test]
    fn shallow_ref_classified_as_ref_kind() {
        let result = analyze("import { shallowRef } from 'vue';\nconst data = shallowRef(null);");
        let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
    }

    /// @ai-generated - shallowReactive() classified as ReactivityKind::Reactive
    #[test]
    fn shallow_reactive_classified_as_reactive_kind() {
        let result =
            analyze("import { shallowReactive } from 'vue';\nconst state = shallowReactive({});");
        let b = result.bindings.iter().find(|b| b.name == "state").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Reactive);
    }

    /// @ai-generated - customRef() classified as ReactivityKind::Ref
    #[test]
    fn custom_ref_classified_as_ref_kind() {
        let result =
            analyze("import { customRef } from 'vue';\nconst val = customRef((track, trigger) => ({ get() { return 1 }, set() {} }));");
        let b = result.bindings.iter().find(|b| b.name == "val").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
    }

    /// @ai-generated - toRef() classified as ReactivityKind::Ref
    #[test]
    fn to_ref_classified_as_ref_kind() {
        let result = analyze("import { toRef } from 'vue';\nconst name = toRef(props, 'name');");
        let b = result.bindings.iter().find(|b| b.name == "name").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
    }

    /// @ai-generated - useXxx() composable classified as ReactivityKind::MaybeRef
    #[test]
    fn composable_use_prefix_classified_as_maybe_ref() {
        let result = analyze("const data = useFetch('/api');");
        let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::MaybeRef);
    }

    /// @ai-generated - let binding classified as ReactivityKind::Mutable
    #[test]
    fn let_binding_classified_as_mutable() {
        let result = analyze("let count = 0;");
        let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::Mutable);
    }

    /// @ai-generated - let binding with reactive initializer still classified as Mutable
    #[test]
    fn let_binding_with_ref_still_mutable() {
        let result = analyze("import { ref } from 'vue';\nlet count = ref(0);");
        let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
        assert_eq!(
            b.reactivity_kind,
            ReactivityKind::Mutable,
            "let bindings should be Mutable even if initialized with ref()"
        );
        // But is_reactive should still be true
        assert!(b.is_reactive);
    }

    /// @ai-generated - const literal classified as ReactivityKind::None
    #[test]
    fn const_literal_classified_as_none() {
        let result = analyze("const MAX = 100;");
        let b = result.bindings.iter().find(|b| b.name == "MAX").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::None);
    }

    /// @ai-generated - function declaration classified as ReactivityKind::None
    #[test]
    fn function_decl_classified_as_none() {
        let result = analyze("function helper() { return 42; }");
        let b = result.bindings.iter().find(|b| b.name == "helper").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::None);
    }

    /// @ai-generated - class declaration classified as ReactivityKind::None
    #[test]
    fn class_decl_classified_as_none() {
        let result = analyze("class MyService {}");
        let b = result
            .bindings
            .iter()
            .find(|b| b.name == "MyService")
            .unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::None);
    }

    /// @ai-generated - Non-Vue function call classified as ReactivityKind::None
    #[test]
    fn non_vue_call_classified_as_none() {
        let result = analyze("const data = fetchData();");
        let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
        assert_eq!(b.reactivity_kind, ReactivityKind::None);
    }

    /// @ai-generated - Short "use" prefix not treated as composable (useX needs 4+ chars with uppercase)
    #[test]
    fn short_use_prefix_not_composable() {
        let result = analyze("const x = use();");
        let b = result.bindings.iter().find(|b| b.name == "x").unwrap();
        assert_eq!(
            b.reactivity_kind,
            ReactivityKind::None,
            "use() is too short to be a composable"
        );
    }

    /// @ai-generated - useXxx with lowercase x not treated as composable
    #[test]
    fn use_lowercase_not_composable() {
        let result = analyze("const x = useful();");
        let b = result.bindings.iter().find(|b| b.name == "x").unwrap();
        assert_eq!(
            b.reactivity_kind,
            ReactivityKind::None,
            "useful() doesn't follow the useXxx convention (4th char not uppercase)"
        );
    }

    /// @ai-generated - Type annotation is extracted from variable declarations
    #[test]
    fn type_annotation_extracted() {
        let code = "const count: Ref<number> = ref(0);";
        let result = analyze(code);
        let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
        assert_eq!(b.type_annotation.as_deref(), Some("Ref<number>"));
    }

    /// @ai-generated - Type annotation is None when not present
    #[test]
    fn type_annotation_none_when_absent() {
        let result = analyze("const count = 0;");
        let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
        assert!(b.type_annotation.is_none());
    }

    // ═══════════════════════════════════════════════════════════
    // Phase 1c: Exported function analysis tests
    // ═══════════════════════════════════════════════════════════

    fn analyze_with_scope(code: &str, scope: AnalysisScope) -> ScriptAnalysisSnapshot {
        let alloc = Allocator::new();
        build_script_analysis_with_scope(code, SourceType::ts(), &alloc, scope)
    }

    /// @ai-generated - Composable returning ref() directly detected
    #[test]
    fn composable_returning_ref_detected() {
        let code = r#"
import { ref } from 'vue';
export function useCounter() {
    return ref(0);
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        assert_eq!(f.name, "useCounter");
        assert_eq!(f.return_reactivity, ReturnReactivity::Ref);
        assert!(f.composable.is_some());
        let comp = f.composable.as_ref().unwrap();
        assert_eq!(comp.name, "useCounter");
    }

    /// @ai-generated - Composable returning reactive() directly detected
    #[test]
    fn composable_returning_reactive_detected() {
        let code = r#"
import { reactive } from 'vue';
export function useState() {
    return reactive({ x: 1 });
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        assert_eq!(
            result.exported_functions[0].return_reactivity,
            ReturnReactivity::Reactive
        );
    }

    /// @ai-generated - Composable returning identifier (indirect) is Unknown
    #[test]
    fn composable_returning_identifier_is_unknown() {
        let code = r#"
import { ref } from 'vue';
export function useCounter() {
    const count = ref(0);
    return count;
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        let f = &result.exported_functions[0];
        // Identifier returns can't be resolved by the heuristic body walk
        assert_eq!(f.return_reactivity, ReturnReactivity::Unknown);
        // But composable info still detects internal reactive state
        let comp = f.composable.as_ref().unwrap();
        assert!(!comp.internal_reactive_state.is_empty());
    }

    /// @ai-generated - Composable returning mixed object detected
    #[test]
    fn composable_returning_mixed_object_detected() {
        let code = r#"
import { ref, computed } from 'vue';
export function useCounter() {
    const count = ref(0);
    const doubled = computed(() => count.value * 2);
    return { count, doubled };
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        // The return has identifier refs, not direct calls, so it won't detect
        // individual field reactivity from identifiers alone (only from call exprs).
        // This is expected: the heuristic classifies `{ count, doubled }` as Plain
        // when the values are identifiers (not call expressions).
        assert!(matches!(
            f.return_reactivity,
            ReturnReactivity::Plain | ReturnReactivity::ObjectWithReactiveFields(_)
        ));
    }

    /// @ai-generated - Simple function returning literal is Plain
    #[test]
    fn simple_function_returning_literal_is_plain() {
        let code = r#"
export function getVersion() {
    return 42;
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        assert_eq!(
            result.exported_functions[0].return_reactivity,
            ReturnReactivity::Plain
        );
    }

    /// @ai-generated - Async function flagged
    #[test]
    fn async_function_flagged() {
        let code = r#"
export async function fetchData() {
    return await fetch('/api');
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        assert!(result.exported_functions[0].is_async);
    }

    /// @ai-generated - Only exported functions are analyzed
    #[test]
    fn only_exported_functions_analyzed() {
        let code = r#"
function internal() { return 1; }
export function exported() { return 2; }
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        assert_eq!(result.exported_functions[0].name, "exported");
    }

    /// @ai-generated - Analysis skipped when FUNC_RETURNS flag not set
    #[test]
    fn analysis_skipped_when_flag_not_set() {
        let code = r#"
export function useCounter() { return ref(0); }
"#;
        let result = analyze_with_scope(code, AnalysisScope::IMPORTS | AnalysisScope::BINDINGS);
        assert!(
            result.exported_functions.is_empty(),
            "exported_functions should be empty when FUNC_RETURNS not in scope"
        );
    }

    /// @ai-generated - Export default function analyzed
    #[test]
    fn export_default_function_analyzed() {
        let code = r#"
export default function useTheme() {
    return 'dark';
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        assert_eq!(f.name, "useTheme");
        assert!(f.is_default);
        assert_eq!(f.return_reactivity, ReturnReactivity::Plain);
    }

    /// @ai-generated - Export const arrow function analyzed
    #[test]
    fn export_const_arrow_function_analyzed() {
        let code = r#"
import { ref } from 'vue';
export const useCount = () => {
    return ref(0);
};
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        assert_eq!(f.name, "useCount");
        assert_eq!(f.return_reactivity, ReturnReactivity::Ref);
    }

    /// @ai-generated - Function with explicit TS return type annotation
    #[test]
    fn function_with_return_type_annotation() {
        let code = r#"
export function getRef(): Ref<number> {
    return ref(0);
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        assert_eq!(f.return_type_annotation.as_deref(), Some("Ref<number>"));
        assert_eq!(f.return_reactivity, ReturnReactivity::Ref);
    }

    /// @ai-generated - Function parameters with types extracted
    #[test]
    fn function_params_extracted() {
        let code = r#"
export function process(name: string, count?: number, active = true) {
    return name;
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        assert_eq!(result.exported_functions.len(), 1);
        let f = &result.exported_functions[0];
        assert_eq!(f.params.len(), 3);
        assert_eq!(f.params[0].name, "name");
        assert_eq!(f.params[0].type_annotation.as_deref(), Some("string"));
        assert!(!f.params[0].is_optional);
        assert_eq!(f.params[1].name, "count");
        assert!(f.params[1].is_optional);
        assert_eq!(f.params[2].name, "active");
        assert!(f.params[2].has_default);
    }

    /// @ai-generated - Composable with lifecycle hooks detected
    #[test]
    fn composable_with_lifecycle_hooks() {
        let code = r#"
import { ref, onMounted, onUnmounted, watch } from 'vue';
export function useTimer() {
    const elapsed = ref(0);
    onMounted(() => {});
    onUnmounted(() => {});
    watch(elapsed, () => {});
    return elapsed;
}
"#;
        let result = analyze_with_scope(code, AnalysisScope::all());
        let f = &result.exported_functions[0];
        let comp = f.composable.as_ref().unwrap();
        assert!(!comp.lifecycle_hooks.is_empty());
        assert!(comp.has_watchers);
    }
}
