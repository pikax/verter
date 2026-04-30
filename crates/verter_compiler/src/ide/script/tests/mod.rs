//! IDE TSX script-generation test cohort (Phase 11d test sibling root).
//!
//! `tests/common.rs` hosts the shared `gen_tsx_script*` helpers; the
//! per-cohort sibling files live alongside this `mod.rs` and target the
//! same production surface that the corresponding `ide/script/<x>.rs`
//! production sibling implements.

#[allow(dead_code)]
mod common;

mod integration_tests;
mod options_api_tests;
mod template_ref_tests;

use super::*;
use crate::code_transform::CodeTransform;
use crate::ide::CssModuleInfo;

/// Generate TSX script and return (code, bindings, type_constructs).
pub(super) fn gen_tsx_script_full(
    source: &str,
) -> (String, FxHashMap<String, BindingType>, String) {
    gen_tsx_script_full_with_opts(source, "App", "App.vue", vec![])
}

/// Generate TSX script with custom component name and CSS modules.
pub(super) fn gen_tsx_script_full_with_opts(
    source: &str,
    component_name: &str,
    filename: &str,
    css_modules: Vec<CssModuleInfo>,
) -> (String, FxHashMap<String, BindingType>, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    // Parse SFC to extract script blocks
    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let js_component_name = crate::ide::sanitize_js_identifier(filename);
    let options = IdeScriptOptions {
        component_name,
        js_component_name: &js_component_name,
        filename,
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: false,
        conditional_root_narrowing: false,
        style_v_bind_vars: vec![],
        css_modules,
    };

    // Use unified CT mode: pass template_end so comp functions are emitted in code
    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    // Apply deferred return+close after template (same as compile.rs)
    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    // Remove template/style blocks from output
    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    let bindings: FxHashMap<String, BindingType> = result
        .bindings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

    (code, bindings, result.type_constructs)
}

pub(super) fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
    let (code, bindings, _) = gen_tsx_script_full(source);
    (code, bindings)
}

/// Like gen_tsx_script_full but with conditional_root_narrowing enabled.
pub(super) fn gen_tsx_script_narrowing(source: &str) -> String {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let options = IdeScriptOptions {
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
        conditional_root_narrowing: true,
        style_v_bind_vars: vec![],
        css_modules: vec![],
    };

    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    ct.build_string()
}

/// IDE output rewrites `.vue` import specifiers to `.vue.ts` so that
/// type providers (TSGO/tsserver) resolve them to the public API output.
/// The rewrite uses CodeTransform::prepend_left so the sourcemap stays correct.
#[test]
fn standalone_and_ambient_types_preserve_slot_argument_maps() {
    let slot_signature =
        "TSlots extends Record<string, any>,\n    N extends keyof TSlots & string,";

    assert!(
        VERTER_TYPES_AMBIENT_MODULE.contains(slot_signature),
        "ambient @verter/types declarations should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_AMBIENT_MODULE
            .contains("): TSlots[N] extends (...args: infer P) => any ? P[0] : never;"),
        "ambient @verter/types declarations should preserve slot prop types"
    );
    assert!(
        VERTER_TYPES_STANDALONE_DTS
            .contains("TSlots extends Record<string, any>,\n  N extends keyof TSlots & string,"),
        "standalone @verter/types stub should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_STANDALONE_DTS
            .contains("): TSlots[N] extends (...args: infer P) => any ? P[0] : never;"),
        "standalone @verter/types stub should preserve slot prop types"
    );
}

