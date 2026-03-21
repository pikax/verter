use super::*;

fn analyze(code: &str) -> ScriptAnalysisSnapshot {
    let alloc = Allocator::new();
    build_script_analysis(code, SourceType::ts(), &alloc)
}

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

#[test]
fn ref_binding_is_reactive() {
    let result = analyze("import { ref } from 'vue';\nconst count = ref(0);");
    assert!(result
        .bindings
        .iter()
        .any(|b| b.name == "count" && b.is_reactive));
    assert!(result.flags.contains(AnalysisFlags::HAS_REACTIVE_STATE));
}

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

#[test]
fn define_props_runtime() {
    let result = analyze("defineProps({foo: String});");
    assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_PROPS));
    assert!(!result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_PROPS));
    assert!(result.macro_type_deps.is_empty());
}

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

#[test]
fn lifecycle_hooks_flag() {
    let result = analyze("import { onMounted } from 'vue';");
    assert!(result.flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS));
}

#[test]
fn watcher_flag() {
    let result = analyze("import { watch, watchEffect } from 'vue';");
    assert!(result.flags.contains(AnalysisFlags::HAS_WATCHERS));
}

#[test]
fn provide_inject_flags() {
    let result = analyze("import { provide, inject } from 'vue';");
    assert!(result.flags.contains(AnalysisFlags::HAS_PROVIDE));
    assert!(result.flags.contains(AnalysisFlags::HAS_INJECT));
}

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

#[test]
fn empty_content() {
    let result = analyze("");
    assert!(result.imports.is_empty());
    assert!(result.macros.is_empty());
    assert!(result.bindings.is_empty());
    assert_eq!(result.flags, AnalysisFlags::empty());
}

#[test]
fn function_and_class_bindings() {
    let result = analyze("function helper() {}\nclass MyClass {}");
    let names: Vec<&str> = result.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"helper"));
    assert!(names.contains(&"MyClass"));
}

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

#[test]
fn collects_static_module_references() {
    let result = analyze(
        "import Foo from './Foo.vue';\nexport { Bar } from './bar';\nexport * from './baz';",
    );
    assert_eq!(result.module_references.len(), 3);

    assert_eq!(
        result.module_references[0].syntax,
        ModuleReferenceSyntax::StaticImport
    );
    assert_eq!(
        result.module_references[0].analyzability,
        ModuleReferenceAnalyzability::Exact
    );
    assert_eq!(
        result.module_references[0].literal_specifier.as_deref(),
        Some("./Foo.vue")
    );

    assert_eq!(
        result.module_references[1].syntax,
        ModuleReferenceSyntax::ExportFrom
    );
    assert_eq!(
        result.module_references[2].literal_specifier.as_deref(),
        Some("./baz")
    );
}

#[test]
fn collects_dynamic_module_references() {
    let result = analyze(
        "const target = './Foo.vue';\nconst branch = cond ? './a' : './b';\nimport(target);\nimport(branch);\nrequire(`./widgets/${name}`);",
    );
    assert_eq!(result.module_references.len(), 3);

    assert_eq!(
        result.module_references[0].literal_specifier.as_deref(),
        Some("./Foo.vue")
    );
    assert_eq!(
        result.module_references[0].analyzability,
        ModuleReferenceAnalyzability::Exact
    );

    assert_eq!(
        result.module_references[1].finite_specifiers,
        vec!["./a".to_string(), "./b".to_string()]
    );
    assert_eq!(
        result.module_references[1].analyzability,
        ModuleReferenceAnalyzability::FiniteSet
    );

    assert_eq!(
        result.module_references[2].syntax,
        ModuleReferenceSyntax::RequireCall
    );
    assert_eq!(
        result.module_references[2].analyzability,
        ModuleReferenceAnalyzability::UnknownDynamic
    );
    assert_eq!(
        result.module_references[2].static_prefix.as_deref(),
        Some("./widgets/")
    );
}

#[test]
fn define_model_type_based() {
    let result = analyze("const model = defineModel<string>();");
    assert!(result.flags.contains(AnalysisFlags::HAS_DEFINE_MODEL));
    assert!(result.flags.contains(AnalysisFlags::HAS_TYPE_BASED_MODEL));
}

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

#[test]
fn aliased_ref_binding_is_reactive() {
    let result = analyze("import { ref as createRef } from 'vue';\nconst count = createRef(0);");
    assert!(
        result
            .bindings
            .iter()
            .any(|b| b.name == "count" && b.is_reactive),
        "binding initialized with aliased ref should still be detected as reactive"
    );
    assert!(result.flags.contains(AnalysisFlags::HAS_REACTIVE_STATE));
}

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

#[test]
fn async_setup_flag_for_top_level_await() {
    let result = analyze("const data = await fetchData();");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "top-level await in variable initializer should set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_flag_for_await_expression_statement() {
    let result = analyze("await someAsyncCall();");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "top-level await expression statement should set ASYNC_SETUP flag"
    );
}

