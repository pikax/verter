use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::classify::{is_lifecycle_api, is_reactivity_api, is_watcher_api};
use crate::exports::extract_export_signatures_from_program;
use crate::imports::analyze_import_declaration;

use crate::macros::{
    collect_type_references, try_extract_macro_from_expr, try_extract_macro_from_var_decl,
};
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
                        let (initializer, is_reactive) = if let Some(ref init) = decl.init {
                            classify_initializer(init, &imports, &import_map)
                        } else {
                            (None, false)
                        };
                        bindings.push(AnalyzedBinding {
                            name: id.name.to_string(),
                            kind,
                            is_reactive,
                            initializer,
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
                        initializer: None,
                    });
                }
            }

            Statement::ClassDeclaration(cls) => {
                if let Some(ref id) = cls.id {
                    bindings.push(AnalyzedBinding {
                        name: id.name.to_string(),
                        kind: AnalyzedBindingKind::Class,
                        is_reactive: false,
                        initializer: None,
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

    ScriptAnalysisSnapshot {
        imports,
        bindings,
        macros,
        macro_type_deps,
        flags,
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

fn classify_initializer(
    expr: &Expression<'_>,
    imports: &[AnalyzedImport],
    import_map: &ImportBindingMap,
) -> (Option<BindingInitializer>, bool) {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(callee_name) = call_callee_name(&call.callee) {
                let callee_import_source = import_map
                    .source(imports, callee_name)
                    .map(|s| s.to_string());
                let vue_api = import_map.vue_api(callee_name);
                let is_reactive = vue_api.map(is_reactivity_api).unwrap_or(false);

                return (
                    Some(BindingInitializer::FunctionCall {
                        callee: callee_name.to_string(),
                        callee_import_source,
                        vue_api,
                    }),
                    is_reactive,
                );
            }
            (Some(BindingInitializer::Other), false)
        }
        Expression::Identifier(id) => (
            Some(BindingInitializer::Reference {
                name: id.name.to_string(),
            }),
            false,
        ),
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::String,
            }),
            false,
        ),
        Expression::NumericLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Number,
            }),
            false,
        ),
        Expression::BooleanLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Boolean,
            }),
            false,
        ),
        Expression::NullLiteral(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Null,
            }),
            false,
        ),
        Expression::ArrayExpression(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Array,
            }),
            false,
        ),
        Expression::ObjectExpression(_) => (
            Some(BindingInitializer::Literal {
                kind: LiteralKind::Object,
            }),
            false,
        ),
        _ => (Some(BindingInitializer::Other), false),
    }
}

fn call_callee_name<'a>(callee: &'a Expression<'a>) -> Option<&'a str> {
    match callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
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
}