#[test]
fn vue_imports_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
import MyComp from './MyComp.vue'
import { helper } from '../utils'
import Another from "@/components/Another.vue"
const x = 1
</script>"#,
    );

    // Positive: .vue imports should become .vue.ts
    assert!(
        code.contains("from './MyComp.vue.ts'"),
        "single-quoted .vue import should become .vue.ts: {code}"
    );
    assert!(
        code.contains("from \"@/components/Another.vue.ts\""),
        "double-quoted .vue import should become .vue.ts: {code}"
    );

    // Negative: non-.vue imports should NOT be rewritten
    assert!(
        code.contains("from '../utils'"),
        "non-.vue import must not be rewritten: {code}"
    );

    // Negative: should NOT have bare .vue' or .vue" (without .ts)
    assert!(
        !code.contains(".vue'") || code.contains(".vue.ts'"),
        "bare .vue' should not remain: {code}"
    );
    assert!(
        !code.contains(".vue\"") || code.contains(".vue.ts\""),
        "bare .vue\" should not remain: {code}"
    );
}

/// Companion `<script>` imports should also be rewritten to `.vue.ts`.
#[test]
fn companion_script_vue_imports_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script>
import Base from './Base.vue'
export default { extends: Base }
</script>
<script setup>
const x = 1
</script>"#,
    );

    assert!(
        code.contains("from './Base.vue.ts'"),
        "companion script .vue import should become .vue.ts: {code}"
    );
}

/// Re-exports like `export { Foo } from './Foo.vue'` should also be rewritten.
#[test]
fn reexport_vue_specifier_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export { default as Dropdown } from './Dropdown.vue'
export * from './utils'
</script>
<script setup>
const x = 1
</script>"#,
    );

    // Positive: .vue re-export should become .vue.ts
    assert!(
        code.contains("from './Dropdown.vue.ts'"),
        "re-export .vue specifier should become .vue.ts: {code}"
    );

    // Negative: non-.vue re-export should NOT be rewritten
    assert!(
        code.contains("from './utils'"),
        "non-.vue re-export must not be rewritten: {code}"
    );
}

#[test]
fn basic_script_setup() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );

    assert!(code.contains("function ___VERTER___TemplateBindingFN()"));
    assert!(code.contains("const msg = 'hello'"));
    assert!(bindings.contains_key("msg"));
}

// ── Instance declaration tests ───────────────────────────────

#[test]
fn instance_declaration_in_script_setup() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("let ___VERTER___instance!:"),
        "Should declare instance variable. Got: {}",
        code
    );
    assert!(
        code.contains("InstanceType<import("),
        "Should use InstanceType import. Got: {}",
        code
    );
    assert!(
        code.contains("import('./App.vue.ts')"),
        "Should reference the component's own .vue.ts file. Got: {}",
        code
    );
    assert!(
        code.contains("void ___VERTER___instance;"),
        "Should void-suppress instance. Got: {}",
        code
    );
}

#[test]
fn instance_probe_line_in_script_setup() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("(___VERTER___instance).valueOf"),
        "Should have probe line. Got: {}",
        code
    );
}

#[test]
fn instance_declaration_in_template_only() {
    let (code, _) = gen_tsx_script(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains("let ___VERTER___instance!:"),
        "Template-only SFC should declare instance. Got: {}",
        code
    );
    assert!(
        code.contains("(___VERTER___instance).valueOf"),
        "Template-only SFC should have probe line. Got: {}",
        code
    );
}

#[test]
fn instance_declaration_in_options_api() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("declare let ___VERTER___instance:"),
        "Options API should use ambient instance declaration. Got: {}",
        code
    );
    assert!(
        code.contains("InstanceType<import("),
        "Options API should use InstanceType import. Got: {}",
        code
    );
}

#[test]
fn script_setup_with_imports() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
import { ref } from 'vue'
import type { Foo } from './types'
const count = ref(0)
</script>"#,
    );

    // Imports should be hoisted above the function wrapper
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let import_ref_pos = code.find("import { ref } from 'vue'").unwrap();
    let import_type_pos = code.find("import type { Foo } from './types'").unwrap();

    assert!(
        import_ref_pos < fn_pos,
        "Runtime import should be hoisted above function"
    );
    assert!(
        import_type_pos < fn_pos,
        "Type import should be hoisted above function"
    );
}

