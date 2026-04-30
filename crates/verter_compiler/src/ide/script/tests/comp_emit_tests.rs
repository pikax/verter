//! Comp-function emission tests (D9 cohort).

use super::*;

// ── Comp function tests ──────────────────────────────────────

#[test]
fn comp_function_html_element() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">hello</div></template>"#,
    );
    assert!(
        code.contains("{} as HTMLElementTagNameMap[\"div\"]"),
        "should emit Comp for div with ref returning plain element type: {}",
        code
    );
    assert!(
        !code.contains("enhanceElementWithProps({} as HTMLElementTagNameMap"),
        "should NOT use enhanceElementWithProps for HTML elements: {}",
        code
    );
}

#[test]
fn comp_function_html_element_plain_type() {
    // Bug: useTemplateRef on HTML elements should resolve to plain HTMLSpanElement,
    // not `HTMLSpanElement & { onClick: () => void }`
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const el = useTemplateRef('el')
</script>
<template><span ref="el" @click="() => {}">hello</span></template>"#,
    );

    // Positive: should return just the HTMLElementTagNameMap type
    assert!(
        code.contains("HTMLElementTagNameMap[\"span\"]"),
        "should reference HTMLElementTagNameMap for span: {}",
        code
    );

    // Negative: must NOT use enhanceElementWithProps for HTML elements
    // (only components need props enhancement)
    assert!(
        !code.contains("enhanceElementWithProps({} as HTMLElementTagNameMap"),
        "should NOT use enhanceElementWithProps for HTML elements: {}",
        code
    );
}

#[test]
fn comp_function_component() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const el = ref()
</script>
<template><MyComp ref="el" /></template>"#,
    );
    assert!(
        code.contains("instantiateComponent(MyComp, {})"),
        "should emit instantiateComponent(MyComp) for component with ref in code: {}",
        code
    );
}

#[test]
fn comp_function_generic() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
const msg = {} as T
</script>
<template><div ref="el">{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("function ___VERTER___Comp") && code.contains("<T>()"),
        "Comp function should have generics in code: {}",
        code
    );
}

// ── getRootComponent tests ───────────────────────────────────

#[test]
fn get_root_component_with_template() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">hello</div></template>"#,
    );
    assert!(
        code.contains("function ___VERTER___getRootComponent()")
            && code.contains("return ___VERTER___Comp"),
        "getRootComponent should delegate to Comp in code: {}",
        code
    );
}

#[test]
fn get_root_component_generic() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T extends string">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
const msg = {} as T
</script>
<template><div ref="el">{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("function ___VERTER___getRootComponent<T extends string>()"),
        "getRootComponent should have generics in code: {}",
        code
    );
}

#[test]
fn get_root_component_no_template() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    // No template: getRootComponent is not emitted (nothing to wrap)
    assert!(
        !code.contains("___VERTER___getRootComponent"),
        "getRootComponent should NOT be emitted when no template: {}",
        code
    );
}

// ── Root element attrs fallthrough tests ──────────────────────

#[test]
fn root_attrs_single_native_element() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const msg = 'hi'
</script>
<template><div :title="msg" id="app">hi</div></template>"#,
    );
    // Positive: getRootComponent delegates to a Comp function
    assert!(
        code.contains("getRootComponent()") && code.contains("return ___VERTER___Comp"),
        "getRootComponent should delegate to Comp: {}",
        code
    );
    // Positive: getRootComponentPassedProps has the static and bound props
    assert!(
        code.contains(r#""id": "app""#),
        "passed props should contain id: {}",
        code
    );
    assert!(
        code.contains(r#""title": msg"#),
        "passed props should contain title: {}",
        code
    );
    // Positive: Attrs includes RootElementProps
    assert!(
        code.contains("___VERTER___Attrs") && code.contains("___VERTER___RootElementProps"),
        "Attrs should include RootElementProps: {}",
        code
    );
}

#[test]
fn root_attrs_native_excludes_class_style() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const x = ''
const y = {}
</script>
<template><div :class="x" :style="y" id="app">hi</div></template>"#,
    );
    // Positive: id is in passed props
    assert!(
        code.contains(r#""id": "app""#),
        "passed props should contain id: {}",
        code
    );
    // Negative: class and style are excluded
    let passed_props_section = code
        .split("getRootComponentPassedProps")
        .nth(1)
        .unwrap_or("");
    assert!(
        !passed_props_section.contains(r#""class""#),
        "class should NOT be in passed props: {}",
        code
    );
    assert!(
        !passed_props_section.contains(r#""style""#),
        "style should NOT be in passed props: {}",
        code
    );
}

#[test]
fn root_attrs_single_component_root() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template><MyComp :foo="42" bar="static"/></template>"#,
    );
    // Positive: Comp function instantiates MyComp
    assert!(
        code.contains("instantiateComponent(MyComp,"),
        "should instantiate MyComp: {}",
        code
    );
    // Positive: getRootComponent delegates to Comp
    assert!(
        code.contains("getRootComponent()") && code.contains("return ___VERTER___Comp"),
        "getRootComponent should delegate: {}",
        code
    );
    // Positive: passed props include foo and bar
    assert!(
        code.contains(r#""foo": 42"#),
        "passed props should contain foo: {}",
        code
    );
    assert!(
        code.contains(r#""bar": "static""#),
        "passed props should contain bar: {}",
        code
    );
    // Negative: getRootComponent does NOT return {}
    let root_fn = code
        .split("getRootComponent()")
        .nth(1)
        .unwrap_or("")
        .split('}')
        .next()
        .unwrap_or("");
    assert!(
        !root_fn.contains("return {};"),
        "getRootComponent should NOT return empty: {}",
        code
    );
}

#[test]
fn root_attrs_multi_root_fragment() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const a = 1
</script>
<template><div>first</div><span>second</span></template>"#,
    );
    // Positive: both functions return {}
    assert!(
        code.contains("getRootComponent() { return {};"),
        "getRootComponent should return empty: {}",
        code
    );
    assert!(
        code.contains("getRootComponentPassedProps() { return {};"),
        "getRootComponentPassedProps should return empty: {}",
        code
    );
}

