use super::*;
use crate::oxc_parse::Parser;
use crate::types::BindingType;
use crate::utils::oxc::vue::script::shared::try_process_import;
use crate::utils::oxc::vue::script::ScriptItem;
use oxc_allocator::Allocator;
use oxc_span::SourceType;

/// @ai-generated — Helper: parse source, extract bindings, return (name, BindingType) pairs.
fn classify(source: &str) -> Vec<(String, BindingType)> {
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
    assert!(
        ret.diagnostics.is_empty(),
        "Parse errors: {:?}",
        ret.diagnostics
    );
    let ctx = ScriptParseContext::new(0, source.as_bytes());
    let entries = extract_bindings(&ret.program, &ctx, &vue_aliases(&ret.program, &ctx));
    entries
        .into_iter()
        .map(|(span, bt)| {
            let name = &source[span.start as usize..span.end as usize];
            (name.to_string(), bt)
        })
        .collect()
}

/// The `vue` import aliases of a parsed `<script setup>` program, from the
/// same import facts `parse_script` collects.
fn vue_aliases<'a>(
    program: &oxc_ast::ast::Program<'a>,
    ctx: &ScriptParseContext<'a>,
) -> VueImportAliases<'a> {
    let mut aliases = VueImportAliases::default();
    for stmt in &program.body {
        if let Some(ScriptItem::Import(import)) = try_process_import(stmt, ctx) {
            aliases.register_setup_import(&import);
        }
    }
    aliases
}

/// Imports every Vue reactivity API the classifier recognises, so a snippet
/// can call them the way an authored `<script setup>` does.
const VUE_IMPORTS: &str =
    "import { ref, computed, shallowRef, toRef, customRef, reactive, useTemplateRef } from 'vue';
";