#[test]
fn script_setup_with_type_declarations() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
interface Props {
  msg: string
}
const msg = 'hello'
</script>"#,
    );

    // Type declaration should be hoisted
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let interface_pos = code.find("interface Props").unwrap();
    assert!(
        interface_pos < fn_pos,
        "Interface should be hoisted above function"
    );
}

#[test]
fn script_setup_preserves_macros() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>"#,
    );

    // Macros should be preserved in the body (not transformed)
    assert!(code.contains("defineProps"));
}

#[test]
fn script_setup_extracts_ref_bindings() {
    let (_, bindings) = gen_tsx_script(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#,
    );

    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::SetupRef),
        "ref() binding should be SetupRef"
    );
}

#[test]
fn script_setup_extracts_const_bindings() {
    let (_, bindings) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
const fn = () => {}
</script>"#,
    );

    assert!(
        matches!(
            bindings.get("msg").copied(),
            Some(BindingType::SetupConst) | Some(BindingType::LiteralConst)
        ),
        "String constant should be SetupConst or LiteralConst"
    );
}

#[test]
fn options_api_script() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export default {
  data() {
return { msg: 'hello' }
  }
}
</script>"#,
    );

    assert!(
        code.contains("const __sfc__ ="),
        "export default should be converted to const __sfc__ ="
    );
    assert!(
        code.contains("export default __sfc__"),
        "Should have export default __sfc__ at the end"
    );
}

#[test]
fn no_script_blocks() {
    let (code, _) = gen_tsx_script(
        r#"<template>
  <div>hello</div>
</template>"#,
    );

    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "Should emit minimal component wrapper"
    );
}

#[test]
fn script_setup_lang_ts_with_type_define_props() {
    // Regression: lang="ts" with defineProps<{...}>() caused a panic because
    // type-based prop binding spans include the content offset (absolute),
    // while content_str is local (relative).
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );

    assert!(
        code.contains("defineProps"),
        "Should preserve defineProps call"
    );
    // "msg" should be classified as a Props binding
    assert_eq!(
        bindings.get("msg").copied(),
        Some(BindingType::Props),
        "msg should be Props, got: {:?}",
        bindings.get("msg")
    );
}

#[test]
fn script_setup_lang_ts_with_assigned_define_props() {
    // const props = defineProps<{...}>() — "props" is SetupConst, "count" is Props
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = defineProps<{ count: number }>()
</script>"#,
    );

    assert!(code.contains("defineProps"));
    assert_eq!(
        bindings.get("props").copied(),
        Some(BindingType::SetupConst),
        "props variable should be SetupConst"
    );
    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props, got: {:?}",
        bindings.get("count")
    );
}

#[test]
fn script_setup_lang_ts_with_interface_props() {
    // defineProps with a type reference to a local interface
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
interface MyProps {
  title: string
  count?: number
}
defineProps<MyProps>()
</script>"#,
    );

    assert!(code.contains("defineProps"));
    assert_eq!(
        bindings.get("title").copied(),
        Some(BindingType::Props),
        "title should be Props, got: {:?}",
        bindings.get("title")
    );
    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props, got: {:?}",
        bindings.get("count")
    );
}

// ── Generic wrapper tests ─────────────────────────────────────

#[test]
fn generic_wrapper_simple() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T>()"),
        "wrapper should have <T>: {}",
        code
    );
}

#[test]
fn generic_wrapper_with_extends() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T extends string>()"),
        "wrapper should have <T extends string>: {}",
        code
    );
}

#[test]
fn generic_wrapper_multiple() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="K extends string, V">
const k = {} as unknown as K
const v = {} as unknown as V
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<K extends string, V>()"),
        "wrapper should have multiple generics: {}",
        code
    );
}

#[test]
fn non_generic_wrapper_unchanged() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "non-generic should have no angle brackets: {}",
        code
    );
    assert!(
        !code.contains("function ___VERTER___TemplateBindingFN<"),
        "non-generic should NOT have angle brackets: {}",
        code
    );
}