#[test]
fn root_attrs_inherit_attrs_false() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>
<template><div id="app">hello</div></template>"#,
    );
    // Positive: Attrs type should NOT include RootElementProps
    let attrs_line = code
        .lines()
        .find(|l| l.contains("type ___VERTER___Attrs"))
        .unwrap_or("");
    assert!(
        !attrs_line.contains("RootElementProps"),
        "Attrs should NOT include RootElementProps when inheritAttrs: false: attrs_line={}, full={}",
        attrs_line,
        code
    );
    // Positive: Attrs = attributes only
    assert!(
        attrs_line.contains("___VERTER___attributes"),
        "Attrs should include attributes: {}",
        attrs_line
    );
}

#[test]
fn root_attrs_inherit_attrs_true_default() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>hello</div></template>"#,
    );
    // Positive: Attrs includes RootElementProps
    let attrs_line = code
        .lines()
        .find(|l| l.contains("type ___VERTER___Attrs"))
        .unwrap_or("");
    assert!(
        attrs_line.contains("___VERTER___RootElementProps"),
        "Attrs should include RootElementProps by default: {}",
        attrs_line
    );
}

#[test]
fn root_attrs_v_if_v_else_single_root() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const show = true
</script>
<template><div v-if="show">A</div><span v-else>B</span></template>"#,
    );
    // Positive: getRootComponent should contain both Comp branches (union)
    let root_fn_body = code
        .split("getRootComponent()")
        .nth(1)
        .unwrap_or("")
        .split("getRootComponentPassedProps")
        .next()
        .unwrap_or("");
    // Both Comp offsets should appear — the div and the span
    let comp_count = root_fn_body.matches("___VERTER___Comp").count();
    assert!(
        comp_count == 2,
        "getRootComponent should union both branches (found {} Comp refs): {}",
        comp_count,
        code
    );
    // Negative: should NOT return {}
    assert!(
        !root_fn_body.contains("return {};"),
        "getRootComponent should NOT return empty for v-if/v-else: {}",
        code
    );
    // Positive: Math.random() pattern used for union branching
    assert!(
        root_fn_body.contains("Math.random()"),
        "union branches should use Math.random() pattern: {}",
        code
    );
}

#[test]
fn root_attrs_v_if_elseif_else_triple_union() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const mode = 'a'
</script>
<template><div v-if="mode === 'a'">A</div><span v-else-if="mode === 'b'">B</span><p v-else>C</p></template>"#,
    );
    // Positive: getRootComponent should contain all 3 Comp branches
    let root_fn_body = code
        .split("getRootComponent()")
        .nth(1)
        .unwrap_or("")
        .split("getRootComponentPassedProps")
        .next()
        .unwrap_or("");
    let comp_count = root_fn_body.matches("___VERTER___Comp").count();
    assert!(
        comp_count == 3,
        "getRootComponent should union all 3 branches (found {} Comp refs): {}",
        comp_count,
        code
    );
    // Negative: should NOT return {}
    assert!(
        !root_fn_body.contains("return {};"),
        "getRootComponent should NOT return empty for triple conditional: {}",
        code
    );
    // Positive: also check getRootComponentPassedProps has 3 branches
    let props_fn_body = code
        .split("getRootComponentPassedProps()")
        .nth(1)
        .unwrap_or("")
        .split("type ___VERTER___RootElement")
        .next()
        .unwrap_or("");
    let props_return_count = props_fn_body.matches("return").count();
    assert!(
        props_return_count == 3,
        "getRootComponentPassedProps should have 3 return branches (found {}): {}",
        props_return_count,
        code
    );
}