/// `classify` over `source` preceded by the `vue` imports.
fn classify_with_vue_imports(source: &str) -> Vec<(String, BindingType)> {
    classify(&format!("{VUE_IMPORTS}{source}"))
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

/// A dynamic template literal is a string, never a ref (Vue's `canNeverBeRef`
/// accepts every literal node).
#[test]
fn const_dynamic_template_literal() {
    let b = classify("const x = `${dynamic}`;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
}

// ── Static compositions + TS-unwrapped scalars → LiteralConst ─────────
// Official `walkDeclaration`: `isStaticNode(unwrapTSNode(init))` → literal-const.

/// @ai-generated — unary of a literal is static → literal-const
#[test]
fn const_static_unary() {
    let b = classify("const x = -1;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — binary of literals is static → literal-const
#[test]
fn const_static_binary() {
    let b = classify("const x = 1 + 2;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — nested static binary is still static → literal-const
#[test]
fn const_static_nested_binary() {
    let b = classify("const x = 1 + 2 * 3;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — unary of a boolean literal is static → literal-const
#[test]
fn const_static_logical_not() {
    let b = classify("const x = !true;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — `as const` over a scalar unwraps to a literal → literal-const
#[test]
fn const_ts_as_const_scalar() {
    let b = classify("const x = 5 as const;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — `satisfies` over a static composition unwraps → literal-const
#[test]
fn const_ts_satisfies_static_binary() {
    let b = classify("const x = (1 + 2) satisfies number;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — non-null `!` over a literal unwraps to a literal → literal-const
#[test]
fn const_ts_nonnull_scalar() {
    let b = classify("const x = 1!;");
    assert_eq!(find(&b, "x"), Some(BindingType::LiteralConst));
}

/// @ai-generated — a NON-static binary of free identifiers can never be a ref →
/// setup-const (the isStaticNode widening must not leak past static operands)
#[test]
fn const_nonstatic_binary_stays_setup_const() {
    let b = classify("const x = a + b;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
}

/// @ai-generated — an object literal is NOT static (official isStaticNode) →
/// setup-const, even under a TS wrapper
#[test]
fn const_object_literal_under_ts_wrapper_stays_setup_const() {
    let b = classify("const x = { a: 1 } as const;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
}

/// @ai-generated — an array literal is NOT static → setup-const
#[test]
fn const_array_literal_stays_setup_const() {
    let b = classify("const x = [1, 2];");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
}

/// @ai-generated — a ref call under a TS cast still classifies as setup-ref
/// (unwrapTSNode runs before the ref-call check, matching official)
#[test]
fn const_ref_call_under_ts_cast_is_setup_ref() {
    let b = classify_with_vue_imports("const x = ref(0) as any;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

// ── Variable declarations: reactivity helpers ────────────────────────

/// @ai-generated
#[test]
fn const_ref() {
    let b = classify_with_vue_imports("const x = ref(0);");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

/// @ai-generated
#[test]
fn const_computed() {
    let b = classify_with_vue_imports("const x = computed(() => 1);");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

/// @ai-generated
#[test]
fn const_shallow_ref() {
    let b = classify_with_vue_imports("const x = shallowRef({});");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

/// @ai-generated
#[test]
fn const_to_ref() {
    let b = classify_with_vue_imports("const x = toRef(props, 'a');");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

/// @ai-generated
#[test]
fn const_custom_ref() {
    let b = classify_with_vue_imports("const x = customRef((track, trigger) => ({}));");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
}

/// @ai-generated
#[test]
fn const_reactive() {
    let b = classify_with_vue_imports("const x = reactive({});");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupReactiveConst));
}

/// @ai-generated
#[test]
fn const_shallow_reactive_is_maybe_ref_like_official() {
    // Official only special-cases `reactive`; `shallowReactive` is an ordinary call.
    let b = classify("const x = shallowReactive({});");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
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
fn const_other_call_is_maybe_ref() {
    // `canNeverBeRef` is false for a call: the callee may return a ref.
    let b = classify("const x = someOtherCall();");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated
#[test]
fn const_member_call_is_maybe_ref() {
    let b = classify("const x = obj.method();");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
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
    let b = classify_with_vue_imports("const a = 1, b = ref(0);");
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

/// @ai-generated — `const props = defineProps()` is a setup binding (the whole object),
/// not an individual prop. Uses `$setup.props` not `$props.props`.
#[test]
fn const_define_props_whole_object() {
    let b = classify("const props = defineProps({ msg: String });");
    assert_eq!(find(&b, "props"), Some(BindingType::SetupConst));
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

/// @ai-generated — `const props = withDefaults(...)` is a setup binding (the whole object).
#[test]
fn const_with_defaults_whole_object() {
    let b = classify("const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hi' });");
    assert_eq!(find(&b, "props"), Some(BindingType::SetupConst));
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

/// A call to a function declared in the same script may return a ref, so it
/// is maybe-ref and `v-model` on it cannot be rejected as a const reassignment.
#[test]
fn const_call_of_local_function_is_maybe_ref() {
    let b = classify(
        "function make() { return ref(''); }
const other = make();",
    );
    assert_eq!(find(&b, "other"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "make"), Some(BindingType::SetupConst));
}

/// A TS wrapper around a call classifies like the call itself.
#[test]
fn const_call_under_ts_wrapper_is_maybe_ref() {
    let b = classify("const x = (someFn() as number);");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
}

/// `useTemplateRef` creates a ref (official ref-creating API list).
#[test]
fn const_use_template_ref_is_setup_ref() {
    let b = classify_with_vue_imports("const el = useTemplateRef('el');");
    assert_eq!(find(&b, "el"), Some(BindingType::SetupRef));
}

/// A call inside a nested expression that cannot be a ref stays const.
#[test]
fn const_binary_of_calls_stays_setup_const() {
    let b = classify("const x = a() + b();");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupConst));
}

/// `let` bindings stay `SetupLet` whatever the initializer call is.
#[test]
fn let_call_stays_setup_let() {
    let b = classify("let x = someFn();");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupLet));
}

/// An aliased `vue` import is recognised under its local name.
#[test]
fn const_aliased_vue_import_call_is_setup_ref() {
    let b = classify(
        "import { ref as r, reactive as rx } from 'vue';
const a = r(0);
const b = rx({});",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupRef));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupReactiveConst));
}

/// The original name of an aliased import is no longer the `vue` export.
#[test]
fn const_call_of_original_name_after_alias_import_is_maybe_ref() {
    let b = classify(
        "import { ref as r } from 'vue';
const a = ref(0);",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// A same-named local function is an ordinary call, not `vue`'s `ref`.
#[test]
fn const_call_of_local_function_named_like_a_vue_api_is_maybe_ref() {
    let b = classify(
        "function ref(v) { return v; }
const a = ref(0);",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// `ref` imported from another module is not `vue`'s `ref`.
#[test]
fn const_call_of_same_named_import_from_another_module_is_maybe_ref() {
    let b = classify(
        "import { ref } from './my-reactivity';
const a = ref(0);",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// An auto-imported `ref` has no import statement, so Vue treats it as an
/// ordinary call.
#[test]
fn const_call_of_unimported_ref_is_maybe_ref() {
    let b = classify("const a = ref(0);");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// Two aliases of the same `vue` export: only the last one is recognised,
/// as Vue's alias table keeps one local name per export.
#[test]
fn const_call_of_earlier_alias_of_a_twice_imported_api_is_maybe_ref() {
    let b = classify(
        "import { ref as a } from 'vue';
import { ref as b } from 'vue';
const x = a(0);
const y = b(0);",
    );
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "y"), Some(BindingType::SetupRef));
}

/// A local name bound by an earlier import keeps that binding, so a later
/// `vue` import under the same name does not make it `vue`'s API.
#[test]
fn const_call_of_name_first_imported_elsewhere_is_maybe_ref() {
    let b = classify(
        "import { ref } from './my-reactivity';
import { ref } from 'vue';
const a = ref(0);",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// A sequence ending in `vue`'s `reactive` (under its alias) can never be a
/// ref, while one ending in an unknown call may be.
#[test]
fn const_sequence_ending_in_aliased_reactive_is_setup_const() {
    let b = classify(
        "import { reactive as rx } from 'vue';
const a = (0, rx({}));
const b = (0, reactive({}));",
    );
    assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
}

/// Parentheses are transparent, around the call and around its callee: Vue
/// reads a Babel AST that keeps none.
#[test]
fn const_parenthesized_ref_call_is_setup_ref() {
    let b = classify_with_vue_imports(
        "const x = (ref(0));
const y = (ref)(0);
const z = (reactive)({});",
    );
    assert_eq!(find(&b, "x"), Some(BindingType::SetupRef));
    assert_eq!(find(&b, "y"), Some(BindingType::SetupRef));
    assert_eq!(find(&b, "z"), Some(BindingType::SetupReactiveConst));
}

/// A non-static literal (a regular expression) can never be a ref.
#[test]
fn const_regexp_literal_is_setup_const() {
    let b = classify("const r = /a+/;");
    assert_eq!(find(&b, "r"), Some(BindingType::SetupConst));
}

// ── Standalone expression macros ─────────────────────────────────────

/// Typed macro surfaces are registered from TypeInfo DTOs, not parser bindings.
#[test]
fn standalone_define_props_typed() {
    let b = classify("defineProps<{ msg: string }>();");
    assert_eq!(find(&b, "msg"), None);
}

/// @ai-generated
#[test]
fn standalone_define_props_multi_props() {
    let b = classify("defineProps<{ msg: string; count: number }>();");
    assert_eq!(find(&b, "msg"), None);
    assert_eq!(find(&b, "count"), None);
}

/// @ai-generated
#[test]
fn standalone_with_defaults_typed() {
    let b = classify("withDefaults(defineProps<{ msg: string }>(), { msg: 'hi' });");
    assert_eq!(find(&b, "msg"), None);
}

// ── Props: runtime object syntax ────────────────────────────────────

/// @ai-generated — standalone `defineProps({ foo: String })` should extract individual props
#[test]
fn standalone_define_props_runtime_object() {
    let b = classify("defineProps({ foo: String, bar: Number });");
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Runtime object prop 'foo' should be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Runtime object prop 'bar' should be Props"
    );
}

/// @ai-generated — runtime object with nested options `{ type: String, required: true }`
#[test]
fn standalone_define_props_runtime_object_nested() {
    let b = classify("defineProps({ foo: { type: String, required: true }, bar: Number });");
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Nested runtime prop 'foo' should be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Simple runtime prop 'bar' should be Props"
    );
}

// ── Props: array syntax ─────────────────────────────────────────────

/// @ai-generated — standalone `defineProps(['foo', 'bar'])` should extract individual props
#[test]
fn standalone_define_props_array_syntax() {
    let b = classify("defineProps(['foo', 'bar']);");
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Array syntax prop 'foo' should be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Array syntax prop 'bar' should be Props"
    );
}

// ── Props: type reference (interface / type alias) ──────────────────

/// Type references remain semantic and do not enter parser binding inventory.
#[test]
fn standalone_define_props_interface_reference() {
    let b = classify("interface MyProps { foo: string; bar: number }\ndefineProps<MyProps>();");
    assert_eq!(
        find(&b, "foo"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
    assert_eq!(
        find(&b, "bar"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
}

/// @ai-generated — `defineProps<MyType>()` with local type alias should resolve props
#[test]
fn standalone_define_props_type_alias_reference() {
    let b = classify("type MyProps = { msg: string }\ndefineProps<MyProps>();");
    assert_eq!(
        find(&b, "msg"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
}

/// @ai-generated — `withDefaults(defineProps<MyInterface>(), {...})` with local interface
#[test]
fn standalone_with_defaults_interface_reference() {
    let b = classify(
        "interface MyProps { foo?: string; bar?: number }\nwithDefaults(defineProps<MyProps>(), { foo: 'hi' });",
    );
    assert_eq!(
        find(&b, "foo"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
    assert_eq!(
        find(&b, "bar"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
}

// ── Props: declarator + individual prop extraction ───────────────────

/// Typed prop names stay out of the syntax binding inventory.
#[test]
fn const_define_props_typed_records_only_authored_declarator() {
    let b = classify("const props = defineProps<{ foo: string, bar: number }>();");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "foo"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
    assert_eq!(
        find(&b, "bar"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
}

/// @ai-generated — `const props = defineProps({ foo: String })` should extract
/// both `props` as SetupConst AND `foo` as Props
#[test]
fn const_define_props_runtime_also_extracts_individual_props() {
    let b = classify("const props = defineProps({ foo: String, bar: Number });");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Individual runtime prop 'foo' should also be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Individual runtime prop 'bar' should also be Props"
    );
}

/// withDefaults does not change ownership of typed prop semantics.
#[test]
fn const_with_defaults_typed_records_only_authored_declarator() {
    let b =
        classify("const props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' });");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "foo"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
}

/// Interface expansion is owned by TypeInfo.
#[test]
fn const_define_props_interface_ref_records_only_authored_declarator() {
    let b = classify("interface MyProps { title: string }\nconst props = defineProps<MyProps>();");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "title"),
        None,
        "typed prop names are registered from the TypeInfo DTO"
    );
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
    // Official `isAllLiteral`: an enum whose members all have no initializer
    // or scalar-literal initializers is a literal-const binding.
    let b = classify("enum Direction { Up, Down }");
    assert_eq!(find(&b, "Direction"), Some(BindingType::LiteralConst));
}

/// @ai-generated
#[test]
fn enum_declaration_non_literal_member_is_setup_const() {
    // A non-literal member (call expression) keeps the enum a setup binding.
    let b = classify("enum E { A = make() }");
    assert_eq!(find(&b, "E"), Some(BindingType::SetupConst));
}

/// @ai-generated — official `isAllLiteral` runs `isStaticNode` (which
/// `unwrapTSNode`s first), so members carrying `as const` / `satisfies` /
/// non-null `!` over scalar literals are still all-literal → literal-const.
#[test]
fn enum_ts_wrapped_literal_members_are_literal_const() {
    let b = classify("enum E { A = 1 as const, B = 2 satisfies number, C = 3! }");
    assert_eq!(find(&b, "E"), Some(BindingType::LiteralConst));
}

/// @ai-generated — a TS wrapper over a NON-static member (a call) does NOT make
/// it static, so the enum stays setup-const (the unwrap must not over-accept).
#[test]
fn enum_ts_wrapped_non_static_member_stays_setup_const() {
    let b = classify("enum E { A = make() as const }");
    assert_eq!(find(&b, "E"), Some(BindingType::SetupConst));
}

/// @ai-generated — a static composition member (binary of literals) is static →
/// the enum is all-literal.
#[test]
fn enum_static_composition_member_is_literal_const() {
    let b = classify("enum E { A = 1 + 2, B = -3 }");
    assert_eq!(find(&b, "E"), Some(BindingType::LiteralConst));
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
    assert_eq!(find(&b, "bar"), Some(BindingType::SetupImport));
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
    assert_eq!(find(&b, "Foo"), Some(BindingType::SetupImport));
}

/// @ai-generated
#[test]
fn import_named() {
    let b = classify("import { ref } from 'vue';");
    assert_eq!(find(&b, "ref"), Some(BindingType::SetupImport));
}

/// @ai-generated
#[test]
fn import_namespace() {
    let b = classify("import * as utils from './utils';");
    assert_eq!(find(&b, "utils"), Some(BindingType::SetupImport));
}

/// @ai-generated
#[test]
fn import_multiple_named() {
    let b = classify("import { a, b } from 'mod';");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupImport));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupImport));
}

// ── Destructuring ────────────────────────────────────────────────────

/// @ai-generated — destructured from identifier (might be ref) → SetupMaybeRef
#[test]
fn const_object_destructure() {
    let b = classify("const { a, b } = someObj;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — destructured from identifier (might be ref) → SetupMaybeRef
#[test]
fn const_array_destructure() {
    let b = classify("const [a, b] = someArr;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated
#[test]
fn let_object_destructure() {
    let b = classify("let { a, b } = someObj;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupLet));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupLet));
}

/// @ai-generated — destructured from identifier with default → SetupMaybeRef
#[test]
fn const_destructure_with_default() {
    let b = classify("const { a = 1 } = someObj;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — nested destructure from identifier → SetupMaybeRef
#[test]
fn const_nested_destructure() {
    let b = classify("const { a: { b } } = someObj;");
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
    // 'a' is not bound — it's a property key, not a binding
    assert_eq!(find(&b, "a"), None);
}

/// A rest element collects a fresh array, so it can never be a ref.
#[test]
fn const_array_rest() {
    let b = classify("const [a, ...rest] = someArr;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "rest"), Some(BindingType::SetupConst));
}

/// A rest property collects a fresh object, so it can never be a ref.
#[test]
fn const_object_rest() {
    let b = classify("const { a, ...rest } = someObj;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "rest"), Some(BindingType::SetupConst));
}

/// A destructured binding is maybe-ref whatever the initializer: the
/// literal's elements may hold refs, and Vue never inspects them.
#[test]
fn const_destructure_from_array_literal() {
    let b = classify("const [a, b] = [1, 2];");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
}

/// A destructured binding is maybe-ref even from an object literal.
#[test]
fn const_destructure_from_object_literal() {
    let b = classify("const { a, b } = { a: 1, b: 2 };");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
}

/// Destructuring a `vue` ref-creating call does not make the bindings refs.
#[test]
fn const_destructure_of_ref_call_is_maybe_ref() {
    let b = classify_with_vue_imports("const { a } = ref({ a: 1 });");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
}

/// Under a declaring macro the outermost bindings and the rest are consts;
/// a nested pattern's bindings are maybe-ref, as in Vue.
#[test]
fn const_destructure_of_define_emits_keeps_outer_bindings_const() {
    let b = classify("const { a, b: { c }, d = 1, ...r } = defineEmits(['a']);");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "c"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "d"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "r"), Some(BindingType::SetupConst));
}

/// @ai-generated — ternary expression might be ref → SetupMaybeRef
#[test]
fn const_ternary_expression() {
    let b = classify("const x = cond ? a : b;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — member expression might be ref → SetupMaybeRef
#[test]
fn const_member_expression() {
    let b = classify("const x = obj.prop;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — await expression might be ref → SetupMaybeRef
#[test]
fn const_await_expression() {
    let b = classify("const x = await fetchData();");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
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
    assert_eq!(find(&b, "ref"), Some(BindingType::SetupImport));
    assert_eq!(find(&b, "Ref"), None);
    assert_eq!(find(&b, "MyType"), None);
    assert_eq!(find(&b, "MyInterface"), None);
    assert_eq!(find(&b, "count"), Some(BindingType::SetupRef));
    assert_eq!(find(&b, "name"), Some(BindingType::LiteralConst));
    assert_eq!(find(&b, "mutable"), Some(BindingType::SetupLet));
    assert_eq!(find(&b, "doSomething"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "MyClass"), Some(BindingType::SetupConst));
    // All-literal enum (no member initializers) → literal-const (official).
    assert_eq!(find(&b, "Color"), Some(BindingType::LiteralConst));
}

/// @ai-generated
#[test]
fn spans_are_local_coordinates() {
    // Statement/expression spans come directly from OXC.
    // content_offset only affects TS type annotation spans.
    let source = "const x = ref(0);";
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
    let ctx = ScriptParseContext::new(0, source.as_bytes());
    let entries = extract_bindings(&ret.program, &ctx, &VueImportAliases::default());
    assert_eq!(entries.len(), 1);
    let (span, bt) = &entries[0];
    // 'x' is at position 6 in source
    assert_eq!(span.start, 6);
    assert_eq!(span.end, 7);
    assert_eq!(*bt, BindingType::SetupMaybeRef);
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

/// @ai-generated — array destructure with holes from identifier → SetupMaybeRef
#[test]
fn array_destructure_with_holes() {
    let b = classify("const [, b, , d] = arr;");
    assert_eq!(b.len(), 2);
    assert_eq!(find(&b, "b"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "d"), Some(BindingType::SetupMaybeRef));
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

// ── Full-program classification pin ──────────────────────────────────

/// Pins the COMPLETE ordered `(name, BindingType)` output of
/// `extract_bindings` for a representative `<script setup>` program
/// covering every classification family: imports (value, type-only),
/// reactivity helpers, literals, `defineProps` destructuring, plain consts,
/// `let` bindings, and
/// function / class / enum declarations. The Vue classification layer
/// delegates generic statement/pattern inventory to the neutral script
/// bindings module; this pin holds that delegation byte-stable.
#[test]
fn full_script_setup_classification_pin() {
    let source = r#"
import Comp from './Comp.vue';
import { helper, util } from './helpers';
import type { OnlyType } from './types';
import { computed, ref, reactive } from 'vue';

const { foo, bar } = defineProps<{ foo: string; bar?: number }>();
const count = ref(0);
const doubled = computed(() => count.value * 2);
const state = reactive({ on: false });
const label = 'static';
const pieces = [1, 2, 3];
let mutable = 0;

function describe(): string {
  return label;
}
class Widget {}
enum Mode {
  A,
  B,
}
type Ignored = string;
interface AlsoIgnored {
  x: number;
}
"#;
    let actual = classify(source);
    let expected: Vec<(String, BindingType)> = vec![
        ("Comp".to_string(), BindingType::SetupImport),
        ("helper".to_string(), BindingType::SetupImport),
        ("util".to_string(), BindingType::SetupImport),
        ("computed".to_string(), BindingType::SetupImport),
        ("ref".to_string(), BindingType::SetupImport),
        ("reactive".to_string(), BindingType::SetupImport),
        ("foo".to_string(), BindingType::PropsAliased),
        ("bar".to_string(), BindingType::PropsAliased),
        ("count".to_string(), BindingType::SetupRef),
        ("doubled".to_string(), BindingType::SetupRef),
        ("state".to_string(), BindingType::SetupReactiveConst),
        ("label".to_string(), BindingType::LiteralConst),
        ("pieces".to_string(), BindingType::SetupConst),
        ("mutable".to_string(), BindingType::SetupLet),
        ("describe".to_string(), BindingType::SetupConst),
        ("Widget".to_string(), BindingType::SetupConst),
        // All-literal enum → literal-const (official `isAllLiteral`).
        ("Mode".to_string(), BindingType::LiteralConst),
    ];
    assert_eq!(
        actual, expected,
        "Vue <script setup> binding classification changed"
    );
    // Type-only constructs never produce runtime bindings.
    assert_eq!(find(&actual, "OnlyType"), None);
    assert_eq!(find(&actual, "Ignored"), None);
    assert_eq!(find(&actual, "AlsoIgnored"), None);
}