#[test]
fn generic_wrapper_invalid_syntax_fallback() {
    // "T in string" is invalid TS (should be "extends"), but the raw
    // string should still pass through so TypeScript surfaces the error.
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts" generic="T in string">
const value = 'hello'
</script>"#,
    );
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T in string>()"),
        "invalid generic should still be emitted raw: {}",
        code
    );
}

// ── Helper imports tests ──────────────────────────────────────

#[test]
fn helper_imports_emitted() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("import type { Prettify as ___VERTER___Prettify"),
        "should have Prettify import: {}",
        code
    );
    assert!(
        code.contains("import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef"),
        "should have shallowUnwrapRef import: {}",
        code
    );
    assert!(
        !code.contains("import type { default as ___VERTER___Self }"),
        "self-import should no longer be emitted: {}",
        code
    );
}

#[test]
fn helper_imports_hoisted_before_wrapper() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let import_pos = code.find("import type { Prettify").unwrap();
    assert!(
        import_pos < fn_pos,
        "helper imports should be before wrapper function"
    );
}

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

// ── Bare useAttrs() cast tests ─────────────────────────────

#[test]
fn bare_use_attrs_with_template_gets_cast() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    // Positive: bare useAttrs() should get cast to ___VERTER___Attrs
    assert!(
        code.contains("useAttrs() as unknown as ___VERTER___Attrs"),
        "bare useAttrs() should be cast to ___VERTER___Attrs: {}",
        code
    );
}

#[test]
fn bare_use_attrs_with_inherit_attrs_false_still_cast() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    // Positive: cast is still present (Attrs = attributes only, no RootElementProps)
    assert!(
        code.contains("useAttrs() as unknown as ___VERTER___Attrs"),
        "bare useAttrs() should still be cast when inheritAttrs: false: {}",
        code
    );
}

#[test]
fn typed_use_attrs_no_cast() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const attrs = useAttrs<{ class?: string }>()
</script>
<template><div>hello</div></template>"#,
    );
    // Negative: typed useAttrs<T>() should NOT get an additional cast
    assert!(
        !code.contains("as unknown as ___VERTER___Attrs"),
        "useAttrs<T>() should NOT get a cast: {}",
        code
    );
}

#[test]
fn bare_use_attrs_no_template_no_cast() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const attrs = useAttrs()
</script>"#,
    );
    // Negative: no template means no ___VERTER___Attrs, so no cast
    assert!(
        !code.contains("as unknown as ___VERTER___Attrs"),
        "bare useAttrs() without template should NOT get cast: {}",
        code
    );
}

#[test]
fn bare_use_attrs_with_generics_includes_names() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T">
const attrs = useAttrs()
defineProps<{ items: T[] }>()
</script>
<template><div>hello</div></template>"#,
    );
    // Positive: cast should include generic name bracket
    assert!(
        code.contains("useAttrs() as unknown as ___VERTER___Attrs<T>"),
        "bare useAttrs() with generics should include <T> in cast: {}",
        code
    );
}

// ── Macro Boxing Tests ───────────────────────────────────────

#[test]
fn define_props_no_args() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineProps()
</script>"#,
    );
    assert!(
        code.contains("const ___VERTER___props=defineProps()"),
        "should prepend variable assignment: {}",
        code
    );
}

#[test]
fn define_props_with_type_params() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
        "should emit type alias with Prettify: {}",
        code
    );
    assert!(
        code.contains("defineProps<___VERTER___defineProps_Type>()"),
        "should replace type arg with alias: {}",
        code
    );
    assert!(
        code.contains("const ___VERTER___props=defineProps"),
        "should prepend variable assignment: {}",
        code
    );
}

#[test]
fn define_props_with_type_params_assigned() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
        "should emit type alias with Prettify: {}",
        code
    );
    assert!(
        code.contains("const props = defineProps<___VERTER___defineProps_Type>()"),
        "should keep user variable name: {}",
        code
    );
}