#[test]
fn root_attrs_nested_no_leak() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const x = ''
</script>
<template><div><span :title="x">inner</span></div></template>"#,
    );
    // Positive: getRootComponentPassedProps returns {} (div has no props)
    assert!(
        code.contains("getRootComponentPassedProps() { return {};"),
        "root div has no props so passed props should be empty: {}",
        code
    );
    // Negative: title should NOT leak to root
    let passed_section = code
        .split("getRootComponentPassedProps")
        .nth(1)
        .unwrap_or("")
        .split('}')
        .next()
        .unwrap_or("");
    assert!(
        !passed_section.contains("title"),
        "inner span's title should NOT leak to root: {}",
        code
    );
}

#[test]
fn root_attrs_event_handler_camelized() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
</script>
<template><div @my-event="() => {}">hello</div></template>"#,
    );
    // Kebab events now preserve hyphens: onMy-event (not camelized onMyEvent)
    assert!(
        code.contains(r#""onMy-event""#),
        "event handler should preserve hyphens as onMy-event: {}",
        code
    );
    // Negative: camelized form should NOT appear
    assert!(
        !code.contains(r#""onMyEvent""#),
        "camelized event name should NOT appear: {}",
        code
    );
}

#[test]
fn root_attrs_functional_component_uses_instantiate() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template><MyComp :label="'hello'"/></template>"#,
    );
    // Positive: uses instantiateComponent, not new
    assert!(
        code.contains("instantiateComponent(MyComp,"),
        "should use instantiateComponent for components: {}",
        code
    );
    // Negative: should NOT use new
    assert!(
        !code.contains("new MyComp("),
        "should NOT use new for component instantiation: {}",
        code
    );
}

#[test]
fn root_attrs_dynamic_component_prop_binding_prefixed() {
    // When a prop is used in `:is="propName"`, the generated Comp function
    // and getRootComponentPassedProps must emit `__props.propName` (not bare
    // `propName`) because props are not destructured at script scope.
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
defineProps<{ tag?: string }>()
</script>
<template><component :is="tag"><slot /></component></template>"#,
    );
    // Positive: props literal should reference __props.tag, not bare tag
    let _passed_section = code
        .split("getRootComponentPassedProps")
        .nth(1)
        .unwrap_or("")
        .split('}')
        .nth(1) // skip the first } which closes the return object
        .unwrap_or("");
    assert!(
        code.contains(r#""is": __props.tag"#),
        "prop reference in :is should be prefixed with __props.: {}",
        code
    );
    // Negative: bare `tag` (without __props.) should NOT appear in props literal
    // (except as a key name in quotes)
    let passed_body = code
        .split("getRootComponentPassedProps")
        .nth(1)
        .unwrap_or("")
        .split('}')
        .next()
        .unwrap_or("");
    assert!(
        !passed_body.contains(": tag}") && !passed_body.contains(": tag,"),
        "bare prop name should NOT appear as value in passed props: {}",
        code
    );
}

#[test]
fn root_vfor_is_treated_as_fragment_for_root_attrs_helpers() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
interface Action { label: string; disabled: boolean }
const actions: Action[] = [{ label: 'ok', disabled: false }]
</script>
<template>
  <button v-for="action in actions" :key="action.label" :disabled="action.disabled">
{{ action.label }}
  </button>
</template>"#,
    );

    assert!(
        code.contains("getRootComponent() { return {};"),
        "root v-for should not synthesize a single root component helper: {code}"
    );
    assert!(
        code.contains("getRootComponentPassedProps() { return {};"),
        "root v-for should not synthesize passed props from loop-local bindings: {code}"
    );

    let passed_props_section = code
        .split("getRootComponentPassedProps")
        .nth(1)
        .unwrap_or("")
        .split('}')
        .next()
        .unwrap_or("");
    assert!(
        !passed_props_section.contains("action.disabled"),
        "loop-local bindings must not leak into root passed props helper: {code}"
    );
}

// ── Script Error Recovery Tests ─────────────────────────────────

#[test]
fn imports_hoisted_with_script_syntax_error() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
    );
    // With partial AST recovery, clean prefix is parsed normally.
    // Import is hoisted, binding extracted, TemplateBindingFN wraps the body.
    assert!(
        code.contains("import { ref } from 'vue'"),
        "import must be present:\n{}",
        code
    );
    // The broken `count.` line must still appear (passthrough in CodeTransform)
    assert!(
        code.contains("count."),
        "broken expression must be preserved:\n{}",
        code
    );
    // With recovery: count binding should be extracted from clean prefix
    assert!(
        bindings.contains_key("count"),
        "count binding should be extracted: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
}

#[test]
fn imports_hoisted_with_broken_expression_in_function() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
  count.
}
</script>"#,
    );
    // In error mode, script body is at file scope
    assert!(
        code.contains("import { ref } from 'vue'"),
        "import must be present:\n{}",
        code
    );
    // Positive: the `count.value++` line must appear
    assert!(
        code.contains("count.value++"),
        "valid expression must be preserved:\n{}",
        code
    );
    // Positive: `function increment()` must appear (user function preserved)
    assert!(
        code.contains("function increment()"),
        "user function must be preserved:\n{}",
        code
    );
}

