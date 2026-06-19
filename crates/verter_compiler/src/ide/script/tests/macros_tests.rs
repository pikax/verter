//! Macro-projection tests (D3 cohort).

use super::*;

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
        style_usage_complete: true,
        css_modules: vec![],
        template_used_vars: None,
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