#[test]
fn define_props_simple_type_ref_no_prettify() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
interface Props { msg: string }
defineProps<Props>()
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineProps_Type=Props;"),
        "simple type ref should NOT have Prettify wrapper: {}",
        code
    );
}

#[test]
fn define_props_with_runtime_args() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
defineProps({ a: String })
</script>"#,
    );
    // Runtime args stay as-is, variable assignment prepended, no boxing
    assert!(
        code.contains("const ___VERTER___props=defineProps({ a: String })"),
        "should prepend variable assignment with args as-is: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

#[test]
fn define_props_runtime_args_assigned() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const props = defineProps({ a: String })
</script>"#,
    );
    // Runtime args stay as-is, no boxing
    assert!(
        code.contains("const props = defineProps({ a: String })"),
        "should keep user variable and args: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

#[test]
fn define_emits_no_args() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineEmits()
</script>"#,
    );
    assert!(
        code.contains("const ___VERTER___emits=defineEmits()"),
        "should prepend variable assignment: {}",
        code
    );
}

#[test]
fn define_emits_with_type_params() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineEmits<{ (e: 'change'): void }>()
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineEmits_Type=___VERTER___Prettify<{ (e: 'change'): void }>"),
        "should emit type alias: {}",
        code
    );
    assert!(
        code.contains("defineEmits<___VERTER___defineEmits_Type>()"),
        "should replace type arg: {}",
        code
    );
}

#[test]
fn define_emits_with_array_arg() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
defineEmits(['change', 'update'])
</script>"#,
    );
    // defineEmits with runtime args: variable assignment prepended, no boxing
    assert!(
        code.contains("const ___VERTER___emits=defineEmits(['change', 'update'])"),
        "should prepend variable assignment: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

#[test]
fn define_expose_no_return_no_var() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineExpose({ foo: 'bar' })
</script>"#,
    );
    // defineExpose is a no-return macro, so no `const xxx =` prepended
    assert!(
        !code.contains("const ___VERTER___expose=defineExpose"),
        "defineExpose should NOT have variable assignment: {}",
        code
    );
    // defineExpose stays as-is, no boxing
    assert!(
        code.contains("defineExpose({ foo: 'bar' })"),
        "defineExpose call should be preserved: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

#[test]
fn define_options_no_return() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>"#,
    );
    assert!(
        !code.contains("const ___VERTER___options=defineOptions"),
        "defineOptions should NOT have variable assignment: {}",
        code
    );
    // defineOptions stays as-is, no boxing
    assert!(
        code.contains("defineOptions({ inheritAttrs: false })"),
        "defineOptions call should be preserved: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

#[test]
fn define_slots_no_args() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineSlots()
</script>"#,
    );
    assert!(
        code.contains("const ___VERTER___slots=defineSlots()"),
        "should prepend variable assignment: {}",
        code
    );
}

#[test]
fn define_slots_with_type_params() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineSlots<{ default: (props: {}) => any }>()
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineSlots_Type"),
        "should emit type alias: {}",
        code
    );
}

// ── TemplateBinding Return Tests ─────────────────────────────

#[test]
fn template_binding_return_with_bindings() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("___VERTER___shallowUnwrapRef("),
        "should have shallowUnwrapRef in return: {}",
        code
    );
    assert!(
        code.contains("count: count as unknown as typeof count"),
        "should have count binding in return: {}",
        code
    );
    assert!(
        code.contains("msg: msg as unknown as typeof msg"),
        "should have msg binding in return: {}",
        code
    );
}

// ── withDefaults Tests ───────────────────────────────────────