#[test]
fn template_wrapper_with_script_error() {
    let (code, bindings, _) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>"#,
    );
    // With partial AST recovery: TemplateBindingFN wraps the BODY (not just template)
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "TemplateBindingFN wrapper must exist:\n{}",
        code
    );
    // Import hoisted to file scope
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let import_pos = code.find("import { ref } from 'vue'").unwrap();
    assert!(
        import_pos < fn_pos,
        "import must be hoisted before TemplateBindingFN:\n{}",
        code
    );
    // count binding extracted from clean prefix
    assert!(
        bindings.contains_key("count"),
        "count binding should be extracted: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    // Wrapper must close
    assert!(
        code.contains("close templateBindingFN"),
        "TemplateBindingFN must be closed:\n{}",
        code
    );
}

#[test]
fn script_error_with_partial_recovery_has_destructuring() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
    );
    // With partial AST recovery, the clean prefix is used for normal codegen.
    // count is a ref binding so it gets unwrapped destructuring.
    assert!(
        bindings.contains_key("count"),
        "count should be extracted: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        code.contains("import { ref } from 'vue'"),
        "import should be present:\n{}",
        code
    );
}

#[test]
fn script_error_partial_recovery_preserves_declarations() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
    );
    // With partial AST recovery: declaration preserved in output
    assert!(
        code.contains("const count = ref(0)"),
        "declaration should be preserved:\n{}",
        code
    );
    // Import should be hoisted
    assert!(
        code.contains("import { ref } from 'vue'"),
        "import should be preserved:\n{}",
        code
    );
    // Binding extracted from clean prefix
    assert!(
        bindings.contains_key("count"),
        "count binding should be extracted: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
}

// =========================================================================
// JSX mode tests — verify JS SFCs produce JavaScript + JSDoc, not TypeScript
// =========================================================================

#[test]
fn jsx_mode_no_prettify_import() {
    let (code, type_constructs) = gen_jsx_script(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#,
    );
    // Positive: should still have the vue import
    assert!(
        code.contains("import { ref } from 'vue'"),
        "vue import should be preserved:\n{}",
        code
    );
    // Negative: no TS-only Prettify import
    assert!(
        !code.contains("Prettify"),
        "JSX mode must not import Prettify:\n{}",
        code
    );
    assert!(
        !code.contains("import type"),
        "JSX mode must not have import type:\n{}",
        code
    );
    // Negative: no ambient module in type_constructs
    assert!(
        !type_constructs.contains("declare module"),
        "JSX mode must not have declare module in type_constructs:\n{}",
        type_constructs
    );
}

#[test]
fn jsx_mode_no_as_unknown_cast() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    // Negative: no `as unknown as typeof`
    assert!(
        !code.contains("as unknown as typeof"),
        "JSX mode must not have 'as unknown as typeof' cast:\n{}",
        code
    );
    // Negative: no `as unknown`
    assert!(
        !code.contains("as unknown"),
        "JSX mode must not have any 'as unknown' cast:\n{}",
        code
    );
}

#[test]
fn jsx_mode_no_type_alias() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>"#,
    );
    // Negative: no `;type` alias
    assert!(
        !code.contains(";type "),
        "JSX mode must not have type aliases:\n{}",
        code
    );
    // Negative: generic brackets should be removed from defineProps<...>
    assert!(
        !code.contains("defineProps<"),
        "JSX mode must remove generic brackets from defineProps:\n{}",
        code
    );
}

#[test]
fn jsx_mode_no_generic_on_wrapper() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    // Negative: no generic on TemplateBindingFN
    assert!(
        !code.contains("TemplateBindingFN<"),
        "JSX mode must not have generics on TemplateBindingFN:\n{}",
        code
    );
    // Positive: should still have the function
    assert!(
        code.contains("TemplateBindingFN"),
        "should still have TemplateBindingFN:\n{}",
        code
    );
}

#[test]
fn jsx_mode_no_ambient_module() {
    let (_, type_constructs) = gen_jsx_script(
        r#"<script setup>
const count = ref(0)
</script>"#,
    );
    // Negative: no ambient module declaration
    assert!(
        !type_constructs.contains("declare module"),
        "JSX mode must not have 'declare module' in type_constructs:\n{}",
        type_constructs
    );
    assert!(
        !type_constructs.contains("@verter/types"),
        "JSX mode must not reference @verter/types module:\n{}",
        type_constructs
    );
}

#[test]
fn jsx_mode_instance_declaration_no_ts_syntax() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    // Negative: no TS instance declaration syntax
    assert!(
        !code.contains("!:"),
        "JSX mode must not have definite assignment assertion '!:':\n{}",
        code
    );
    assert!(
        !code.contains("InstanceType<"),
        "JSX mode must not have InstanceType<>:\n{}",
        code
    );
    assert!(
        !code.contains("declare let"),
        "JSX mode must not have 'declare let':\n{}",
        code
    );
    // Positive: should have JSDoc-style instance declaration
    assert!(
        code.contains("/** @type {any} */"),
        "JSX mode should use JSDoc @type for instance:\n{}",
        code
    );
}

