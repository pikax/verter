//! Binding type classification for Vue `<script setup>`.
//!
//! Extracts binding metadata from the OXC AST to determine the correct
//! accessor prefix/suffix for template expressions. This follows Vue's
//! official compiler classification (see `BindingTypes` in `@vue/compiler-core`).

use oxc_ast::ast::{
    BindingPattern, CallExpression, Expression, ImportDeclaration, ImportDeclarationSpecifier,
    PropertyKey, Statement, TSSignature, TSType, TSTypeParameterInstantiation, VariableDeclaration,
    VariableDeclarationKind,
};

use super::shared::ScriptParseContext;
use crate::common::Span;
use crate::syntax_kai::binding_types::BindingType;

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

    for stmt in &program.body {
        classify_statement(stmt, &mut entries, ctx);
    }

    entries
}

/// Classify a single top-level statement.
fn classify_statement<'a>(
    stmt: &Statement<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    match stmt {
        Statement::VariableDeclaration(decl) => {
            classify_variable_declaration(decl, entries, ctx);
        }
        Statement::ImportDeclaration(import) => {
            classify_import(import, entries, ctx);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            classify_expression_statement(&expr_stmt.expression, entries, ctx);
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                entries.push((ctx.adjust_span(id.span), BindingType::SetupConst));
            }
        }
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                entries.push((ctx.adjust_span(id.span), BindingType::SetupConst));
            }
        }
        Statement::TSEnumDeclaration(e) => {
            // Enums have a runtime JS representation
            entries.push((ctx.adjust_span(e.id.span), BindingType::SetupConst));
        }
        // TypeScript-only declarations — no runtime binding
        Statement::TSTypeAliasDeclaration(_) | Statement::TSInterfaceDeclaration(_) => {}
        _ => {}
    }
}

/// Classify variable declarations: const/let/var.
fn classify_variable_declaration<'a>(
    decl: &VariableDeclaration<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    let is_const = decl.kind == VariableDeclarationKind::Const;

    for declarator in &decl.declarations {
        // Handle destructuring from defineProps
        if is_const {
            if let Some(init) = &declarator.init {
                if is_define_props_call(init) {
                    extract_destructured_props(&declarator.id, entries, ctx);
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

        extract_pattern_bindings(&declarator.id, binding_type, entries, ctx);
    }
}

/// Classify the initializer of a `const` declaration.
fn classify_const_init<'a>(init: &Expression<'a>) -> BindingType {
    match init {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_) => BindingType::LiteralConst,

        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => BindingType::LiteralConst,

        Expression::CallExpression(call) => classify_call_expression(call),

        _ => BindingType::SetupConst,
    }
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
/// For a plain identifier (`const props = defineProps()`), the binding is `Props`.
fn extract_destructured_props<'a>(
    pattern: &BindingPattern<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    match pattern {
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                extract_pattern_bindings(&prop.value, BindingType::PropsAliased, entries, ctx);
            }
            if let Some(rest) = &obj.rest {
                extract_pattern_bindings(&rest.argument, BindingType::PropsAliased, entries, ctx);
            }
        }
        BindingPattern::BindingIdentifier(ident) => {
            entries.push((ctx.adjust_span(ident.span), BindingType::Props));
        }
        _ => {}
    }
}

/// Extract binding names from a pattern and classify them.
fn extract_pattern_bindings<'a>(
    pattern: &BindingPattern<'a>,
    binding_type: BindingType,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            entries.push((ctx.adjust_span(ident.span), binding_type));
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                extract_pattern_bindings(&prop.value, binding_type, entries, ctx);
            }
            if let Some(rest) = &obj.rest {
                extract_pattern_bindings(&rest.argument, binding_type, entries, ctx);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                extract_pattern_bindings(elem, binding_type, entries, ctx);
            }
            if let Some(rest) = &arr.rest {
                extract_pattern_bindings(&rest.argument, binding_type, entries, ctx);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            extract_pattern_bindings(&assign.left, binding_type, entries, ctx);
        }
    }
}