#[test]
fn with_defaults_type_params() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hello' })
</script>"#,
    );
    assert!(
        code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
        "should emit defineProps type alias: {}",
        code
    );
    // withDefaults call stays with type alias replacement, no boxing
    assert!(
        code.contains(
            "withDefaults(defineProps<___VERTER___defineProps_Type>(), { msg: 'hello' })"
        ),
        "withDefaults call should stay with type alias replacement: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

// ── is_simple_type_reference Tests ───────────────────────────

#[test]
fn simple_type_ref_detection() {
    assert!(is_simple_type_reference("Props"));
    assert!(is_simple_type_reference("MyType"));
    assert!(is_simple_type_reference("Foo.Bar"));
    assert!(!is_simple_type_reference("{ msg: string }"));
    assert!(!is_simple_type_reference("string | number"));
    assert!(!is_simple_type_reference("Array<string>"));
    assert!(!is_simple_type_reference(""));
    assert!(!is_simple_type_reference("  "));
}

// ── Part H: ___VERTER___Comp condition guards ────────────────────

#[test]
fn comp_v_if_gets_narrowing_guard() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const el = ref<HTMLDivElement>()
</script>
<template><div v-if="isTypeA" ref="el">A</div></template>"#,
    );
    // Comp function should have condition guard
    assert!(
        code.contains("if(!((isTypeA))) return null;"),
        "Comp for v-if should have condition guard, got:\n{}",
        code
    );
}

#[test]
fn comp_v_else_if_negates_prior_siblings() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const isTypeB = true
const el = ref<HTMLDivElement>()
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else-if="isTypeB" ref="el">B</div>
</template>"#,
    );
    // v-else-if Comp should negate prior v-if and include own condition
    assert!(
        code.contains("!((isTypeA)) && (isTypeB)"),
        "Comp for v-else-if should negate prior v-if, got:\n{}",
        code
    );
}

#[test]
fn comp_v_else_negates_all_prior() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const el = ref<HTMLDivElement>()
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else ref="el">B</div>
</template>"#,
    );
    // v-else Comp should negate all prior conditions
    assert!(
        code.contains("if(!(!((isTypeA)))) return null;"),
        "Comp for v-else should negate prior v-if, got:\n{}",
        code
    );
}

#[test]
fn comp_nested_v_if_combines_parent_and_own() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const parent = true
const child = true
const el = ref<HTMLSpanElement>()
</script>
<template><div v-if="parent"><span v-if="child" ref="el">nested</span></div></template>"#,
    );
    // Nested Comp should combine parent + own condition
    // The span's Comp should have: if(!((parent) && (child))) return null;
    assert!(
        code.contains("(parent) && (child)"),
        "nested Comp should combine parent + own condition, got:\n{}",
        code
    );
}

#[test]
fn comp_all_elements_get_functions_not_just_root() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el1 = ref<HTMLDivElement>()
const el2 = ref<HTMLSpanElement>()
</script>
<template><div ref="el1"><span ref="el2">inner</span></div></template>"#,
    );
    // Both div and span should get Comp functions (both have ref)
    let comp_count = code.matches("function ___VERTER___Comp").count();
    assert!(
        comp_count >= 2,
        "should emit Comp for all ref elements (div + span), found {} Comp functions, got:\n{}",
        comp_count,
        code
    );
}

#[test]
fn no_script_blocks_has_type_constructs() {
    let (code, bindings, type_constructs) =
        gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

    // OXC validation: code + type_constructs must parse as valid TSX
    let full = format!("{}\n{}", code, type_constructs);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Full TSX must be valid: {:?}\n---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        full
    );

    // Positive: minimal wrapper
    assert!(
        code.contains("___VERTER___TemplateBindingFN"),
        "should emit wrapper fn"
    );
    // Positive: helper imports
    assert!(
        code.contains("from \"@verter/types\""),
        "should import from @verter/types"
    );
    // Negative: Instance type should no longer be emitted
    assert!(
        !type_constructs.contains("___VERTER___Instance"),
        "should not emit Instance"
    );
    assert!(
        !type_constructs.contains("InstanceType<typeof ___VERTER___Self>"),
        "should not emit InstanceType<typeof Self>"
    );
    // Negative: no macro imports (template-only has no macros)
    assert!(
        !code.contains("createMacroReturn"),
        "should NOT import createMacroReturn"
    );
    // Bindings should be empty
    assert!(
        bindings.is_empty(),
        "template-only SFC should have no bindings"
    );
}