#[test]
fn jsx_mode_global_component_no_conditional_type() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
</script>
<template><RouterView /></template>"#,
    );
    // Negative: no TS conditional type for global components
    assert!(
        !code.contains("infer C"),
        "JSX mode must not have 'infer C' conditional type:\n{}",
        code
    );
    assert!(
        !code.contains("import('vue').GlobalComponents"),
        "JSX mode must not reference GlobalComponents type:\n{}",
        code
    );
    // Positive: should have JSDoc unknown assertion
    assert!(
        code.contains("/** @type {unknown} */"),
        "JSX mode should use /** @type {{unknown}} */ for global components:\n{}",
        code
    );
}

#[test]
fn jsx_mode_comp_function_no_ts_assertion() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
</script>
<template><div>hello</div></template>"#,
    );
    // Negative: no TS `as` cast for native elements
    assert!(
        !code.contains("{} as HTMLElementTagNameMap"),
        "JSX mode must not have '{{}} as HTMLElementTagNameMap':\n{}",
        code
    );
    // Positive: should use JSDoc for element type
    if code.contains("HTMLElementTagNameMap") {
        assert!(
            code.contains("/** @type"),
            "JSX mode should use JSDoc @type for element types:\n{}",
            code
        );
    }
}

#[test]
fn jsx_mode_no_angle_bracket_rewrite() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const x = (foo as Bar)
</script>"#,
    );
    // For JSX mode, TS type assertions should not be rewritten
    // (they're already not valid JS, but we skip the rewrite)
    assert!(
        !code.contains("( as "),
        "JSX mode should not rewrite angle bracket casts:\n{}",
        code
    );
}

#[test]
fn jsx_mode_with_defaults() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const props = withDefaults(defineProps<{ msg?: string }>(), {
  msg: 'hello'
})
</script>"#,
    );
    // Negative: no type alias
    assert!(
        !code.contains(";type "),
        "JSX mode withDefaults must not have type aliases:\n{}",
        code
    );
    // Negative: no Prettify
    assert!(
        !code.contains("Prettify"),
        "JSX mode withDefaults must not have Prettify:\n{}",
        code
    );
    // Negative: no generic brackets on defineProps
    assert!(
        !code.contains("defineProps<"),
        "JSX mode must remove generic brackets from defineProps:\n{}",
        code
    );
}

#[test]
fn jsx_mode_define_emits() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const emit = defineEmits<{
  (e: 'update', value: string): void
}>()
</script>"#,
    );
    // Negative: no type alias
    assert!(
        !code.contains(";type "),
        "JSX mode defineEmits must not have type aliases:\n{}",
        code
    );
    // Negative: no Prettify
    assert!(
        !code.contains("Prettify"),
        "JSX mode defineEmits must not have Prettify:\n{}",
        code
    );
}

#[test]
fn jsx_mode_define_model() {
    let (code, _) = gen_jsx_script(
        r#"<script setup>
const modelValue = defineModel<string>()
</script>"#,
    );
    // Negative: no type alias
    assert!(
        !code.contains(";type "),
        "JSX mode defineModel must not have type aliases:\n{}",
        code
    );
}