#[test]
fn destructured_object_bindings_captured() {
    let result = analyze("const { a, b } = someObject;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "a");
    assert_eq!(result.bindings[0].kind, AnalyzedBindingKind::Const);
    assert_eq!(result.bindings[1].name, "b");
    assert_eq!(result.bindings[1].kind, AnalyzedBindingKind::Const);
    // No type annotations on destructured bindings
    assert!(result.bindings[0].type_annotation.is_none());
}

#[test]
fn destructured_array_bindings_captured() {
    let result = analyze("const [x, y] = someArray;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "x");
    assert_eq!(result.bindings[1].name, "y");
}

#[test]
fn nested_destructured_bindings_captured() {
    let result = analyze("const { a: { b, c } } = obj;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "b");
    assert_eq!(result.bindings[1].name, "c");
    // 'a' is NOT a binding — it's the property key, not an identifier
}

#[test]
fn destructured_rest_element_captured() {
    let result = analyze("const { a, ...rest } = obj;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "a");
    assert_eq!(result.bindings[1].name, "rest");
}

#[test]
fn destructured_from_reactive_call() {
    let result = analyze("import { toRefs } from 'vue';\nconst { a, b } = toRefs(state);");
    // toRefs is a reactivity API
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "a");
    assert_eq!(result.bindings[1].name, "b");
    assert!(result.bindings[0].is_reactive);
    assert!(result.bindings[1].is_reactive);
}

#[test]
fn destructured_with_defaults_captured() {
    let result = analyze("const { a = 1, b = 'hello' } = obj;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].name, "a");
    assert_eq!(result.bindings[1].name, "b");
}

#[test]
fn destructured_let_bindings_are_mutable() {
    let result = analyze("let { a, b } = obj;");
    assert_eq!(result.bindings.len(), 2);
    assert_eq!(result.bindings[0].kind, AnalyzedBindingKind::Let);
    assert_eq!(result.bindings[0].reactivity_kind, ReactivityKind::Mutable);
}

// ═══════════════════════════════════════════════════════════
// Transitive type dep discovery tests
// ═══════════════════════════════════════════════════════════

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

#[test]
fn transitive_dep_interface_extends_pick_of_imported() {
    let code = r#"
import type { BaseProps } from './types';
interface Local extends Pick<BaseProps, 'a' | 'b'> { own: string }
defineProps<Local>();
"#;
    let result = analyze(code);
    assert!(
        result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "BaseProps" && d.import_source == "./types"),
        "should discover BaseProps via utility heritage, got: {:?}",
        result.macro_type_deps
    );
}

#[test]
fn transitive_dep_interface_extends_omit_of_imported_with_imported_keys() {
    let code = r#"
import type { ButtonProps, LinkPropsKeys } from './types';
interface ChildProps extends Omit<ButtonProps, LinkPropsKeys | 'icon' | 'color' | 'variant'> {
  status?: string
}
defineProps<ChildProps>();
"#;
    let result = analyze(code);
    assert!(
        result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "ButtonProps" && d.import_source == "./types"),
        "should discover ButtonProps via utility heritage, got: {:?}",
        result.macro_type_deps
    );
    assert!(
        result
            .macro_type_deps
            .iter()
            .any(|d| d.type_name == "LinkPropsKeys" && d.import_source == "./types"),
        "should discover LinkPropsKeys via utility heritage, got: {:?}",
        result.macro_type_deps
    );
}

#[test]
fn async_setup_nested_await_in_call_arg() {
    let result = analyze("const data = bar(await fetchData());");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "nested await in function call argument should set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_await_in_array() {
    let result = analyze("const data = [await a(), await b()];");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "await in array expression should set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_await_in_ternary() {
    let result = analyze("const data = cond ? await fetchA() : fallback;");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "await in ternary expression should set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_not_set_for_await_in_arrow() {
    let result = analyze("const fn = async () => await fetchData();");
    assert!(
        !result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "await inside arrow function body should NOT set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_not_set_for_await_in_function_expr() {
    let result = analyze("const fn = async function() { await fetchData(); };");
    assert!(
        !result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "await inside function expression body should NOT set ASYNC_SETUP flag"
    );
}

#[test]
fn async_setup_await_in_object_value() {
    let result = analyze("const data = { result: await fetch() };");
    assert!(
        result.flags.contains(AnalysisFlags::ASYNC_SETUP),
        "await in object property value should set ASYNC_SETUP flag"
    );
}

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

#[test]
fn binding_span_matches_source_position() {
    let code = "const count = ref(0);";
    let result = analyze(code);
    assert_eq!(result.bindings.len(), 1);
    let b = &result.bindings[0];
    // "count" starts at position 6, ends at 11
    assert_eq!(&code[b.span.start as usize..b.span.end as usize], "count");
}

#[test]
fn import_span_covers_full_declaration() {
    let code = "import { ref } from 'vue';";
    let result = analyze(code);
    assert_eq!(result.imports.len(), 1);
    let imp = &result.imports[0];
    assert_eq!(
        &code[imp.span.start as usize..imp.span.end as usize],
        "import { ref } from 'vue';"
    );
}

#[test]
fn import_binding_span_covers_specifier() {
    let code = "import { ref, computed } from 'vue';";
    let result = analyze(code);
    let bindings = &result.imports[0].bindings;
    assert_eq!(bindings.len(), 2);
    assert_eq!(
        &code[bindings[0].span.start as usize..bindings[0].span.end as usize],
        "ref"
    );
    assert_eq!(
        &code[bindings[1].span.start as usize..bindings[1].span.end as usize],
        "computed"
    );
}

#[test]
fn macro_span_covers_call_expression() {
    let code = "defineProps<{msg: string}>();";
    let result = analyze(code);
    assert_eq!(result.macros.len(), 1);
    let m = &result.macros[0];
    // The call span should cover the entire call expression
    let span_text = &code[m.span.start as usize..m.span.end as usize];
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

#[test]
fn function_binding_span_covers_name() {
    let code = "function handleClick() {}";
    let result = analyze(code);
    assert_eq!(result.bindings.len(), 1);
    let b = &result.bindings[0];
    assert_eq!(
        &code[b.span.start as usize..b.span.end as usize],
        "handleClick"
    );
}

#[test]
fn class_binding_span_covers_name() {
    let code = "class MyService {}";
    let result = analyze(code);
    assert_eq!(result.bindings.len(), 1);
    let b = &result.bindings[0];
    assert_eq!(
        &code[b.span.start as usize..b.span.end as usize],
        "MyService"
    );
}

#[test]
fn multiple_binding_spans_distinct() {
    let code = "const a = 1;\nconst b = 2;";
    let result = analyze(code);
    assert_eq!(result.bindings.len(), 2);
    let a = &result.bindings[0];
    let b = &result.bindings[1];
    assert_eq!(&code[a.span.start as usize..a.span.end as usize], "a");
    assert_eq!(&code[b.span.start as usize..b.span.end as usize], "b");
    // Ensure spans don't overlap
    assert!(a.span.end <= b.span.start);
}

#[test]
fn import_resolved_canonical_id_none_by_default() {
    let result = analyze("import { ref } from 'vue';");
    assert!(result.imports[0].resolved_canonical_id.is_none());
}

// ═══════════════════════════════════════════════════════════
// Phase 1b: ReactivityKind classification tests
// ═══════════════════════════════════════════════════════════

#[test]
fn ref_classified_as_ref_kind() {
    let result = analyze("import { ref } from 'vue';\nconst count = ref(0);");
    let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
}

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

#[test]
fn reactive_classified_as_reactive_kind() {
    let result = analyze("import { reactive } from 'vue';\nconst state = reactive({ x: 1 });");
    let b = result.bindings.iter().find(|b| b.name == "state").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Reactive);
}

#[test]
fn shallow_ref_classified_as_ref_kind() {
    let result = analyze("import { shallowRef } from 'vue';\nconst data = shallowRef(null);");
    let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
}

#[test]
fn shallow_reactive_classified_as_reactive_kind() {
    let result =
        analyze("import { shallowReactive } from 'vue';\nconst state = shallowReactive({});");
    let b = result.bindings.iter().find(|b| b.name == "state").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Reactive);
}

#[test]
fn custom_ref_classified_as_ref_kind() {
    let result =
        analyze("import { customRef } from 'vue';\nconst val = customRef((track, trigger) => ({ get() { return 1 }, set() {} }));");
    let b = result.bindings.iter().find(|b| b.name == "val").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
}

#[test]
fn to_ref_classified_as_ref_kind() {
    let result = analyze("import { toRef } from 'vue';\nconst name = toRef(props, 'name');");
    let b = result.bindings.iter().find(|b| b.name == "name").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Ref);
}

#[test]
fn composable_use_prefix_classified_as_maybe_ref() {
    let result = analyze("const data = useFetch('/api');");
    let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::MaybeRef);
}