#[test]
fn no_script_blocks_imports_before_function_wrapper() {
    // TS1232: imports inside a function body are invalid.
    // Template-only SFCs must emit imports BEFORE the function wrapper.
    let (code, _, _) = gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

    let import_pos = code.find("import ").expect("should have import statement");
    let fn_pos = code
        .find("export function ___VERTER___TemplateBindingFN")
        .expect("should have function wrapper");

    assert!(
        import_pos < fn_pos,
        "imports (pos {}) must appear BEFORE function wrapper (pos {})\n---\n{}",
        import_pos,
        fn_pos,
        code
    );
}

#[test]
fn no_script_blocks_no_unused_attributes_type() {
    // TS6196: template-only SFCs should NOT emit ___VERTER___attributes type
    // since there are no Comp functions to reference it.
    let (_, _, type_constructs) = gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

    assert!(
        !type_constructs.contains("___VERTER___attributes"),
        "template-only SFC should NOT emit ___VERTER___attributes (unused), got:\n{}",
        type_constructs
    );
}

#[test]
fn no_script_blocks_with_slot_and_style() {
    let (code, _, type_constructs) = gen_tsx_script_full(
        r#"<template><div class="wrapper"><slot /></div></template>
<style scoped>.wrapper { padding: 20px; }</style>"#,
    );

    let full = format!("{}\n{}", code, type_constructs);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Full TSX must be valid: {:?}\n---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        full
    );

    assert!(
        code.contains("___VERTER___TemplateBindingFN"),
        "should emit wrapper"
    );
    assert!(
        !type_constructs.contains("___VERTER___Instance"),
        "should not emit Instance"
    );
}

// ── #13: Async wrapper function ──────────────────────────────────

// @ai-generated — Async setup must produce async wrapper function.
#[test]
fn script_setup_async_emits_async_wrapper() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const data = await fetch('/api')
</script>"#,
    );
    assert!(
        code.contains("async function ___VERTER___TemplateBindingFN"),
        "async setup must emit async wrapper function: {code}"
    );
}

#[test]
fn script_setup_sync_does_not_emit_async_wrapper() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const x = 1
</script>"#,
    );
    assert!(
        !code.contains("async function"),
        "sync setup must NOT have async keyword: {code}"
    );
}

// ── #11: Angle bracket type assertions ───────────────────────────

// @ai-generated — TSTypeAssertion <T>expr must be rewritten to (expr as T).
#[test]
fn script_setup_ts_type_assertion_rewrite_simple() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const value = <string>someExpr
</script>"#,
    );
    assert!(
        code.contains("(someExpr as string)"),
        "should rewrite <string>someExpr to (someExpr as string): {code}"
    );
    assert!(
        !code.contains("<string>someExpr"),
        "angle bracket assertion must not remain: {code}"
    );
}

#[test]
fn script_setup_ts_type_assertion_rewrite_union() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
let a = <1 | 2>1
</script>"#,
    );
    assert!(
        code.contains("(1 as 1 | 2)"),
        "should rewrite <1|2>1 to (1 as 1|2): {code}"
    );
    assert!(
        !code.contains("<1 | 2>"),
        "angle bracket assertion must not remain: {code}"
    );
}

