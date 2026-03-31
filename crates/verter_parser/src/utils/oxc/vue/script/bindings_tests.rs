use super::*;
use crate::types::BindingType;
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

/// @ai-generated — dynamic template literal might evaluate to a ref
#[test]
fn const_dynamic_template_literal() {
    let b = classify("const x = `${dynamic}`;");
    assert_eq!(find(&b, "x"), Some(BindingType::SetupMaybeRef));
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

/// @ai-generated — `defineProps<MyInterface>()` with local interface should resolve props
#[test]
fn standalone_define_props_interface_reference() {
    let b = classify("interface MyProps { foo: string; bar: number }\ndefineProps<MyProps>();");
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Interface-referenced prop 'foo' should be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Interface-referenced prop 'bar' should be Props"
    );
}

/// @ai-generated — `defineProps<MyType>()` with local type alias should resolve props
#[test]
fn standalone_define_props_type_alias_reference() {
    let b = classify("type MyProps = { msg: string }\ndefineProps<MyProps>();");
    assert_eq!(
        find(&b, "msg"),
        Some(BindingType::Props),
        "Type-alias-referenced prop 'msg' should be Props"
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
        Some(BindingType::Props),
        "withDefaults + interface prop 'foo' should be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "withDefaults + interface prop 'bar' should be Props"
    );
}

// ── Props: declarator + individual prop extraction ───────────────────

/// @ai-generated — `const props = defineProps<{ foo: string }>()` should extract
/// both `props` as SetupConst AND `foo` as Props
#[test]
fn const_define_props_typed_also_extracts_individual_props() {
    let b = classify("const props = defineProps<{ foo: string, bar: number }>();");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Individual typed prop 'foo' should also be Props"
    );
    assert_eq!(
        find(&b, "bar"),
        Some(BindingType::Props),
        "Individual typed prop 'bar' should also be Props"
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

/// @ai-generated — `const props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })`
/// should extract both `props` as SetupConst AND `foo` as Props
#[test]
fn const_with_defaults_typed_also_extracts_individual_props() {
    let b =
        classify("const props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' });");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "foo"),
        Some(BindingType::Props),
        "Individual prop 'foo' from withDefaults should also be Props"
    );
}

/// @ai-generated — `const props = defineProps<MyInterface>()` with local interface
#[test]
fn const_define_props_interface_ref_also_extracts_individual_props() {
    let b = classify("interface MyProps { title: string }\nconst props = defineProps<MyProps>();");
    assert_eq!(
        find(&b, "props"),
        Some(BindingType::SetupConst),
        "Declarator 'props' should be SetupConst"
    );
    assert_eq!(
        find(&b, "title"),
        Some(BindingType::Props),
        "Interface prop 'title' should be Props"
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

/// @ai-generated — array rest from identifier → SetupMaybeRef
#[test]
fn const_array_rest() {
    let b = classify("const [a, ...rest] = someArr;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "rest"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — object rest from identifier → SetupMaybeRef
#[test]
fn const_object_rest() {
    let b = classify("const { a, ...rest } = someObj;");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupMaybeRef));
    assert_eq!(find(&b, "rest"), Some(BindingType::SetupMaybeRef));
}

/// @ai-generated — destructured from array literal (canNeverBeRef) → SetupConst
#[test]
fn const_destructure_from_array_literal() {
    let b = classify("const [a, b] = [1, 2];");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
}

/// @ai-generated — destructured from object literal (canNeverBeRef) → SetupConst
#[test]
fn const_destructure_from_object_literal() {
    let b = classify("const { a, b } = { a: 1, b: 2 };");
    assert_eq!(find(&b, "a"), Some(BindingType::SetupConst));
    assert_eq!(find(&b, "b"), Some(BindingType::SetupConst));
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
    assert_eq!(find(&b, "Color"), Some(BindingType::SetupConst));
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
    let entries = extract_bindings(&ret.program, &ctx);
    assert_eq!(entries.len(), 1);
    let (span, bt) = &entries[0];
    // 'x' is at position 6 in source
    assert_eq!(span.start, 6);
    assert_eq!(span.end, 7);
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