#[test]
fn jsx_mode_options_api_no_declare_let() {
    let (code, _) = gen_jsx_script(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    // Negative: no TS-only syntax
    assert!(
        !code.contains("declare let"),
        "JSX options API must not have 'declare let':\n{}",
        code
    );
    assert!(
        !code.contains("!:"),
        "JSX options API must not have definite assignment:\n{}",
        code
    );
}

#[test]
fn jsx_options_api_instance_uses_var() {
    let (code, _) = gen_jsx_script(
        r#"<script>
export default {
  data() { return { d_rows: [] } }
}
</script>
<template><div>{{ d_rows }}</div></template>"#,
    );
    // Positive: JS Options API uses var (not declare let) for instance
    assert!(
        code.contains("var ___VERTER___instance"),
        "JS Options API should use var for instance:\n{}",
        code
    );
    // Negative: must NOT use TS declare let syntax
    assert!(
        !code.contains("declare let ___VERTER___instance"),
        "JS Options API must not use 'declare let' for instance:\n{}",
        code
    );
}

#[test]
fn tsx_mode_still_has_ts_constructs() {
    // Contrast test: TSX mode (is_jsx = false) should still have TS constructs
    let (code, _, type_constructs) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    // Positive: TSX mode should have TS constructs
    assert!(
        code.contains("Prettify") || code.contains("import type"),
        "TSX mode should have Prettify import:\n{}",
        code
    );
    assert!(
        code.contains("as unknown as typeof") || code.contains("TemplateBindingFN<"),
        "TSX mode should have TS casts or generics:\n{}",
        code
    );
    // Positive: type_constructs should have ambient module
    assert!(
        type_constructs.contains("declare module") || type_constructs.contains("@verter/types"),
        "TSX mode should have ambient module:\n{}",
        type_constructs
    );
}

// ── Scope-Aware Comp Functions (v-slot / v-for) ─────────────────

#[test]
fn comp_function_vslot_component_references_parent_slot_type() {
    // When <Comp /> comes from v-slot="{Comp}" on a parent component,
    // the Comp function should reconstruct the type through the parent's
    // instantiated slot type.
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { useTemplateRef } from 'vue'
const myRef = useTemplateRef('myRef')
</script>
<template>
  <MyComp v-slot="{ Comp }">
<Comp ref="myRef" />
  </MyComp>
</template>"#,
    );

    // Positive: parent MyComp should have a Comp function emitted
    assert!(
        code.contains("instantiateComponent(MyComp,"),
        "parent MyComp should have a Comp function: {code}"
    );

    // Positive: child Comp should reference parent's slot type
    // The child Comp function should drill into $slots.default to get the slot props
    assert!(
        code.contains("$slots") && code.contains("default"),
        "child Comp should reference parent's $slots.default: {code}"
    );

    // Negative: child Comp should NOT directly use `instantiateComponent(Comp, {})`
    // WITHOUT the slot type reconstruction preamble. The scope-aware Comp function
    // DOES use instantiateComponent(Comp, {}) but only after reconstructing the type.
    // So we verify the preamble is present (the __Parent type alias).
    assert!(
        code.contains("type __Parent = ReturnType<typeof ___VERTER___Comp"),
        "child Comp should have __Parent type reconstruction preamble: {code}"
    );
}

#[test]
fn comp_function_vslot_named_slot() {
    // Named slot: <template #items="{Comp}"> should reference $slots['items']
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { useTemplateRef } from 'vue'
const myRef = useTemplateRef('myRef')
</script>
<template>
  <MyComp>
<template #items="{ Comp }">
  <Comp ref="myRef" />
</template>
  </MyComp>
</template>"#,
    );

    // Positive: should reference the named slot 'items'
    assert!(
        code.contains("$slots") && code.contains("items"),
        "named slot should reference $slots['items']: {code}"
    );
}

#[test]
fn comp_function_vfor_scope_component_ref() {
    // v-for with PascalCase iterator used as component tag:
    // <MyComp v-slot="{ items }">
    //   <template v-for="Comp in items">
    //     <Comp ref="compRef" />
    //   </template>
    // </MyComp>
    // The Comp comes from v-for iteration, so the Comp function should
    // reconstruct the iterator element type.
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { useTemplateRef } from 'vue'
const compRef = useTemplateRef('compRef')
const components = [() => {}]
</script>
<template>
  <div v-for="Comp in components">
<Comp ref="compRef" />
  </div>
</template>"#,
    );

    // Positive: Comp function should reconstruct the v-for iterator type
    assert!(
        code.contains("(typeof components)[number]"),
        "v-for Comp should use iterable element type: {code}"
    );
}

#[test]
fn comp_function_parent_vslot_emits_comp_even_without_ref() {
    // Parent elements with v-slot should always emit a Comp function
    // even without a ref, since child scope-aware Comp functions reference the parent offset
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { useTemplateRef } from 'vue'
const myRef = useTemplateRef('myRef')
</script>
<template>
  <MyComp v-slot="{ Comp }">
<Comp ref="myRef" />
  </MyComp>
</template>"#,
    );

    // Count Comp functions: should have at least 2 (one for MyComp parent, one for Comp child)
    let comp_fn_count = code.matches("function ___VERTER___Comp").count();
    assert!(
        comp_fn_count >= 2,
        "should emit Comp function for both parent (MyComp) and child (Comp from v-slot), found {comp_fn_count}: {code}"
    );
}

#[test]
fn template_first_empty_script_setup_generates_valid_tsx() {
    let (code, _bindings, _) = gen_tsx_script_full(
        r#"<template>
	<section class="page">
		<h1>Chat</h1>
	</section>
</template>
<script setup lang="ts">
</script>"#,
    );
    // The function wrapper must open BEFORE the template return and close AFTER it.
    // Template-first + empty script setup should still produce valid TSX.
    let fn_open_pos = code
        .find("function ___VERTER___TemplateBindingFN")
        .unwrap_or_else(|| panic!("should have TemplateBindingFN: {code}"));
    let close_fn_pos = code
        .find("} // close templateBindingFN")
        .unwrap_or_else(|| panic!("should have close marker: {code}"));
    assert!(
        fn_open_pos < close_fn_pos,
        "function opening must come before closing. open={fn_open_pos}, close={close_fn_pos}: {code}"
    );
    // Must not have return_close before function opening
    assert!(
        !code[..fn_open_pos].contains("close block scope"),
        "return_close must not appear before function opening: {code}"
    );
}