#[test]
fn let_binding_classified_as_mutable() {
    let result = analyze("let count = 0;");
    let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::Mutable);
}

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

#[test]
fn const_literal_classified_as_none() {
    let result = analyze("const MAX = 100;");
    let b = result.bindings.iter().find(|b| b.name == "MAX").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::None);
}

#[test]
fn function_decl_classified_as_none() {
    let result = analyze("function helper() { return 42; }");
    let b = result.bindings.iter().find(|b| b.name == "helper").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::None);
}

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

#[test]
fn non_vue_call_classified_as_none() {
    let result = analyze("const data = fetchData();");
    let b = result.bindings.iter().find(|b| b.name == "data").unwrap();
    assert_eq!(b.reactivity_kind, ReactivityKind::None);
}

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

#[test]
fn type_annotation_extracted() {
    let code = "const count: Ref<number> = ref(0);";
    let result = analyze(code);
    let b = result.bindings.iter().find(|b| b.name == "count").unwrap();
    assert_eq!(b.type_annotation.as_deref(), Some("Ref<number>"));
}

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
    // The top-level `return_reactivity` doesn't resolve local identifiers
    // (it uses a general heuristic without function-local binding context).
    // The composable `return_shape` DOES resolve identifiers via `internal_reactive_state`.
    assert!(
        matches!(
            f.return_reactivity,
            ReturnReactivity::Plain | ReturnReactivity::ObjectWithReactiveFields(_)
        ),
        "return_reactivity: {:?}",
        f.return_reactivity,
    );
    // Verify composable return shape has per-field reactivity
    let composable = f.composable.as_ref().expect("should be a composable");
    if let ComposableReturn::Object(fields) = &composable.return_shape {
        let count_field = fields
            .iter()
            .find(|f| f.name == "count")
            .expect("count field");
        assert_eq!(
            count_field.reactivity,
            ReactivityKind::Ref,
            "count should be Ref"
        );
        let doubled_field = fields
            .iter()
            .find(|f| f.name == "doubled")
            .expect("doubled field");
        assert_eq!(
            doubled_field.reactivity,
            ReactivityKind::Computed,
            "doubled should be Computed"
        );
    } else {
        panic!(
            "expected Object return shape, got: {:?}",
            composable.return_shape
        );
    }
}

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

