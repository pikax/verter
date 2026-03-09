use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType};

use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;

use crate::classify::{classify_vue_api, is_lifecycle_api, is_reactivity_api, is_watcher_api};
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
    let mut module_references = Vec::new();
    let mut import_map = ImportBindingMap::new();
    let mut macros = Vec::new();
    let mut bindings = Vec::new();
    let mut vue_api_calls = Vec::new();
    let mut dom_query_calls = Vec::new();
    let mut css_var_manipulations = Vec::new();
    let mut first_await_offset: Option<u32> = None;
    let mut const_string_values: FxHashMap<String, Vec<String>> = FxHashMap::default();
    // Track local type → referenced type names (from extends and intersection)
    // e.g., `interface Local extends Base {}` → { "Local": ["Base"] }
    // e.g., `type Local = Base & { own: string }` → { "Local": ["Base"] }
    let mut local_type_deps: FxHashMap<String, Vec<String>> = FxHashMap::default();

    for stmt in &program.body {
        match stmt {
            Statement::ImportDeclaration(decl) => {
                let analyzed = analyze_import_declaration(decl);
                import_map.register(imports.len(), &analyzed);
                module_references.push(build_static_module_reference(
                    ModuleReferenceSyntax::StaticImport,
                    ModuleReferenceSemantics::Import,
                    decl.import_kind.is_type(),
                    decl.span.into(),
                    decl.source.span.into(),
                    content,
                    decl.source.value.as_str(),
                ));
                imports.push(analyzed);
            }

            Statement::ExportNamedDeclaration(decl) => {
                if let Some(source) = &decl.source {
                    module_references.push(build_static_module_reference(
                        ModuleReferenceSyntax::ExportFrom,
                        ModuleReferenceSemantics::Import,
                        decl.export_kind.is_type(),
                        decl.span.into(),
                        source.span.into(),
                        content,
                        source.value.as_str(),
                    ));
                }

                if let Some(decl) = &decl.declaration {
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

            Statement::ExportAllDeclaration(decl) => {
                module_references.push(build_static_module_reference(
                    ModuleReferenceSyntax::ExportFrom,
                    ModuleReferenceSemantics::Import,
                    false,
                    decl.span.into(),
                    decl.source.span.into(),
                    content,
                    decl.source.value.as_str(),
                ));
            }

            Statement::ExpressionStatement(expr_stmt) => {
                try_extract_macro_from_expr(&expr_stmt.expression, &mut macros);
                // Detect Vue API call sites (e.g., onMounted(cb), watch(src, cb))
                try_extract_vue_api_call(&expr_stmt.expression, &import_map, &mut vue_api_calls);
                // Detect DOM query calls (e.g., document.querySelector('.foo'))
                try_extract_dom_query(&expr_stmt.expression, &mut dom_query_calls);
                // Detect CSS variable manipulations (e.g., el.style.setProperty('--x', val))
                try_extract_css_var_manipulation(
                    &expr_stmt.expression,
                    content,
                    &mut css_var_manipulations,
                );
                if first_await_offset.is_none() {
                    if let Some(offset) = find_await_offset(&expr_stmt.expression) {
                        first_await_offset = Some(offset);
                    }
                }
                collect_module_references_from_expression(
                    &expr_stmt.expression,
                    content,
                    &const_string_values,
                    &mut module_references,
                );
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

                    if let BindingPattern::BindingIdentifier(id) = &decl.id {
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
                            span: id.span.into(),
                            used_in_script: false,
                            used_in_style: false,
                        });
                        if matches!(var_decl.kind, VariableDeclarationKind::Const) {
                            if let Some(init) = &decl.init {
                                if let Some(values) =
                                    evaluate_string_candidates(init, &const_string_values)
                                {
                                    const_string_values.insert(id.name.to_string(), values);
                                }
                            }
                        }
                    } else {
                        // Destructured binding: ObjectPattern, ArrayPattern, etc.
                        extract_destructured_bindings(
                            &decl.id,
                            kind,
                            is_reactive,
                            reactivity_kind,
                            &initializer,
                            &mut bindings,
                        );
                    }

                    // Extract Vue API calls, DOM queries, and CSS manipulations from initializer
                    if let Some(ref init) = decl.init {
                        try_extract_vue_api_call(init, &import_map, &mut vue_api_calls);
                        try_extract_dom_query(init, &mut dom_query_calls);
                        try_extract_css_var_manipulation(init, content, &mut css_var_manipulations);
                        collect_module_references_from_expression(
                            init,
                            content,
                            &const_string_values,
                            &mut module_references,
                        );
                    }

                    if first_await_offset.is_none() {
                        if let Some(ref init) = decl.init {
                            if let Some(offset) = find_await_offset(init) {
                                first_await_offset = Some(offset);
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
                        span: id.span.into(),
                        used_in_script: false,
                        used_in_style: false,
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
                        span: id.span.into(),
                        used_in_script: false,
                        used_in_style: false,
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
            Statement::ForOfStatement(for_of) if for_of.r#await => {
                if first_await_offset.is_none() {
                    first_await_offset = Some(for_of.span.start);
                }
            }

            _ => {}
        }
    }

    // ── Derive: macro type deps ──
    let macro_type_deps = derive_macro_type_deps(&macros, &imports, &import_map, &local_type_deps);

    // ── Derive: flags ──
    let mut flags = derive_flags(&imports, &macros, &bindings, &macro_type_deps);
    if first_await_offset.is_some() {
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
        module_references,
        bindings,
        macros,
        macro_type_deps,
        vue_api_calls,
        dom_query_calls,
        css_var_manipulations,
        first_await_offset,
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

fn build_static_module_reference(
    syntax: ModuleReferenceSyntax,
    semantics: ModuleReferenceSemantics,
    is_type_only: bool,
    span: Span,
    expr_span: Span,
    content: &str,
    specifier: &str,
) -> AnalyzedModuleReference {
    let raw_text = expr_span.slice(content).to_string();
    AnalyzedModuleReference {
        syntax,
        semantics,
        is_type_only,
        span,
        expr_span,
        raw_text,
        literal_specifier: Some(specifier.to_string()),
        finite_specifiers: Vec::new(),
        static_prefix: Some(specifier.to_string()),
        analyzability: ModuleReferenceAnalyzability::Exact,
    }
}

fn build_dynamic_module_reference(
    syntax: ModuleReferenceSyntax,
    semantics: ModuleReferenceSemantics,
    span: Span,
    expr_span: Span,
    expr: &Expression<'_>,
    content: &str,
    const_string_values: &FxHashMap<String, Vec<String>>,
) -> AnalyzedModuleReference {
    let raw_text = expr_span.slice(content).to_string();
    let candidates = evaluate_string_candidates(expr, const_string_values);
    let static_prefix =
        static_prefix_from_expression(expr, const_string_values, candidates.as_ref());

    match candidates {
        Some(mut values) if values.len() == 1 => AnalyzedModuleReference {
            syntax,
            semantics,
            is_type_only: false,
            span,
            expr_span,
            raw_text,
            literal_specifier: Some(values.remove(0)),
            finite_specifiers: Vec::new(),
            static_prefix,
            analyzability: ModuleReferenceAnalyzability::Exact,
        },
        Some(values) if !values.is_empty() => AnalyzedModuleReference {
            syntax,
            semantics,
            is_type_only: false,
            span,
            expr_span,
            raw_text,
            literal_specifier: None,
            finite_specifiers: values,
            static_prefix,
            analyzability: ModuleReferenceAnalyzability::FiniteSet,
        },
        _ => AnalyzedModuleReference {
            syntax,
            semantics,
            is_type_only: false,
            span,
            expr_span,
            raw_text,
            literal_specifier: None,
            finite_specifiers: Vec::new(),
            static_prefix,
            analyzability: ModuleReferenceAnalyzability::UnknownDynamic,
        },
    }
}

fn collect_module_references_from_expression(
    expr: &Expression<'_>,
    content: &str,
    const_string_values: &FxHashMap<String, Vec<String>>,
    module_references: &mut Vec<AnalyzedModuleReference>,
) {
    match expr {
        Expression::ImportExpression(import) => {
            module_references.push(build_dynamic_module_reference(
                ModuleReferenceSyntax::DynamicImport,
                ModuleReferenceSemantics::Import,
                import.span.into(),
                import.source.span().into(),
                &import.source,
                content,
                const_string_values,
            ));
            collect_module_references_from_expression(
                &import.source,
                content,
                const_string_values,
                module_references,
            );
        }
        Expression::CallExpression(call) => {
            if let Expression::Identifier(id) = &call.callee {
                if id.name.as_str() == "require" {
                    if let Some(source) = call.arguments.first().and_then(|arg| arg.as_expression())
                    {
                        module_references.push(build_dynamic_module_reference(
                            ModuleReferenceSyntax::RequireCall,
                            ModuleReferenceSemantics::Require,
                            call.span.into(),
                            source.span().into(),
                            source,
                            content,
                            const_string_values,
                        ));
                    }
                }
            }

            collect_module_references_from_expression(
                &call.callee,
                content,
                const_string_values,
                module_references,
            );
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_module_references_from_expression(
                        arg_expr,
                        content,
                        const_string_values,
                        module_references,
                    );
                }
            }
        }
        Expression::AwaitExpression(aw) => collect_module_references_from_expression(
            &aw.argument,
            content,
            const_string_values,
            module_references,
        ),
        Expression::ParenthesizedExpression(paren) => collect_module_references_from_expression(
            &paren.expression,
            content,
            const_string_values,
            module_references,
        ),
        Expression::TSAsExpression(expr) => collect_module_references_from_expression(
            &expr.expression,
            content,
            const_string_values,
            module_references,
        ),
        Expression::TSSatisfiesExpression(expr) => collect_module_references_from_expression(
            &expr.expression,
            content,
            const_string_values,
            module_references,
        ),
        Expression::TSTypeAssertion(expr) => collect_module_references_from_expression(
            &expr.expression,
            content,
            const_string_values,
            module_references,
        ),
        Expression::BinaryExpression(bin) => {
            collect_module_references_from_expression(
                &bin.left,
                content,
                const_string_values,
                module_references,
            );
            collect_module_references_from_expression(
                &bin.right,
                content,
                const_string_values,
                module_references,
            );
        }
        Expression::LogicalExpression(log) => {
            collect_module_references_from_expression(
                &log.left,
                content,
                const_string_values,
                module_references,
            );
            collect_module_references_from_expression(
                &log.right,
                content,
                const_string_values,
                module_references,
            );
        }
        Expression::ConditionalExpression(cond) => {
            collect_module_references_from_expression(
                &cond.test,
                content,
                const_string_values,
                module_references,
            );
            collect_module_references_from_expression(
                &cond.consequent,
                content,
                const_string_values,
                module_references,
            );
            collect_module_references_from_expression(
                &cond.alternate,
                content,
                const_string_values,
                module_references,
            );
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let Some(elem_expr) = elem.as_expression() {
                    collect_module_references_from_expression(
                        elem_expr,
                        content,
                        const_string_values,
                        module_references,
                    );
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    ObjectPropertyKind::ObjectProperty(prop) => {
                        collect_module_references_from_expression(
                            &prop.value,
                            content,
                            const_string_values,
                            module_references,
                        );
                    }
                    ObjectPropertyKind::SpreadProperty(prop) => {
                        collect_module_references_from_expression(
                            &prop.argument,
                            content,
                            const_string_values,
                            module_references,
                        );
                    }
                }
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                collect_module_references_from_expression(
                    expr,
                    content,
                    const_string_values,
                    module_references,
                );
            }
        }
        Expression::TaggedTemplateExpression(tagged) => {
            collect_module_references_from_expression(
                &tagged.tag,
                content,
                const_string_values,
                module_references,
            );
            for expr in &tagged.quasi.expressions {
                collect_module_references_from_expression(
                    expr,
                    content,
                    const_string_values,
                    module_references,
                );
            }
        }
        Expression::StaticMemberExpression(member) => collect_module_references_from_expression(
            &member.object,
            content,
            const_string_values,
            module_references,
        ),
        Expression::ComputedMemberExpression(member) => {
            collect_module_references_from_expression(
                &member.object,
                content,
                const_string_values,
                module_references,
            );
            collect_module_references_from_expression(
                &member.expression,
                content,
                const_string_values,
                module_references,
            );
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => {
                collect_module_references_from_expression(
                    &call.callee,
                    content,
                    const_string_values,
                    module_references,
                );
                for arg in &call.arguments {
                    if let Some(arg_expr) = arg.as_expression() {
                        collect_module_references_from_expression(
                            arg_expr,
                            content,
                            const_string_values,
                            module_references,
                        );
                    }
                }
            }
            ChainElement::ComputedMemberExpression(member) => {
                collect_module_references_from_expression(
                    &member.object,
                    content,
                    const_string_values,
                    module_references,
                );
                collect_module_references_from_expression(
                    &member.expression,
                    content,
                    const_string_values,
                    module_references,
                );
            }
            ChainElement::StaticMemberExpression(member) => {
                collect_module_references_from_expression(
                    &member.object,
                    content,
                    const_string_values,
                    module_references,
                )
            }
            ChainElement::TSNonNullExpression(non_null) => {
                collect_module_references_from_expression(
                    &non_null.expression,
                    content,
                    const_string_values,
                    module_references,
                )
            }
            _ => {}
        },
        _ => {}
    }
}

fn evaluate_string_candidates(
    expr: &Expression<'_>,
    const_string_values: &FxHashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    match expr {
        Expression::StringLiteral(lit) => Some(vec![lit.value.to_string()]),
        Expression::TemplateLiteral(tpl) => {
            if tpl.quasis.is_empty() {
                return Some(Vec::new());
            }

            let mut values = vec![String::new()];
            for (idx, quasi) in tpl.quasis.iter().enumerate() {
                for value in &mut values {
                    value.push_str(quasi.value.raw.as_str());
                }
                if let Some(expr) = tpl.expressions.get(idx) {
                    let expr_values = evaluate_string_candidates(expr, const_string_values)?;
                    values = combine_string_candidates(&values, &expr_values)?;
                }
            }
            dedupe_string_candidates(values)
        }
        Expression::BinaryExpression(bin) => {
            if bin.operator.as_str() != "+" {
                return None;
            }
            let left = evaluate_string_candidates(&bin.left, const_string_values)?;
            let right = evaluate_string_candidates(&bin.right, const_string_values)?;
            dedupe_string_candidates(combine_string_candidates(&left, &right)?)
        }
        Expression::ConditionalExpression(cond) => {
            let mut values = evaluate_string_candidates(&cond.consequent, const_string_values)?;
            values.extend(evaluate_string_candidates(
                &cond.alternate,
                const_string_values,
            )?);
            dedupe_string_candidates(values)
        }
        Expression::Identifier(id) => const_string_values.get(id.name.as_str()).cloned(),
        Expression::ParenthesizedExpression(paren) => {
            evaluate_string_candidates(&paren.expression, const_string_values)
        }
        Expression::TSAsExpression(expr) => {
            evaluate_string_candidates(&expr.expression, const_string_values)
        }
        Expression::TSSatisfiesExpression(expr) => {
            evaluate_string_candidates(&expr.expression, const_string_values)
        }
        Expression::TSTypeAssertion(expr) => {
            evaluate_string_candidates(&expr.expression, const_string_values)
        }
        Expression::SequenceExpression(seq) => seq
            .expressions
            .last()
            .and_then(|expr| evaluate_string_candidates(expr, const_string_values)),
        _ => None,
    }
}

fn combine_string_candidates(left: &[String], right: &[String]) -> Option<Vec<String>> {
    const MAX_CANDIDATES: usize = 16;
    let mut out = Vec::with_capacity(left.len().saturating_mul(right.len()));
    for lhs in left {
        for rhs in right {
            if out.len() >= MAX_CANDIDATES {
                return None;
            }
            out.push(format!("{lhs}{rhs}"));
        }
    }
    Some(out)
}

fn dedupe_string_candidates(values: Vec<String>) -> Option<Vec<String>> {
    let mut seen = FxHashSet::default();
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        if seen.insert(value.clone()) {
            out.push(value);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn static_prefix_from_expression(
    expr: &Expression<'_>,
    const_string_values: &FxHashMap<String, Vec<String>>,
    candidates: Option<&Vec<String>>,
) -> Option<String> {
    if let Some(values) = candidates {
        if !values.is_empty() {
            let prefix = common_prefix(values);
            if !prefix.is_empty() {
                return Some(prefix);
            }
        }
    }

    match expr {
        Expression::TemplateLiteral(tpl) => tpl
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.as_str().to_string())
            .filter(|prefix| !prefix.is_empty()),
        Expression::BinaryExpression(bin) if bin.operator.as_str() == "+" => {
            static_prefix_from_expression(&bin.left, const_string_values, None)
        }
        Expression::Identifier(id) => {
            const_string_values
                .get(id.name.as_str())
                .and_then(|values| {
                    let prefix = common_prefix(values);
                    if prefix.is_empty() {
                        None
                    } else {
                        Some(prefix)
                    }
                })
        }
        Expression::ParenthesizedExpression(paren) => {
            static_prefix_from_expression(&paren.expression, const_string_values, None)
        }
        Expression::TSAsExpression(expr) => {
            static_prefix_from_expression(&expr.expression, const_string_values, None)
        }
        Expression::TSSatisfiesExpression(expr) => {
            static_prefix_from_expression(&expr.expression, const_string_values, None)
        }
        Expression::TSTypeAssertion(expr) => {
            static_prefix_from_expression(&expr.expression, const_string_values, None)
        }
        _ => None,
    }
}

fn common_prefix(values: &[String]) -> String {
    let Some(first) = values.first() else {
        return String::new();
    };
    let mut prefix = first.clone();
    for value in &values[1..] {
        while !value.starts_with(&prefix) {
            if prefix.is_empty() {
                break;
            }
            prefix.pop();
        }
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

/// Classify an initializer expression.
/// Returns (initializer, is_reactive, reactivity_kind).
/// Recursively extract all binding identifiers from a destructuring pattern.
///
/// Walks `ObjectPattern`, `ArrayPattern`, and `AssignmentPattern` nodes
/// to collect each leaf `BindingIdentifier`. Each is emitted as an
/// `AnalyzedBinding` with the given kind, reactivity, and initializer info.
fn extract_destructured_bindings(
    pattern: &BindingPattern<'_>,
    kind: AnalyzedBindingKind,
    is_reactive: bool,
    reactivity_kind: ReactivityKind,
    initializer: &Option<BindingInitializer>,
    bindings: &mut Vec<AnalyzedBinding>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            bindings.push(AnalyzedBinding {
                name: id.name.to_string(),
                kind,
                is_reactive,
                reactivity_kind,
                type_annotation: None,
                initializer: initializer.clone(),
                span: id.span.into(),
                used_in_script: false,
                used_in_style: false,
            });
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                extract_destructured_bindings(
                    &prop.value,
                    kind,
                    is_reactive,
                    reactivity_kind,
                    initializer,
                    bindings,
                );
            }
            if let Some(rest) = &obj.rest {
                extract_destructured_bindings(
                    &rest.argument,
                    kind,
                    is_reactive,
                    reactivity_kind,
                    initializer,
                    bindings,
                );
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                extract_destructured_bindings(
                    elem,
                    kind,
                    is_reactive,
                    reactivity_kind,
                    initializer,
                    bindings,
                );
            }
            if let Some(rest) = &arr.rest {
                extract_destructured_bindings(
                    &rest.argument,
                    kind,
                    is_reactive,
                    reactivity_kind,
                    initializer,
                    bindings,
                );
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            // `a = default` — extract the left-hand binding
            extract_destructured_bindings(
                &assign.left,
                kind,
                is_reactive,
                reactivity_kind,
                initializer,
                bindings,
            );
        }
    }
}

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
                let vue_api = import_map.vue_api(callee_name).or_else(|| {
                    // Compiler macros (defineModel, defineProps, etc.) are not imported
                    // but still have a Vue API classification. Fall back to name-based
                    // classification for unimported callees.
                    let api = classify_vue_api(callee_name);
                    if api != VueApiClassification::Other {
                        Some(api)
                    } else {
                        None
                    }
                });
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
        Some(VueApiClassification::ToRefs) => ReactivityKind::Ref,
        Some(VueApiClassification::Readonly | VueApiClassification::ShallowReadonly) => {
            ReactivityKind::Reactive
        }
        Some(VueApiClassification::DefineModel) => ReactivityKind::Ref,
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

/// Extract Vue API call site from an expression (e.g., `onMounted(cb)`, `provide(key, val)`).
/// Only records calls where the callee is a known Vue API import.
fn try_extract_vue_api_call(
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
    vue_api_calls: &mut Vec<VueApiCallSite>,
) {
    if let Expression::CallExpression(call) = expr {
        if let Some(callee_name) = call_callee_name(&call.callee) {
            if let Some(api) = import_map.vue_api(callee_name) {
                // Extract first string argument for useTemplateRef, provide, inject
                let arg_value = if matches!(
                    api,
                    VueApiClassification::UseTemplateRef
                        | VueApiClassification::Provide
                        | VueApiClassification::Inject
                ) {
                    first_string_arg(call)
                } else {
                    None
                };
                let is_async_callback = first_arg_is_async(call);
                let callback_params = extract_callback_params(call, api);
                vue_api_calls.push(VueApiCallSite {
                    api,
                    span: call.span.into(),
                    arg_value,
                    is_async_callback,
                    callback_params,
                });
            }
        }
    }
}

/// Extract callback parameter names and spans from a Vue API call.
///
/// For watchers: `watch(source, (val, old) => ...)` → extracts `val`, `old` from 2nd arg
/// For lifecycle/watchEffect: `onMounted(() => ...)` → extracts from 1st arg (if any params)
fn extract_callback_params(
    call: &oxc_ast::ast::CallExpression<'_>,
    api: VueApiClassification,
) -> Vec<VueApiCallbackParam> {
    use crate::types::VueApiCallbackParam;

    // Determine which argument index has the callback
    let cb_index = match api {
        VueApiClassification::Watch | VueApiClassification::WatchSyncEffect => 1,
        _ if api.is_lifecycle()
            || api == VueApiClassification::WatchEffect
            || api == VueApiClassification::WatchPostEffect =>
        {
            0
        }
        _ => return vec![],
    };

    let arg = match call.arguments.get(cb_index) {
        Some(a) => a,
        None => return vec![],
    };

    let expr = match arg.as_expression() {
        Some(e) => e,
        None => return vec![],
    };

    // Extract params from arrow or function expression
    let params = match expr {
        Expression::ArrowFunctionExpression(arrow) => &arrow.params,
        Expression::FunctionExpression(func) => &func.params,
        _ => return vec![],
    };

    params
        .items
        .iter()
        .filter_map(|param| {
            // Only extract simple identifier params (not destructured)
            if let BindingPattern::BindingIdentifier(id) = &param.pattern {
                // Skip if already has a type annotation
                if param.type_annotation.is_some() {
                    return None;
                }
                Some(VueApiCallbackParam {
                    name: id.name.to_string(),
                    span: id.span.into(),
                    inferred_type: None, // Will be populated later
                })
            } else {
                None
            }
        })
        .collect()
}

/// Extract the first string literal argument from a call expression.
fn first_string_arg(call: &oxc_ast::ast::CallExpression<'_>) -> Option<String> {
    let arg = call.arguments.first()?;
    match arg.as_expression()? {
        Expression::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

/// Check whether the first argument to a call is an async function or async arrow.
/// Used to detect `computed(async () => ...)` patterns.
fn first_arg_is_async(call: &oxc_ast::ast::CallExpression<'_>) -> bool {
    let Some(first) = call.arguments.first() else {
        return false;
    };
    let Some(expr) = first.as_expression() else {
        return false;
    };
    match expr {
        Expression::ArrowFunctionExpression(f) => f.r#async,
        Expression::FunctionExpression(f) => f.r#async,
        _ => false,
    }
}

/// Extract a DOM query call site from an expression.
/// Detects patterns like `document.querySelector('.foo')`, `el.value?.querySelector('.bar')`,
/// `document.getElementById('app')`, `document.getElementsByClassName('btn')`.
fn try_extract_dom_query(expr: &Expression<'_>, dom_query_calls: &mut Vec<DomQueryCallSite>) {
    let call = match expr {
        Expression::CallExpression(c) => c,
        _ => return,
    };

    // Match member expressions: something.querySelector(...)
    let method_name = match &call.callee {
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        Expression::ChainExpression(chain) => {
            if let oxc_ast::ast::ChainElement::StaticMemberExpression(member) = &chain.expression {
                member.property.name.as_str()
            } else {
                return;
            }
        }
        _ => return,
    };

    let kind = match method_name {
        "querySelector" => DomQueryKind::QuerySelector,
        "querySelectorAll" => DomQueryKind::QuerySelectorAll,
        "getElementById" => DomQueryKind::GetElementById,
        "getElementsByClassName" => DomQueryKind::GetElementsByClassName,
        _ => return,
    };

    // Extract the string argument
    let arg = match call.arguments.first() {
        Some(a) => a,
        None => return,
    };
    let (selector_text, arg_span) = match arg.as_expression() {
        Some(Expression::StringLiteral(s)) => {
            (s.value.to_string(), verter_span::Span::from(s.span))
        }
        _ => return,
    };

    // Parse the selector for querySelector/querySelectorAll
    let parsed = match kind {
        DomQueryKind::QuerySelector | DomQueryKind::QuerySelectorAll => {
            crate::style::parse_selector(&selector_text)
        }
        DomQueryKind::GetElementById => {
            // Synthesize a selector: #id
            crate::style::parse_selector(&format!("#{selector_text}"))
        }
        DomQueryKind::GetElementsByClassName => {
            // Synthesize a selector: .class
            crate::style::parse_selector(&format!(".{selector_text}"))
        }
    };

    dom_query_calls.push(DomQueryCallSite {
        kind,
        selector_text,
        parsed,
        span: call.span.into(),
        arg_span,
    });
}

/// Extract a CSS variable manipulation from an expression.
/// Detects patterns like `el.style.setProperty('--x', val)`,
/// `getComputedStyle(el).getPropertyValue('--x')`,
/// `el.style.removeProperty('--x')`.
fn try_extract_css_var_manipulation(
    expr: &Expression<'_>,
    source: &str,
    css_var_manipulations: &mut Vec<CssVarManipulation>,
) {
    let call = match expr {
        Expression::CallExpression(c) => c,
        _ => return,
    };

    // Match member expressions: something.setProperty(...), something.getPropertyValue(...), something.removeProperty(...)
    let method_name = match &call.callee {
        Expression::StaticMemberExpression(member) => member.property.name.as_str(),
        Expression::ChainExpression(chain) => {
            if let oxc_ast::ast::ChainElement::StaticMemberExpression(member) = &chain.expression {
                member.property.name.as_str()
            } else {
                return;
            }
        }
        _ => return,
    };

    let kind = match method_name {
        "setProperty" => CssVarManipulationKind::SetProperty,
        "getPropertyValue" => CssVarManipulationKind::GetPropertyValue,
        "removeProperty" => CssVarManipulationKind::RemoveProperty,
        _ => return,
    };

    // First argument must be a string literal starting with "--"
    let arg = match call.arguments.first() {
        Some(a) => a,
        None => return,
    };
    let var_name = match arg.as_expression() {
        Some(Expression::StringLiteral(s)) => {
            let val = s.value.as_str();
            if !val.starts_with("--") {
                return;
            }
            val.to_string()
        }
        _ => return,
    };

    // For setProperty, extract the value expression source text
    let value_expr = if kind == CssVarManipulationKind::SetProperty {
        call.arguments.get(1).and_then(|a| {
            let start = a.span().start as usize;
            let end = a.span().end as usize;
            if start < source.len() && end <= source.len() {
                Some(source[start..end].to_string())
            } else {
                None
            }
        })
    } else {
        None
    };

    css_var_manipulations.push(CssVarManipulation {
        kind,
        var_name,
        value_expr,
        span: call.span.into(),
    });
}
/// Stops at function boundaries (arrow/function expressions don't make setup async).
fn find_await_offset(expr: &Expression<'_>) -> Option<u32> {
    match expr {
        Expression::AwaitExpression(aw) => Some(aw.span.start),
        // Stop at function boundaries
        Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => None,
        Expression::CallExpression(call) => {
            if let Some(offset) = find_await_offset(&call.callee) {
                return Some(offset);
            }
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    if let Some(offset) = find_await_offset(e) {
                        return Some(offset);
                    }
                }
            }
            None
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                match elem {
                    oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) => {
                        if let Some(offset) = find_await_offset(&s.argument) {
                            return Some(offset);
                        }
                    }
                    oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                    _ => {
                        if let Some(e) = elem.as_expression() {
                            if let Some(offset) = find_await_offset(e) {
                                return Some(offset);
                            }
                        }
                    }
                }
            }
            None
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                        if let Some(offset) = find_await_offset(&p.value) {
                            return Some(offset);
                        }
                    }
                    oxc_ast::ast::ObjectPropertyKind::SpreadProperty(s) => {
                        if let Some(offset) = find_await_offset(&s.argument) {
                            return Some(offset);
                        }
                    }
                }
            }
            None
        }
        Expression::AssignmentExpression(assign) => find_await_offset(&assign.right),
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                if let Some(offset) = find_await_offset(expr) {
                    return Some(offset);
                }
            }
            None
        }
        Expression::ConditionalExpression(cond) => find_await_offset(&cond.test)
            .or_else(|| find_await_offset(&cond.consequent))
            .or_else(|| find_await_offset(&cond.alternate)),
        Expression::BinaryExpression(bin) => {
            find_await_offset(&bin.left).or_else(|| find_await_offset(&bin.right))
        }
        Expression::LogicalExpression(log) => {
            find_await_offset(&log.left).or_else(|| find_await_offset(&log.right))
        }
        Expression::UnaryExpression(un) => find_await_offset(&un.argument),
        Expression::TemplateLiteral(tpl) => {
            for expr in &tpl.expressions {
                if let Some(offset) = find_await_offset(expr) {
                    return Some(offset);
                }
            }
            None
        }
        Expression::TaggedTemplateExpression(tagged) => {
            find_await_offset(&tagged.tag).or_else(|| {
                tagged
                    .quasi
                    .expressions
                    .iter()
                    .find_map(|e| find_await_offset(e))
            })
        }
        Expression::ComputedMemberExpression(m) => {
            find_await_offset(&m.object).or_else(|| find_await_offset(&m.expression))
        }
        Expression::StaticMemberExpression(m) => find_await_offset(&m.object),
        Expression::ParenthesizedExpression(p) => find_await_offset(&p.expression),
        Expression::YieldExpression(y) => y.argument.as_ref().and_then(|a| find_await_offset(a)),
        Expression::TSNonNullExpression(e) => find_await_offset(&e.expression),
        Expression::TSAsExpression(e) => find_await_offset(&e.expression),
        Expression::TSSatisfiesExpression(e) => find_await_offset(&e.expression),
        Expression::TSTypeAssertion(e) => find_await_offset(&e.expression),
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
            classify_single_return_expr(&expr_stmt.expression, import_map, &[])
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

        let span = match &param.pattern {
            BindingPattern::BindingIdentifier(id) => id.span.into(),
            _ => verter_span::Span::new(param.span.start, param.span.end),
        };
        out.push(FunctionParam {
            name,
            type_annotation,
            is_optional,
            has_default,
            span,
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
    collect_return_expressions(body, import_map, &[], &mut return_kinds);

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
    local_bindings: &[(String, ReactivityKind)],
    out: &mut Vec<ReturnReactivity>,
) {
    for stmt in &body.statements {
        collect_returns_from_stmt(stmt, import_map, local_bindings, out);
    }
}

/// Recursively walk statements for return expressions, stopping at function boundaries.
fn collect_returns_from_stmt(
    stmt: &Statement<'_>,
    import_map: &ImportBindingMap,
    local_bindings: &[(String, ReactivityKind)],
    out: &mut Vec<ReturnReactivity>,
) {
    match stmt {
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                out.push(classify_single_return_expr(arg, import_map, local_bindings));
            } else {
                out.push(ReturnReactivity::Plain);
            }
        }
        // Recurse into blocks
        Statement::BlockStatement(block) => {
            for s in &block.body {
                collect_returns_from_stmt(s, import_map, local_bindings, out);
            }
        }
        Statement::IfStatement(if_stmt) => {
            collect_returns_from_stmt(&if_stmt.consequent, import_map, local_bindings, out);
            if let Some(alt) = &if_stmt.alternate {
                collect_returns_from_stmt(alt, import_map, local_bindings, out);
            }
        }
        Statement::TryStatement(try_stmt) => {
            for s in &try_stmt.block.body {
                collect_returns_from_stmt(s, import_map, local_bindings, out);
            }
            if let Some(catch) = &try_stmt.handler {
                for s in &catch.body.body {
                    collect_returns_from_stmt(s, import_map, local_bindings, out);
                }
            }
            if let Some(fin) = &try_stmt.finalizer {
                for s in &fin.body {
                    collect_returns_from_stmt(s, import_map, local_bindings, out);
                }
            }
        }
        Statement::SwitchStatement(switch) => {
            for case in &switch.cases {
                for s in &case.consequent {
                    collect_returns_from_stmt(s, import_map, local_bindings, out);
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
    local_bindings: &[(String, ReactivityKind)],
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
                        let kind = classify_value_reactivity(&p.value, import_map, local_bindings);
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
            classify_single_return_expr(&p.expression, import_map, local_bindings)
        }
        // TS assertions → unwrap
        Expression::TSAsExpression(e) => {
            classify_single_return_expr(&e.expression, import_map, local_bindings)
        }
        Expression::TSSatisfiesExpression(e) => {
            classify_single_return_expr(&e.expression, import_map, local_bindings)
        }
        _ => ReturnReactivity::Unknown,
    }
}

/// Classify the reactivity of a value expression (for object return fields).
/// Resolves identifier references to local bindings when possible.
fn classify_value_reactivity(
    expr: &Expression<'_>,
    import_map: &ImportBindingMap,
    local_bindings: &[(String, ReactivityKind)],
) -> ReactivityKind {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(callee_name) = call_callee_name(&call.callee) {
                let vue_api = import_map.vue_api(callee_name);
                return classify_reactivity_kind(vue_api, callee_name);
            }
            ReactivityKind::None
        }
        Expression::Identifier(id) => {
            // Resolve identifier to a local binding's reactivity
            local_bindings
                .iter()
                .find(|(name, _)| name == id.name.as_str())
                .map(|(_, kind)| *kind)
                .unwrap_or(ReactivityKind::None)
        }
        Expression::ParenthesizedExpression(p) => {
            classify_value_reactivity(&p.expression, import_map, local_bindings)
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
        detect_composable_return_shape(body, import_map, &internal_reactive_state)
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
    local_bindings: &[(String, ReactivityKind)],
) -> ComposableReturn {
    // Find the last return statement's expression
    let mut returns = Vec::new();
    collect_return_expressions(body, import_map, local_bindings, &mut returns);

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
            AnalyzedMacroKind::DefineOptions => {
                flags |= AnalysisFlags::HAS_DEFINE_OPTIONS;
                if m.has_inherit_attrs_false {
                    flags |= AnalysisFlags::HAS_INHERIT_ATTRS_FALSE;
                }
            }
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
#[path = "analysis_tests.rs"]
mod analysis_tests;