/// Classify standalone expression statements (e.g., `defineProps<{msg: string}>()`).
fn classify_expression_statement<'a>(
    expr: &Expression<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    if let Expression::CallExpression(call) = expr {
        let callee_name = get_callee_name(&call.callee);
        match callee_name.as_deref() {
            Some("defineProps") => {
                if let Some(type_args) = &call.type_arguments {
                    extract_props_from_type_params(type_args, entries, ctx);
                }
            }
            Some("withDefaults") => {
                if let Some(first_arg) = call.arguments.first() {
                    if let Some(Expression::CallExpression(inner_call)) = first_arg.as_expression()
                    {
                        if let Some(tp) = &inner_call.type_arguments {
                            extract_props_from_type_params(tp, entries, ctx);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract prop names from TypeScript type parameters of `defineProps`.
fn extract_props_from_type_params<'a>(
    type_params: &TSTypeParameterInstantiation<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
) {
    if let Some(TSType::TSTypeLiteral(literal)) = type_params.params.first() {
        for member in &literal.members {
            if let TSSignature::TSPropertySignature(prop) = member {
                if let PropertyKey::StaticIdentifier(ident) = &prop.key {
                    entries.push((ctx.adjust_span(ident.span), BindingType::Props));
                }
            }
        }
    }
}

/// Classify import declarations.
///
/// Type-only imports (`import type { ... }`) and per-specifier type imports
/// (`import { type Foo }`) are skipped — they have no runtime binding.
fn classify_import<'a>(
    import: &ImportDeclaration<'a>,
    entries: &mut Vec<(Span, BindingType)>,
    ctx: &ScriptParseContext<'a>,
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
                    entries.push((ctx.adjust_span(s.local.span), BindingType::SetupConst));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    entries.push((ctx.adjust_span(s.local.span), BindingType::SetupConst));
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    entries.push((ctx.adjust_span(s.local.span), BindingType::SetupConst));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::binding_types::BindingType;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    /// @ai-generated — Helper: parse source, extract bindings, return (name, BindingType) pairs.
    fn classify(source: &str) -> Vec<(String, BindingType)> {
        let alloc = Allocator::default();
        let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
        assert!(ret.errors.is_empty(), "Parse errors: {:?}", ret.errors);
        let ctx = ScriptParseContext::new(0, source.as_bytes());
        let entries = extract_bindings(&ret.program, &ctx);
        entries
            .into_iter()
            .map(|(span, bt)| {
                let name = &source[span.start as usize..span.end as usize];
                (name.to_string(), bt)
            })
            .collect()
    }

    /// @ai-generated — Helper: find binding type by name.
    fn find(bindings: &[(String, BindingType)], name: &str) -> Option<BindingType> {
        bindings.iter().find(|(n, _)| n == name).map(|(_, bt)| *bt)
    }

    // ── Variable declarations: literals ──────────────────────────────────

    /// @ai-generated
    #[test]
    fn const_string_literal() {
        let b = classify("const x = 'hello';");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_numeric_literal() {
        let b = classify("const x = 42;");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_boolean_literal() {
        let b = classify("const x = true;");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_null_literal() {
        let b = classify("const x = null;");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_bigint_literal() {
        let b = classify("const x = 123n;");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_static_template_literal() {
        let b = classify("const x = `static`;");
        assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
    }

    /// @ai-generated
    #[test]
    fn const_dynamic_template_literal() {
        let b = classify("const x = `${dynamic}`;");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
    }

    // ── Variable declarations: reactivity helpers ────────────────────────

    /// @ai-generated
    #[test]
    fn const_ref() {
        let b = classify("const x = ref(0);");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_computed() {
        let b = classify("const x = computed(() => 1);");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_shallow_ref() {
        let b = classify("const x = shallowRef({});");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_to_ref() {
        let b = classify("const x = toRef(props, 'a');");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_custom_ref() {
        let b = classify("const x = customRef((track, trigger) => ({}));");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_reactive() {
        let b = classify("const x = reactive({});");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupReactiveConst));
    }

    /// @ai-generated
    #[test]
    fn const_shallow_reactive() {
        let b = classify("const x = shallowReactive({});");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupReactiveConst));
    }

    /// @ai-generated
    #[test]
    fn const_use_composable() {
        let b = classify("const x = useFetch('/api');");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
    }

    /// @ai-generated
    #[test]
    fn const_use_router() {
        let b = classify("const x = useRouter();");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
    }

    /// @ai-generated
    #[test]
    fn const_other_call() {
        let b = classify("const x = someOtherCall();");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_member_call() {
        let b = classify("const x = obj.method();");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_no_init() {
        // const without init is valid TS (in some contexts like declare)
        // Our classifier returns SetupConst
        let b = classify("declare const x: string;");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
    }

    // ── let / var ────────────────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn let_declaration() {
        let b = classify("let x = 0;");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupLet));
    }

    /// @ai-generated
    #[test]
    fn var_declaration() {
        let b = classify("var x = 0;");
        assert_eq!(find(&b, "x"), Some(BindingType::SetupLet));
    }

    // ── Multiple declarators ─────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn multiple_const_declarators() {
        let b = classify("const a = 1, b = ref(0);");
        assert_eq!(find(&b, "a"), Some(BindingType::LiteralConst));
        assert_eq!(find(&b, "b"), Some(BindingType::SetupRef));
    }

    // ── Vue macros ───────────────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn const_define_model() {
        let b = classify("const model = defineModel();");
        assert_eq!(find(&b, "model"), Some(BindingType::SetupRef));
    }

    /// @ai-generated
    #[test]
    fn const_define_props_whole_object() {
        let b = classify("const props = defineProps({ msg: String });");
        assert_eq!(find(&b, "props"), Some(BindingType::Props));
    }

    /// @ai-generated
    #[test]
    fn const_define_props_destructured() {
        let b = classify("const { msg } = defineProps<{ msg: string }>();");
        assert_eq!(find(&b, "msg"), Some(BindingType::PropsAliased));
    }

    /// @ai-generated
    #[test]
    fn const_define_props_destructured_aliased() {
        let b = classify("const { msg: m } = defineProps<{ msg: string }>();");
        assert_eq!(find(&b, "m"), Some(BindingType::PropsAliased));
    }

    /// @ai-generated
    #[test]
    fn const_define_props_destructured_rest() {
        let b = classify("const { a, ...rest } = defineProps<{ a: string, b: number }>();");
        assert_eq!(find(&b, "a"), Some(BindingType::PropsAliased));
        assert_eq!(find(&b, "rest"), Some(BindingType::PropsAliased));
    }

    /// @ai-generated
    #[test]
    fn const_with_defaults_whole_object() {
        let b =
            classify("const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hi' });");
        assert_eq!(find(&b, "props"), Some(BindingType::Props));
    }

    /// @ai-generated
    #[test]
    fn const_define_emits() {
        let b = classify("const emit = defineEmits(['click']);");
        assert_eq!(find(&b, "emit"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_define_slots() {
        let b = classify("const slots = defineSlots();");
        assert_eq!(find(&b, "slots"), Some(BindingType::SetupConst));
    }

    // ── Standalone expression macros ─────────────────────────────────────

    /// @ai-generated
    #[test]
    fn standalone_define_props_typed() {
        let b = classify("defineProps<{ msg: string }>();");
        assert_eq!(find(&b, "msg"), Some(BindingType::Props));
    }

    /// @ai-generated
    #[test]
    fn standalone_define_props_multi_props() {
        let b = classify("defineProps<{ msg: string; count: number }>();");
        assert_eq!(find(&b, "msg"), Some(BindingType::Props));
        assert_eq!(find(&b, "count"), Some(BindingType::Props));
    }

    /// @ai-generated
    #[test]
    fn standalone_with_defaults_typed() {
        let b = classify("withDefaults(defineProps<{ msg: string }>(), { msg: 'hi' });");
        assert_eq!(find(&b, "msg"), Some(BindingType::Props));
    }

    // ── Function / class / enum declarations ─────────────────────────────

    /// @ai-generated
    #[test]
    fn function_declaration() {
        let b = classify("function foo() {}");
        assert_eq!(find(&b, "foo"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn async_function_declaration() {
        let b = classify("async function foo() {}");
        assert_eq!(find(&b, "foo"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn class_declaration() {
        let b = classify("class Foo {}");
        assert_eq!(find(&b, "Foo"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn enum_declaration() {
        let b = classify("enum Direction { Up, Down }");
        assert_eq!(find(&b, "Direction"), Some(BindingType::SetupConst));
    }

    // ── TypeScript-only declarations (NO binding) ────────────────────────

    /// @ai-generated
    #[test]
    fn type_alias_not_bound() {
        let b = classify("type Foo = string;");
        assert!(b.is_empty(), "type alias should produce no binding");
    }

    /// @ai-generated
    #[test]
    fn interface_not_bound() {
        let b = classify("interface Foo { x: number }");
        assert!(b.is_empty(), "interface should produce no binding");
    }

    /// @ai-generated
    #[test]
    fn import_type_not_bound() {
        let b = classify("import type { Foo } from 'bar';");
        assert!(b.is_empty(), "import type should produce no binding");
    }

    /// @ai-generated
    #[test]
    fn import_specifier_type_not_bound() {
        let b = classify("import { type Foo } from 'bar';");
        assert!(
            b.is_empty(),
            "per-specifier type import should produce no binding"
        );
    }

    /// @ai-generated
    #[test]
    fn import_mixed_type_and_value() {
        let b = classify("import { type Foo, bar } from 'baz';");
        assert_eq!(b.len(), 1, "only value import should produce a binding");
        assert_eq!(find(&b, "bar"), Some(BindingType::SetupConst));
        assert_eq!(
            find(&b, "Foo"),
            None,
            "type import should not produce binding"
        );
    }

    // ── Imports ──────────────────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn import_default() {
        let b = classify("import Foo from './Foo.vue';");
        assert_eq!(find(&b, "Foo"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn import_named() {
        let b = classify("import { ref } from 'vue';");
        assert_eq!(find(&b, "ref"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn import_namespace() {
        let b = classify("import * as utils from './utils';");
        assert_eq!(find(&b, "utils"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn import_multiple_named() {
        let b = classify("import { a, b } from 'mod';");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
    }

    // ── Destructuring ────────────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn const_object_destructure() {
        let b = classify("const { a, b } = someObj;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_array_destructure() {
        let b = classify("const [a, b] = someArr;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn let_object_destructure() {
        let b = classify("let { a, b } = someObj;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupLet));
        assert_eq!(find(&b, "b"), Some(BindingType::SetupLet));
    }

    /// @ai-generated
    #[test]
    fn const_destructure_with_default() {
        let b = classify("const { a = 1 } = someObj;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_nested_destructure() {
        let b = classify("const { a: { b } } = someObj;");
        assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
        // 'a' is not bound — it's a property key, not a binding
        assert_eq!(find(&b, "a"), None);
    }

    /// @ai-generated
    #[test]
    fn const_array_rest() {
        let b = classify("const [a, ...rest] = someArr;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "rest"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn const_object_rest() {
        let b = classify("const { a, ...rest } = someObj;");
        assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "rest"), Some(BindingType::SetupConst));
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    /// @ai-generated
    #[test]
    fn empty_script() {
        let b = classify("");
        assert!(b.is_empty());
    }

    /// @ai-generated
    #[test]
    fn mixed_declarations() {
        let b = classify(
            r#"
import { ref } from 'vue';
import type { Ref } from 'vue';
type MyType = string;
interface MyInterface {}
const count = ref(0);
const name = 'hello';
let mutable = 0;
function doSomething() {}
class MyClass {}
enum Color { Red, Green }
"#,
        );
        assert_eq!(find(&b, "ref"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "Ref"), None);
        assert_eq!(find(&b, "MyType"), None);
        assert_eq!(find(&b, "MyInterface"), None);
        assert_eq!(find(&b, "count"), Some(BindingType::SetupRef));
        assert_eq!(find(&b, "name"), Some(BindingType::LiteralConst));
        assert_eq!(find(&b, "mutable"), Some(BindingType::SetupLet));
        assert_eq!(find(&b, "doSomething"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "MyClass"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "Color"), Some(BindingType::SetupConst));
    }

    /// @ai-generated
    #[test]
    fn offset_adjustment() {
        let source = "const x = ref(0);";
        let alloc = Allocator::default();
        let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
        let ctx = ScriptParseContext::new(100, source.as_bytes());
        let entries = extract_bindings(&ret.program, &ctx);
        assert_eq!(entries.len(), 1);
        // Span should be offset by 100
        let (span, bt) = &entries[0];
        assert!(
            span.start >= 100,
            "span start {} should be >= 100",
            span.start
        );
        assert!(span.end > span.start);
        assert_eq!(*bt, BindingType::SetupRef);
    }

    /// @ai-generated — standalone defineEmits produces no binding (no type params to extract)
    #[test]
    fn standalone_define_emits_no_binding() {
        let b = classify("defineEmits(['click']);");
        assert!(
            b.is_empty(),
            "standalone defineEmits without assignment produces no binding"
        );
    }

    /// @ai-generated — standalone defineSlots produces no binding
    #[test]
    fn standalone_define_slots_no_binding() {
        let b = classify("defineSlots();");
        assert!(
            b.is_empty(),
            "standalone defineSlots without assignment produces no binding"
        );
    }

    /// @ai-generated — array destructure with holes
    #[test]
    fn array_destructure_with_holes() {
        let b = classify("const [, b, , d] = arr;");
        assert_eq!(b.len(), 2);
        assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
        assert_eq!(find(&b, "d"), Some(BindingType::SetupConst));
    }

    /// @ai-generated — const arrow function expression
    #[test]
    fn const_arrow_function() {
        let b = classify("const fn = () => {};");
        assert_eq!(find(&b, "fn"), Some(BindingType::SetupConst));
    }

    /// @ai-generated — const object expression
    #[test]
    fn const_object_expression() {
        let b = classify("const obj = { a: 1, b: 2 };");
        assert_eq!(find(&b, "obj"), Some(BindingType::SetupConst));
    }

    /// @ai-generated — const array expression
    #[test]
    fn const_array_expression() {
        let b = classify("const arr = [1, 2, 3];");
        assert_eq!(find(&b, "arr"), Some(BindingType::SetupConst));
    }

    /// @ai-generated — expression statement that is not a macro (should not produce binding)
    #[test]
    fn plain_expression_statement_no_binding() {
        let b = classify("console.log('hello');");
        assert!(b.is_empty());
    }

    /// @ai-generated — export function should not produce binding (export not handled)
    #[test]
    fn export_default_no_binding() {
        let b = classify("export default {};");
        assert!(b.is_empty());
    }
}