// --- CSS Variable Manipulation Detection ---

#[test]
fn css_var_set_property_detected() {
    let result = analyze("el.style.setProperty('--color', val);");
    assert_eq!(result.css_var_manipulations.len(), 1);
    let m = &result.css_var_manipulations[0];
    assert_eq!(m.kind, CssVarManipulationKind::SetProperty);
    assert_eq!(m.var_name, "--color");
    assert_eq!(m.value_expr.as_deref(), Some("val"));
    // No non-CSS-var setProperty calls
    let result2 = analyze("el.style.setProperty('color', 'red');");
    assert!(result2.css_var_manipulations.is_empty());
}

#[test]
fn css_var_get_property_value_detected() {
    let result = analyze("getComputedStyle(el).getPropertyValue('--theme-bg');");
    assert_eq!(result.css_var_manipulations.len(), 1);
    let m = &result.css_var_manipulations[0];
    assert_eq!(m.kind, CssVarManipulationKind::GetPropertyValue);
    assert_eq!(m.var_name, "--theme-bg");
    assert!(m.value_expr.is_none());
}

#[test]
fn css_var_remove_property_detected() {
    let result = analyze("el.style.removeProperty('--size');");
    assert_eq!(result.css_var_manipulations.len(), 1);
    let m = &result.css_var_manipulations[0];
    assert_eq!(m.kind, CssVarManipulationKind::RemoveProperty);
    assert_eq!(m.var_name, "--size");
    assert!(m.value_expr.is_none());
}

#[test]
fn css_var_manipulation_ignores_non_var_names() {
    // Only string literals starting with "--" are tracked
    let result = analyze("el.style.setProperty('color', 'red');");
    assert!(result.css_var_manipulations.is_empty());
    let result2 = analyze("el.style.removeProperty('margin');");
    assert!(result2.css_var_manipulations.is_empty());
}

#[test]
fn css_var_manipulation_ignores_non_string_args() {
    let result = analyze("el.style.setProperty(varName, val);");
    assert!(result.css_var_manipulations.is_empty());
}

#[test]
fn css_var_manipulation_in_variable_init() {
    let result = analyze("const val = getComputedStyle(el).getPropertyValue('--offset');");
    assert_eq!(result.css_var_manipulations.len(), 1);
    assert_eq!(result.css_var_manipulations[0].var_name, "--offset");
}

#[test]
fn css_var_set_property_value_expr_is_source_text() {
    let result = analyze("el.style.setProperty('--x', computedColor.value);");
    assert_eq!(result.css_var_manipulations.len(), 1);
    assert_eq!(
        result.css_var_manipulations[0].value_expr.as_deref(),
        Some("computedColor.value")
    );
}

// ── ReactivityKind classification tests ──

#[test]
fn torefs_gets_ref_reactivity_kind() {
    let result = analyze("import { toRefs } from 'vue';\nconst { x, y } = toRefs(props);");
    let x_binding = result.bindings.iter().find(|b| b.name == "x").unwrap();
    assert_eq!(
        x_binding.reactivity_kind,
        ReactivityKind::Ref,
        "toRefs() destructured fields should be Ref"
    );
    assert!(x_binding.is_reactive, "toRefs() binding should be reactive");
}

#[test]
fn readonly_gets_reactive_kind() {
    let result = analyze("import { readonly, reactive } from 'vue';\nconst state = readonly(reactive({ count: 0 }));");
    let binding = result.bindings.iter().find(|b| b.name == "state").unwrap();
    assert_eq!(
        binding.reactivity_kind,
        ReactivityKind::Reactive,
        "readonly() should classify as Reactive"
    );
    assert!(binding.is_reactive, "readonly() binding should be reactive");
}

#[test]
fn shallow_readonly_gets_reactive_kind() {
    let result = analyze(
        "import { shallowReadonly } from 'vue';\nconst state = shallowReadonly({ count: 0 });",
    );
    let binding = result.bindings.iter().find(|b| b.name == "state").unwrap();
    assert_eq!(
        binding.reactivity_kind,
        ReactivityKind::Reactive,
        "shallowReadonly() should classify as Reactive"
    );
}

#[test]
fn define_model_gets_ref_kind() {
    let result = analyze("const modelValue = defineModel();");
    let binding = result
        .bindings
        .iter()
        .find(|b| b.name == "modelValue")
        .unwrap();
    assert_eq!(
        binding.reactivity_kind,
        ReactivityKind::Ref,
        "defineModel() returns a Ref-like ModelRef, should classify as Ref"
    );
}