#[test]
fn template_first_nonempty_script_setup_generates_valid_tsx() {
    let (code, _bindings, _) = gen_tsx_script_full(
        r#"<template>
  <div>{{ msg }}</div>
</template>
<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );

    let fn_open_pos = code
        .find("function ___VERTER___TemplateBindingFN")
        .unwrap_or_else(|| panic!("should have TemplateBindingFN: {code}"));
    let close_fn_pos = code
        .find("} // close templateBindingFN")
        .unwrap_or_else(|| panic!("should have close marker: {code}"));
    assert!(
        fn_open_pos < close_fn_pos,
        "function opening must come before closing: {code}"
    );
    assert!(
        code.contains("const msg"),
        "should preserve script content: {code}"
    );
}

#[test]
fn options_api_with_multibyte_utf8_does_not_panic() {
    // Chinese characters in comments cause multi-byte UTF-8 boundaries.
    // Props binding extraction must not panic when slicing into source.
    let (code, bindings) = gen_tsx_script(
        r#"<template><div>{{ title }}</div></template>
<script>
// 设定数据
export default {
  props: ['title', 'count'],
  data() {
return { msg: '你好' }
  }
}
</script>"#,
    );

    // Props should be extracted correctly despite multi-byte chars
    assert!(
        bindings.contains_key("title"),
        "should extract 'title' prop: {bindings:?}"
    );
    assert!(
        bindings.contains_key("count"),
        "should extract 'count' prop: {bindings:?}"
    );
    // Data binding
    assert!(
        bindings.contains_key("msg"),
        "should extract 'msg' data binding: {bindings:?}"
    );
    // Should produce valid output (not panic)
    assert!(!code.is_empty(), "should produce non-empty output");
}

#[test]
fn void_reference_suppresses_unused_emits() {
    // When defineEmits is called without assignment, Verter generates
    // `const ___VERTER___emits = defineEmits(...)`. This auto-var should
    // have a `void` reference to suppress TS6133.
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
defineEmits<{ click: [e: MouseEvent] }>()
</script>
<template><div>content</div></template>"#,
    );
    // Positive: auto-generated emits var exists
    assert!(
        code.contains("___VERTER___emits"),
        "should declare ___VERTER___emits: {}",
        code
    );
    // Positive: void reference suppresses unused warning
    assert!(
        code.contains("void ___VERTER___emits"),
        "should have void ___VERTER___emits to suppress unused warning: {}",
        code
    );
}

#[test]
fn void_reference_suppresses_unused_props() {
    // When defineProps generates `const __props = ...`, it should also emit
    // `void __props;` to suppress TS6133 "declared but never read" when the
    // template doesn't reference any props directly.
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>static content</div></template>"#,
    );
    // Positive: __props is declared
    assert!(
        code.contains("const __props = "),
        "should declare __props: {}",
        code
    );
    // Positive: void __props suppresses unused warning
    assert!(
        code.contains("void __props"),
        "should have void __props to suppress unused warning: {}",
        code
    );
}

// ── void(name) for script-referenced bindings ────────────────────

#[test]
fn test_void_script_referenced_binding() {
    let source = r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
const unused = ref(42)
</script>
<template><div>{{ doubled }}</div></template>"#;
    let (code, _) = gen_tsx_script(source);
    // count is used in script (in computed), should get void()
    assert!(
        code.contains("void(count)"),
        "should emit void(count) for script-referenced binding: {}",
        code
    );
    // doubled is used in template only, not in script — no void needed
    assert!(
        !code.contains("void(doubled)"),
        "should NOT emit void(doubled) — only used in template: {}",
        code
    );
    // unused is not used anywhere in script
    assert!(
        !code.contains("void(unused)"),
        "should NOT emit void(unused) — not referenced in script: {}",
        code
    );
}

#[test]
fn test_void_script_referenced_shadowed() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function foo(count: number) { return count; }
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _) = gen_tsx_script(source);
    // count is only referenced where shadowed by param — not a free ref
    assert!(
        !code.contains("void(count)"),
        "should NOT emit void(count) — only shadowed references: {}",
        code
    );
}

#[test]
fn test_void_style_v_bind_referenced() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const color = ref('red')
</script>
<template><div>hello</div></template>"#;
    let (code, _, _) = gen_tsx_script_full_with_options(
        source,
        IdeScriptOptions {
            component_name: "App",
            js_component_name: "App",
            filename: "App.vue",
            scope_id: "data-v-abc123",
            has_scoped_style: false,
            runtime_module_name: "vue",
            types_module_name: "@verter/types",
            is_vapor: false,
            embed_ambient_types: true,
            is_jsx: false,
            conditional_root_narrowing: false,
            style_v_bind_vars: vec!["color".to_string()],
            css_modules: vec![],
        },
    );
    // color is referenced in style v-bind, should get void()
    assert!(
        code.contains("void(color)"),
        "should emit void(color) for style v-bind referenced binding: {}",
        code
    );
}