#[test]
fn script_setup_ts_type_assertion_nested() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
let b = <string><number>x
</script>"#,
    );
    // Nested: <string><number>x → ((<number>x as string) → ((x as number) as string)
    assert!(
        code.contains("as number") && code.contains("as string"),
        "nested assertions should both be rewritten: {code}"
    );
    assert!(
        !code.contains("<string>") && !code.contains("<number>"),
        "angle bracket syntax must not remain: {code}"
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

/// Helper: generate IDE script output with `is_jsx: true`.
fn gen_jsx_script(source: &str) -> (String, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let options = IdeScriptOptions {
        component_name: "App",
        js_component_name: "App",
        filename: "App.vue",
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: true,
        conditional_root_narrowing: false,
        style_v_bind_vars: vec![],
        css_modules: vec![],
    };

    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    (code, result.type_constructs)
}

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

// ── Bug 4: defineProps type parameter output structure ──

#[test]
fn define_props_type_param_output_structure() {
    let (code, _, _) = gen_tsx_script_full(
        r#"<script setup lang="ts">const props = defineProps<{ foo: string, bar: number }>()</script><template><div/></template>"#,
    );

    // Positive: type alias should be created
    assert!(
        code.contains("___VERTER___defineProps_Type"),
        "should create type alias: {code}"
    );
    // Positive: type alias should contain the original type content
    assert!(
        code.contains("{ foo: string, bar: number }"),
        "type alias should contain original type: {code}"
    );
    // Positive: defineProps call should reference the type alias
    assert!(
        code.contains("defineProps<___VERTER___defineProps_Type>()"),
        "defineProps should use type alias: {code}"
    );
}

#[test]
fn define_emits_type_param_output_structure() {
    let (code, _, _) = gen_tsx_script_full(
        r#"<script setup lang="ts">const emit = defineEmits<{ click: [e: MouseEvent] }>()</script><template><div/></template>"#,
    );

    // Positive: type alias should be created
    assert!(
        code.contains("___VERTER___defineEmits_Type"),
        "should create emits type alias: {code}"
    );
    // Positive: defineEmits should use the type alias
    assert!(
        code.contains("defineEmits<___VERTER___defineEmits_Type>()"),
        "defineEmits should use type alias: {code}"
    );
}

#[test]
fn define_props_type_content_is_source_mapped() {
    // Verify that individual properties inside defineProps<{ foo: string }>
    // have sourcemap coverage via move_wrapped.
    let source = r#"<script setup lang="ts">const props = defineProps<{ foo: string, bar: number }>()</script><template><div/></template>"#;
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let options = IdeScriptOptions {
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
        style_v_bind_vars: vec![],
        css_modules: vec![],
    };

    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    // Generate the sourcemap
    let map =
        ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("App.vue"));

    let output = ct.build_string();

    // Find the position of the type block in the original source
    let type_block_start = source.find("{ foo:").unwrap() as u32;
    let type_block_end = source.find("}>()").unwrap() as u32;

    // Check that there's a sourcemap token covering the type block.
    // The move_wrapped operation preserves Original chunks, so there should
    // be a token at or near the start of the type block.
    let tokens: Vec<_> = map.get_tokens().collect();
    let has_type_coverage = tokens.iter().any(|t| {
        let src = t.get_src_col();
        src >= type_block_start && src < type_block_end
    });
    assert!(
        has_type_coverage,
        "type block [{}..{}) should have sourcemap coverage.\nOutput: {}\nTokens: {:?}",
        type_block_start, type_block_end, output, tokens
    );

    // Verify the type content appears in the output in the type alias
    assert!(
        output.contains("Prettify<{ foo: string, bar: number }>"),
        "type content should be in the Prettify wrapper: {output}"
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

/// Generate TSX script with custom options and return (code, bindings, type_constructs).
pub(super) fn gen_tsx_script_full_with_options(
    source: &str,
    options: IdeScriptOptions<'_>,
) -> (String, FxHashMap<String, BindingType>, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    // Use unified CT mode: pass template_end so comp functions are emitted in code
    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    // Apply deferred return+close after template (same as compile.rs)
    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    let bindings: FxHashMap<String, BindingType> = result
        .bindings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

    (code, bindings, result.type_constructs)
}