#[test]
fn watch_callback_params_extracted() {
    let result = analyze("import { ref, watch } from 'vue';\nconst x = ref(0);\nwatch(x, (val, old) => { console.log(val, old) });");
    let watch_call = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::Watch)
        .expect("should have a watch call");
    assert_eq!(
        watch_call.callback_params.len(),
        2,
        "watch callback should have 2 params"
    );
    assert_eq!(watch_call.callback_params[0].name, "val");
    assert_eq!(watch_call.callback_params[1].name, "old");
    // Spans should be valid (non-zero)
    assert!(
        watch_call.callback_params[0].span.start > 0,
        "val span should have valid start"
    );
}

#[test]
fn watch_callback_typed_params_skipped() {
    let result = analyze(
        "import { ref, watch } from 'vue';\nconst x = ref(0);\nwatch(x, (val: number) => { });",
    );
    let watch_call = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::Watch)
        .expect("should have a watch call");
    assert!(
        watch_call.callback_params.is_empty(),
        "typed params should NOT be extracted for inlay hints"
    );
}

#[test]
fn lifecycle_callback_no_params() {
    let result = analyze("import { onMounted } from 'vue';\nonMounted(() => { });");
    let hook = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::OnMounted)
        .expect("should have onMounted call");
    assert!(
        hook.callback_params.is_empty(),
        "onMounted with no-param arrow should have empty callback_params"
    );
}

#[test]
fn plain_const_stays_none() {
    let result = analyze("const x = 42;");
    let binding = result.bindings.iter().find(|b| b.name == "x").unwrap();
    assert_eq!(
        binding.reactivity_kind,
        ReactivityKind::None,
        "plain const literal should stay None"
    );
    assert!(!binding.is_reactive, "plain const should not be reactive");
}

// ── Vue API calls from variable declarations ──

#[test]
fn use_template_ref_in_var_decl_detected() {
    let result =
        analyze("import { useTemplateRef } from 'vue';\nconst el = useTemplateRef('myRef');");
    assert!(
        !result.vue_api_calls.is_empty(),
        "useTemplateRef in variable declaration should be detected"
    );
    let call = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::UseTemplateRef);
    assert!(call.is_some(), "should find UseTemplateRef api call");
    assert_eq!(
        call.unwrap().arg_value.as_deref(),
        Some("myRef"),
        "arg_value should be 'myRef'"
    );
}

#[test]
fn provide_in_var_decl_detected() {
    let result = analyze("import { provide } from 'vue';\nconst p = provide('myKey', 42);");
    let call = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::Provide);
    assert!(
        call.is_some(),
        "provide in variable declaration should be detected"
    );
    assert_eq!(call.unwrap().arg_value.as_deref(), Some("myKey"));
}

#[test]
fn computed_in_var_decl_detected() {
    let result = analyze("import { computed } from 'vue';\nconst x = computed(() => 1);");
    let call = result
        .vue_api_calls
        .iter()
        .find(|c| c.api == VueApiClassification::Computed);
    assert!(
        call.is_some(),
        "computed in variable declaration should be detected"
    );
}

// ═══════════════════════════════════════════════════════════
// Module reference collector: chain expression tests
// ═══════════════════════════════════════════════════════════

#[test]
fn chain_expression_optional_call_with_import() {
    let result = analyze("foo?.bar?.(import('./lazy.vue'));");
    assert!(
        result
            .module_references
            .iter()
            .any(|r| r.literal_specifier.as_deref() == Some("./lazy.vue")
                && r.syntax == ModuleReferenceSyntax::DynamicImport),
        "should find dynamic import inside optional chained call, got: {:?}",
        result.module_references
    );
    assert!(
        !result.module_references.is_empty(),
        "should have at least one module reference"
    );
}

#[test]
fn chain_expression_deeply_nested_require() {
    let result = analyze("a?.b?.c?.(require('./deep'));");
    assert!(
        result
            .module_references
            .iter()
            .any(|r| r.literal_specifier.as_deref() == Some("./deep")
                && r.syntax == ModuleReferenceSyntax::RequireCall),
        "should find require inside deeply nested optional chain, got: {:?}",
        result.module_references
    );
}

#[test]
fn chain_expression_computed_member_with_import() {
    let result = analyze("obj?.[key]?.(import('./computed-target'));");
    assert!(
        result
            .module_references
            .iter()
            .any(|r| r.literal_specifier.as_deref() == Some("./computed-target")),
        "should find import inside chain with computed member access"
    );
}

// ═══════════════════════════════════════════════════════════
// Script binding usage spans
// ═══════════════════════════════════════════════════════════

fn analyze_with_usages(code: &str) -> ScriptAnalysisSnapshot {
    let alloc = Allocator::new();
    build_script_analysis_with_scope(code, SourceType::ts(), &alloc, AnalysisScope::all())
}

#[test]
fn script_usage_basic_read() {
    let result = analyze_with_usages("const x = 1;\nconsole.log(x);");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "x")
        .collect();
    assert_eq!(usages.len(), 1, "should have one usage of x");
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::Read);
}

#[test]
fn script_usage_write() {
    let result = analyze_with_usages("let x = 1;\nx = 2;");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "x")
        .collect();
    assert_eq!(usages.len(), 1, "should have one write usage of x");
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::Write);
}

#[test]
fn script_usage_read_write() {
    let result = analyze_with_usages("let x = 1;\nx += 2;");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "x")
        .collect();
    assert_eq!(usages.len(), 1, "should have one read-write usage of x");
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::ReadWrite);
}