// ── resolve_all_prop_refs_in_expr: object key context ─────────────

#[test]
fn resolve_prop_refs_skips_object_property_keys() {
    let mut prop_names = rustc_hash::FxHashSet::default();
    prop_names.insert("zIndex");
    prop_names.insert("position");

    // Object literal: key `zIndex` should NOT be prefixed, value `zIndex` SHOULD be
    let result =
        resolve_all_prop_refs_in_expr("{ position: 'absolute', zIndex: zIndex - 2 }", &prop_names);
    assert!(
        result.contains("zIndex: __props.zIndex"),
        "value `zIndex` should be prefixed with __props.: {result}"
    );
    assert!(
        !result.contains("__props.zIndex:"),
        "object key `zIndex` must NOT be prefixed: {result}"
    );
    assert!(
        !result.contains("__props.position:"),
        "object key `position` must NOT be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_still_replaces_ternary_before_colon() {
    let mut prop_names = rustc_hash::FxHashSet::default();
    prop_names.insert("flag");

    // Ternary: `flag` before `:` is a value, not an object key
    let result = resolve_all_prop_refs_in_expr("cond ? flag : other", &prop_names);
    assert!(
        result.contains("__props.flag"),
        "ternary value should still be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_object_key_with_prop_value() {
    let mut prop_names = rustc_hash::FxHashSet::default();
    prop_names.insert("size");

    // Object key is a prop name, value is also a prop name
    let result = resolve_all_prop_refs_in_expr("{ size: size + 1 }", &prop_names);
    assert!(
        result.contains("size: __props.size"),
        "value `size` should be prefixed: {result}"
    );
    assert!(
        !result.contains("__props.size:"),
        "key `size` must NOT be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_shorthand_property_not_prefixed() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("flag");
    // Shorthand property — should NOT prefix (it's both key and value in shorthand form)
    let result = resolve_all_prop_refs_in_expr("{ flag }", &props);
    assert!(
        !result.contains("__props."),
        "shorthand property `flag` should NOT be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_computed_property_key() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("flag");
    // Computed property key — should prefix inside brackets
    let result = resolve_all_prop_refs_in_expr("{ [flag]: 1 }", &props);
    assert!(
        result.contains("__props.flag"),
        "computed property key `flag` should be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_nested_ternary_with_object() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("flag");
    props.insert("size");
    // Nested ternary with object
    let result = resolve_all_prop_refs_in_expr("flag ? { size: size } : null", &props);
    assert!(
        result.contains("__props.flag"),
        "ternary test `flag` should be prefixed: {result}"
    );
    assert!(
        result.contains("__props.size"),
        "object value `size` should be prefixed: {result}"
    );
    assert!(
        !result.contains("__props.size:"),
        "object key `size` must NOT be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_member_expression_only_prefixes_root() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("flag");
    // Member expression — only prefix the root
    let result = resolve_all_prop_refs_in_expr("flag.value", &props);
    assert!(
        result.contains("__props.flag.value"),
        "member expression root should be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_arrow_function_shadows_prop() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("flag");
    // Arrow function param shadows prop
    let result = resolve_all_prop_refs_in_expr("(flag) => flag", &props);
    assert!(
        !result.contains("__props.flag"),
        "arrow function param should shadow prop: {result}"
    );
}

#[test]
fn resolve_prop_refs_template_literal() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("name");
    // Template literal with expression
    let result = resolve_all_prop_refs_in_expr("`hello ${name}`", &props);
    assert!(
        result.contains("__props.name"),
        "template literal expression should be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_logical_expression() {
    let mut props = rustc_hash::FxHashSet::default();
    props.insert("showBoard");
    props.insert("isEditing");
    // Logical OR — both props should be prefixed
    let result = resolve_all_prop_refs_in_expr("showBoard || isEditing", &props);
    assert!(
        result.contains("__props.showBoard"),
        "left operand should be prefixed: {result}"
    );
    assert!(
        result.contains("__props.isEditing"),
        "right operand should be prefixed: {result}"
    );
}

#[test]
fn resolve_prop_refs_comp_function_object_literal_in_binding() {
    // End-to-end: compile SFC with object literal binding containing prop name as key
    let source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
const props = defineProps<{ zIndex: number }>()
</script>
<template>
  <MyComp :overlay-style="{ zIndex: zIndex - 2 }" />
</template>"#;
    let (code, _) = gen_tsx_script(source);
    eprintln!("Object key prop test output:\n{code}");

    // The Comp function should exist
    assert!(code.contains("Comp"), "should have a Comp function: {code}");

    // The Comp function should NOT have `__props.zIndex:` (invalid object key)
    assert!(
        !code.contains("__props.zIndex:"),
        "object key `zIndex` must NOT be prefixed with __props.: {code}"
    );

    // The generated TSX should parse without errors
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated TSX should have no parse errors, got {}: {code}",
        parsed.errors.len()
    );
}