#[test]
fn script_usage_call() {
    let result = analyze_with_usages("function foo() {}\nfoo();");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "foo")
        .collect();
    assert_eq!(usages.len(), 1, "should have one call usage of foo");
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::Call);
}

#[test]
fn script_usage_member_access() {
    let result = analyze_with_usages("const obj = {};\nobj.x;");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "obj")
        .collect();
    assert_eq!(
        usages.len(),
        1,
        "should have one member access usage of obj"
    );
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::MemberAccess);
}

#[test]
fn script_usage_shadowing_does_not_track_inner() {
    let result =
        analyze_with_usages("const x = 1;\n{ const x = 2; console.log(x); }\nconsole.log(x);");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "x")
        .collect();
    // Only the outer x reference should be tracked (the last console.log(x))
    // The inner x is a different binding
    assert_eq!(
        usages.len(),
        1,
        "should track only outer x, not shadowed inner x"
    );
}

#[test]
fn script_usage_type_annotation_not_tracked() {
    let result = analyze_with_usages("interface Foo {}\nconst x: Foo = {};");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "Foo")
        .collect();
    assert!(
        usages.is_empty(),
        "type annotations should not create binding occurrences"
    );
}

#[test]
fn script_usage_string_literal_not_tracked() {
    let result = analyze_with_usages("const x = 1;\nconst y = \"x\";");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "x")
        .collect();
    assert!(
        usages.is_empty(),
        "string literal 'x' should not be a binding occurrence"
    );
}

#[test]
fn script_usage_destructure_source() {
    let result = analyze_with_usages("const obj = { a: 1 };\nconst { a } = obj;");
    let usages: Vec<_> = result
        .script_binding_occurrences
        .iter()
        .filter(|o| o.name == "obj")
        .collect();
    assert_eq!(
        usages.len(),
        1,
        "should have one usage of obj as destructure source"
    );
    assert_eq!(usages[0].usage_kind, ScriptUsageKind::Read);
}

#[test]
fn script_usage_not_collected_without_scope_flag() {
    let alloc = Allocator::new();
    let scope = AnalysisScope::BUILD; // BUILD doesn't include SCRIPT_USAGES
    let result = build_script_analysis_with_scope(
        "const x = 1;\nconsole.log(x);",
        SourceType::ts(),
        &alloc,
        scope,
    );
    assert!(
        result.script_binding_occurrences.is_empty(),
        "BUILD scope should not collect script usages"
    );
}

// ── Nested macro call detection ──

#[test]
fn nested_define_props_in_function() {
    let result = analyze("function setup() { const props = defineProps<{ msg: string }>() }");
    assert_eq!(result.nested_macro_calls.len(), 1);
    assert_eq!(result.nested_macro_calls[0].name, "defineProps");
    // Top-level macros should not be detected
    assert!(
        result.macros.is_empty(),
        "nested macros are not extracted as top-level"
    );
}

#[test]
fn nested_define_emits_in_arrow() {
    let result = analyze("const fn = () => { const emit = defineEmits(['click']) }");
    assert_eq!(result.nested_macro_calls.len(), 1);
    assert_eq!(result.nested_macro_calls[0].name, "defineEmits");
}

#[test]
fn nested_define_props_in_if() {
    let result = analyze("if (true) { defineProps() }");
    assert_eq!(result.nested_macro_calls.len(), 1);
    assert_eq!(result.nested_macro_calls[0].name, "defineProps");
}

#[test]
fn nested_define_props_in_try_catch() {
    let result = analyze("try { defineProps() } catch(e) { defineEmits() }");
    assert_eq!(result.nested_macro_calls.len(), 2);
    assert_eq!(result.nested_macro_calls[0].name, "defineProps");
    assert_eq!(result.nested_macro_calls[1].name, "defineEmits");
}

#[test]
fn top_level_macros_not_detected_as_nested() {
    let result = analyze(
        "const props = defineProps<{ msg: string }>()\nconst emit = defineEmits(['click'])",
    );
    assert!(
        result.nested_macro_calls.is_empty(),
        "top-level macros should NOT appear in nested_macro_calls"
    );
    assert_eq!(
        result.macros.len(),
        2,
        "top-level macros should be in macros vec"
    );
}

#[test]
fn nested_with_defaults_in_function() {
    let result =
        analyze("function setup() { withDefaults(defineProps<{ msg: string }>(), { msg: 'hi' }) }");
    // withDefaults + defineProps both inside a function
    assert!(result.nested_macro_calls.len() >= 2);
    let names: Vec<&str> = result
        .nested_macro_calls
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert!(names.contains(&"withDefaults"));
    assert!(names.contains(&"defineProps"));
}

// =============================================================================
// Store detection tests
// =============================================================================

/// @ai-generated - Pinia useXxxStore usage detected
#[test]
fn pinia_store_usage_detected() {
    let result = analyze(
        r#"
import { useUserStore } from '@/stores/user';
const store = useUserStore();
"#,
    );
    assert!(!result.store_usages.is_empty(), "should detect store usage");
    assert_eq!(result.store_usages[0].callee, "useUserStore");
    assert_eq!(result.store_usages[0].import_source, "@/stores/user");
    assert_eq!(
        result.store_usages[0].store_api,
        StoreApiClassification::StoreComposable
    );
    assert!(result.flags.contains(AnalysisFlags::HAS_STORE_USAGE));
}

/// @ai-generated - Pinia storeToRefs detected
#[test]
fn pinia_store_to_refs_detected() {
    let result = analyze(
        r#"
import { storeToRefs } from 'pinia';
import { useUserStore } from '@/stores/user';
const store = useUserStore();
const { name, email } = storeToRefs(store);
"#,
    );
    assert!(!result.store_usages.is_empty(), "should detect store usage");
    // The useUserStore call should be detected
    let store_usage = result
        .store_usages
        .iter()
        .find(|u| u.callee == "useUserStore")
        .expect("should find useUserStore usage");
    assert_eq!(store_usage.import_source, "@/stores/user");
    assert!(result.flags.contains(AnalysisFlags::HAS_STORE_USAGE));
}

/// @ai-generated - Pinia defineStore detected as store definition
#[test]
fn pinia_define_store_detected() {
    let result = analyze(
        r#"
import { defineStore } from 'pinia';
export const useUserStore = defineStore('user', {
    state: () => ({ name: '', email: '' }),
    getters: { fullName: (state) => state.name },
    actions: { setName(n: string) { this.name = n; } },
});
"#,
    );
    assert!(
        !result.store_definitions.is_empty(),
        "should detect store definition"
    );
    let def = &result.store_definitions[0];
    assert_eq!(def.store_id.as_deref(), Some("user"));
    assert_eq!(def.export_name, "useUserStore");
    assert_eq!(def.store_api, StoreApiClassification::PiniaDefineStore);
    assert!(result.flags.contains(AnalysisFlags::HAS_STORE_DEFINITION));
}

/// @ai-generated - Vuex createStore detected
#[test]
fn vuex_create_store_detected() {
    let result = analyze(
        r#"
import { createStore } from 'vuex';
export const store = createStore({
    state: { count: 0 },
});
"#,
    );
    assert!(
        !result.store_definitions.is_empty(),
        "should detect Vuex store definition"
    );
    let def = &result.store_definitions[0];
    assert_eq!(def.store_api, StoreApiClassification::VuexCreateStore);
    assert_eq!(def.export_name, "store");
    assert!(result.flags.contains(AnalysisFlags::HAS_STORE_DEFINITION));
}

/// @ai-generated - Vuex useStore detected
#[test]
fn vuex_use_store_detected() {
    let result = analyze(
        r#"
import { useStore } from 'vuex';
const store = useStore();
"#,
    );
    assert!(
        !result.store_usages.is_empty(),
        "should detect Vuex useStore"
    );
    assert_eq!(result.store_usages[0].callee, "useStore");
    assert_eq!(
        result.store_usages[0].store_api,
        StoreApiClassification::VuexUseStore
    );
    assert!(result.flags.contains(AnalysisFlags::HAS_STORE_USAGE));
}

/// @ai-generated - Destructured store usage detected
#[test]
fn destructured_store_usage_detected() {
    let result = analyze(
        r#"
import { useUserStore } from '@/stores/user';
const { name, email } = useUserStore();
"#,
    );
    assert!(!result.store_usages.is_empty(), "should detect store usage");
    let usage = &result.store_usages[0];
    assert_eq!(usage.callee, "useUserStore");
    assert!(
        usage.destructured_without_store_to_refs,
        "destructured without storeToRefs should be flagged"
    );
    assert!(
        usage.destructured_props.contains(&"name".to_string()),
        "should track destructured props"
    );
    assert!(
        usage.destructured_props.contains(&"email".to_string()),
        "should track destructured props"
    );
}

/// @ai-generated - Regular composables are NOT detected as store usage
#[test]
fn regular_composables_not_detected_as_store() {
    let result = analyze(
        r#"
import { useRouter } from 'vue-router';
import { useMouse } from '@vueuse/core';
const router = useRouter();
const { x, y } = useMouse();
"#,
    );
    assert!(
        result.store_usages.is_empty(),
        "non-store composables should not be detected"
    );
    assert!(!result.flags.contains(AnalysisFlags::HAS_STORE_USAGE));
    assert!(!result.flags.contains(AnalysisFlags::HAS_STORE_DEFINITION));
}

/// @ai-generated - Pinia mapState detected as store usage
#[test]
fn pinia_map_helpers_detected() {
    let result = analyze(
        r#"
import { mapState, mapActions } from 'pinia';
import { useUserStore } from '@/stores/user';
const mapped = mapState(useUserStore, ['name', 'email']);
const actions = mapActions(useUserStore, ['setName']);
"#,
    );
    // mapState and mapActions from pinia should be detected
    let map_state = result.store_usages.iter().find(|u| u.callee == "mapState");
    assert!(map_state.is_some(), "mapState should be detected");
    assert_eq!(
        map_state.unwrap().store_api,
        StoreApiClassification::PiniaMapState
    );

    let map_actions = result
        .store_usages
        .iter()
        .find(|u| u.callee == "mapActions");
    assert!(map_actions.is_some(), "mapActions should be detected");
    assert_eq!(
        map_actions.unwrap().store_api,
        StoreApiClassification::PiniaMapActions
    );
}

// =============================================================================
// used_in_style — CSS v-bind() binding marking
// =============================================================================

#[test]
fn mark_bindings_used_in_style_simple() {
    let mut result =
        analyze("import { ref } from 'vue';\nconst color = ref('red');\nconst size = ref(12);");
    // Simulate style analysis with v-bind(color)
    let style_analyses = vec![crate::style::StyleBlockAnalysis {
        v_binds: vec![crate::style::AnalyzedVBind {
            expression: "color".into(),
            quoted: false,
            start: 0,
            end: 5,
            generated_var_name: None,
        }],
        ..Default::default()
    }];
    result.mark_bindings_used_in_style(&style_analyses);

    let color = result.bindings.iter().find(|b| b.name == "color").unwrap();
    assert!(color.used_in_style, "color should be marked used_in_style");

    let size = result.bindings.iter().find(|b| b.name == "size").unwrap();
    assert!(
        !size.used_in_style,
        "size should NOT be marked used_in_style"
    );
}

#[test]
fn mark_bindings_used_in_style_member_expression() {
    let mut result = analyze("const theme = reactive({ color: 'red' });");
    let style_analyses = vec![crate::style::StyleBlockAnalysis {
        v_binds: vec![crate::style::AnalyzedVBind {
            expression: "theme.color".into(),
            quoted: false,
            start: 0,
            end: 11,
            generated_var_name: None,
        }],
        ..Default::default()
    }];
    result.mark_bindings_used_in_style(&style_analyses);

    let theme = result.bindings.iter().find(|b| b.name == "theme").unwrap();
    assert!(
        theme.used_in_style,
        "theme should be marked used_in_style (root of theme.color)"
    );
}

#[test]
fn mark_bindings_used_in_style_multiple_blocks() {
    let mut result = analyze("const a = 1;\nconst b = 2;\nconst c = 3;");
    let style_analyses = vec![
        crate::style::StyleBlockAnalysis {
            v_binds: vec![crate::style::AnalyzedVBind {
                expression: "a".into(),
                quoted: false,
                start: 0,
                end: 1,
                generated_var_name: None,
            }],
            ..Default::default()
        },
        crate::style::StyleBlockAnalysis {
            v_binds: vec![crate::style::AnalyzedVBind {
                expression: "c".into(),
                quoted: false,
                start: 0,
                end: 1,
                generated_var_name: None,
            }],
            ..Default::default()
        },
    ];
    result.mark_bindings_used_in_style(&style_analyses);

    let a = result.bindings.iter().find(|b| b.name == "a").unwrap();
    assert!(a.used_in_style, "a should be marked used_in_style");

    let b = result.bindings.iter().find(|b| b.name == "b").unwrap();
    assert!(!b.used_in_style, "b should NOT be marked used_in_style");

    let c = result.bindings.iter().find(|b| b.name == "c").unwrap();
    assert!(c.used_in_style, "c should be marked used_in_style");
}

#[test]
fn mark_bindings_used_in_style_no_v_binds() {
    let mut result = analyze("const color = 'red';");
    let style_analyses = vec![crate::style::StyleBlockAnalysis::default()];
    result.mark_bindings_used_in_style(&style_analyses);

    let color = result.bindings.iter().find(|b| b.name == "color").unwrap();
    assert!(
        !color.used_in_style,
        "color should NOT be marked when no v-bind references it"
    );
}

#[test]
fn mark_bindings_used_in_style_quoted_expression() {
    let mut result = analyze("const color = 'red';");
    let style_analyses = vec![crate::style::StyleBlockAnalysis {
        v_binds: vec![crate::style::AnalyzedVBind {
            expression: "color".into(),
            quoted: true,
            start: 0,
            end: 5,
            generated_var_name: None,
        }],
        ..Default::default()
    }];
    result.mark_bindings_used_in_style(&style_analyses);

    let color = result.bindings.iter().find(|b| b.name == "color").unwrap();
    assert!(
        color.used_in_style,
        "quoted v-bind(color) should still mark the binding"
    );
}

#[test]
fn define_model_with_default_extracts_default_keys() {
    let code = r#"const checked = defineModel<boolean>('checked', { default: false });"#;
    let result = analyze(code);
    let model_macro = result
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineModel)
        .expect("should have DefineModel macro");
    assert!(
        model_macro.default_keys.contains(&"checked".to_string()),
        "defineModel with {{ default: ... }} should populate default_keys with the model name, got: {:?}",
        model_macro.default_keys
    );
}

#[test]
fn define_model_without_default_has_empty_default_keys() {
    let code = r#"const model = defineModel<string>({ required: true });"#;
    let result = analyze(code);
    let model_macro = result
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineModel)
        .expect("should have DefineModel macro");
    assert!(
        model_macro.default_keys.is_empty(),
        "defineModel without default should have empty default_keys, got: {:?}",
        model_macro.default_keys
    );
}

#[test]
fn define_model_default_modelvalue() {
    let code = r#"const model = defineModel<string>({ default: '' });"#;
    let result = analyze(code);
    let model_macro = result
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineModel)
        .expect("should have DefineModel macro");
    assert!(
        model_macro.default_keys.contains(&"modelValue".to_string()),
        "defineModel without name should use 'modelValue' as default key, got: {:?}",
        model_macro.default_keys
    );
}
