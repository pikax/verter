//! Template-ref + conditional-narrowing tests (D7 cohort).

use super::*;

// ── Conditional root narrowing tests ─────────────────────────

#[test]
fn narrowing_bare_prop() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
    );
    // Positive: getRootComponent should have T_foo generic
    assert!(
        code.contains("T_foo extends"),
        "should have T_foo generic: {code}"
    );
    // Positive: conditional type return, not Math.random()
    assert!(
        code.contains("T_foo extends true ?"),
        "should have conditional type T_foo extends true: {code}"
    );
    // Negative: getRootComponent should NOT use Math.random()
    let root_fn = code
        .split("getRootComponent")
        .nth(1)
        .unwrap_or("")
        .split("getRootComponentPassedProps")
        .next()
        .unwrap_or("");
    assert!(
        !root_fn.contains("Math.random()"),
        "getRootComponent should NOT use Math.random() when narrowing is enabled: {code}"
    );
}

#[test]
fn narrowing_multi_prop_chain() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean, s?: 'foo' | 'bar'}>()
</script>
<template><div v-if="foo">A</div><span v-else-if="s === 'foo'">B</span><canvas v-else-if="s === 'bar'">C</canvas><input v-else /></template>"#,
    );
    // Positive: two generics
    assert!(code.contains("T_foo extends"), "should have T_foo: {code}");
    assert!(code.contains("T_s extends"), "should have T_s: {code}");
    // Positive: nested conditional type
    assert!(
        code.contains("T_foo extends true ?"),
        "first condition: {code}"
    );
    assert!(
        code.contains("T_s extends 'foo' ?"),
        "second condition: {code}"
    );
    assert!(
        code.contains("T_s extends 'bar' ?"),
        "third condition: {code}"
    );
}

#[test]
fn narrowing_negated() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
defineProps<{disabled?: boolean}>()
</script>
<template><div v-if="!disabled">A</div><span v-else>B</span></template>"#,
    );
    // Negated: T_disabled extends false means "!disabled is true"
    assert!(
        code.contains("T_disabled extends false ?"),
        "negated should use extends false: {code}"
    );
}

#[test]
fn narrowing_disabled_by_default() {
    // Use the standard helper (conditional_root_narrowing: false)
    let (code, _, _) = gen_tsx_script_full(
        r#"<script setup lang="ts">
defineProps<{foo?: boolean}>()
</script>
<template><div v-if="foo">A</div><span v-else>B</span></template>"#,
    );
    // When disabled, should use Math.random() union, not conditional types
    assert!(
        code.contains("Math.random()"),
        "should use Math.random() when narrowing disabled: {code}"
    );
    assert!(
        !code.contains("T_foo extends"),
        "should NOT have narrowing generics when disabled: {code}"
    );
}

#[test]
fn narrowing_complex_fallback() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
defineProps<{show?: boolean, count?: number}>()
</script>
<template><div v-if="show && count">A</div><span v-else>B</span></template>"#,
    );
    // Complex condition: falls back to Math.random() union
    assert!(
        code.contains("Math.random()"),
        "complex conditions should fall back to Math.random(): {code}"
    );
    assert!(
        !code.contains("T_show extends"),
        "should NOT have narrowing generics for complex conditions: {code}"
    );
}

#[test]
fn narrowing_appends_to_existing_generics() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{show: boolean}>()
</script>
<template><div v-if="show">A</div><span v-else>B</span></template>"#,
    );
    // Should have both T (existing) and T_show (narrowing)
    assert!(
        code.contains("T_show extends"),
        "should have T_show narrowing generic: {code}"
    );
    // The existing generic T should still be present
    assert!(
        code.contains("T extends string"),
        "should preserve existing generic: {code}"
    );
}

#[test]
fn narrowing_triple_same_prop() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
defineProps<{m?: 'a' | 'b' | 'c'}>()
</script>
<template><div v-if="m === 'a'">A</div><span v-else-if="m === 'b'">B</span><p v-else>C</p></template>"#,
    );
    // Single generic T_m for same prop across branches
    assert!(
        code.contains("T_m extends"),
        "should have single T_m generic: {code}"
    );
    assert!(code.contains("T_m extends 'a' ?"), "first branch: {code}");
    assert!(code.contains("T_m extends 'b' ?"), "second branch: {code}");
}

#[test]
fn narrowing_component_roots() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import OtherComp from './OtherComp.vue'
defineProps<{variant?: 'primary' | 'secondary'}>()
</script>
<template><MyComp v-if="variant === 'primary'" /><OtherComp v-else /></template>"#,
    );
    assert!(
        code.contains("T_variant extends"),
        "should have T_variant generic: {code}"
    );
    assert!(
        code.contains("T_variant extends 'primary' ?"),
        "should narrow on variant: {code}"
    );
}

#[test]
fn narrowing_mixed_native_component() {
    let code = gen_tsx_script_narrowing(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
defineProps<{simple?: boolean}>()
</script>
<template><div v-if="simple">A</div><MyComp v-else /></template>"#,
    );
    assert!(
        code.contains("T_simple extends"),
        "should have T_simple generic: {code}"
    );
    assert!(
        code.contains("T_simple extends true ?"),
        "should narrow: {code}"
    );
    // Both branches should be referenced
    assert!(
        code.contains("___VERTER___Comp"),
        "should reference Comp functions: {code}"
    );
}

// ── default_Component tests ──────────────────────────────────

#[test]
fn default_component_emitted() {
    let (_, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(
        !tc.contains("___VERTER___Component"),
        "Component export should not be emitted"
    );
}

// ── Instance types tests ─────────────────────────────────────

#[test]
fn instance_type_non_generic() {
    let (_, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "Instance type should no longer be emitted: {}",
        tc
    );
}

#[test]
fn instance_type_generic() {
    let (_, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "Instance type should no longer be emitted: {}",
        tc
    );
}

#[test]
fn instance_type_generic_with_extends() {
    let (_, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>"#,
    );
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "Instance type should no longer be emitted: {}",
        tc
    );
}

#[test]
fn instance_type_multiple_generics() {
    let (_, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="K extends string, V">
const k = {} as unknown as K
const v = {} as unknown as V
</script>"#,
    );
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "Instance type should no longer be emitted: {}",
        tc
    );
}

// ── Bug 1: Event handler param source map ──────────────────────

#[test]
fn event_handler_param_sourcemap_preserved() {
    // Verify that hovering over the `event` parameter in `function handleClick(event) {}`
    // maps back to the original source. The fix uses two targeted overwrites to leave
    // identifiers as Original source chunks instead of one big overwrite.
    let source = r#"<script setup lang="ts">
function handleClick(event) {}
</script>
<template><button @click="handleClick">click</button></template>"#;
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

    let map =
        ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("App.vue"));
    let output = ct.build_string();

    // Positive: the output should contain the tuple-param annotation
    assert!(
        output.contains("...[event]"),
        "should contain tuple param with event: {output}"
    );

    // Find position of "event" in the original SFC source (inside function params)
    let event_src_offset =
        source.find("function handleClick(event)").unwrap() + "function handleClick(".len();
    let event_src_line = source[..event_src_offset].matches('\n').count() as u32;
    let event_src_col = event_src_offset - source[..event_src_offset].rfind('\n').unwrap() - 1;

    // Find position of "event" in the generated output (inside ...[event]: ...)
    let event_gen_pos = output.find("...[event]").unwrap() + "...[".len();
    let event_gen_line = output[..event_gen_pos].matches('\n').count() as u32;
    let event_gen_col = event_gen_pos
        - output[..event_gen_pos]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);

    // Verify there's a sourcemap token mapping the generated `event` back to source `event`
    let tokens: Vec<_> = map.get_tokens().collect();
    let has_event_mapping = tokens.iter().any(|t| {
        t.get_dst_line() == event_gen_line
            && t.get_dst_col() == event_gen_col as u32
            && t.get_src_line() == event_src_line
            && t.get_src_col() == event_src_col as u32
    });
    assert!(
        has_event_mapping,
        "event param should have sourcemap token mapping gen({},{}) → src({},{})\nOutput: {}\nTokens: {:?}",
        event_gen_line, event_gen_col, event_src_line, event_src_col, output,
        tokens.iter().filter(|t| t.get_dst_line() == event_gen_line).collect::<Vec<_>>()
    );
}

#[test]
fn event_handler_multi_param_sourcemap_preserved() {
    // Multi-param case: function handleDrag(startEvent, endEvent) {}
    let source = r#"<script setup lang="ts">
function handleDrag(startEvent, endEvent) {}
</script>
<template><div @drag="handleDrag">drag</div></template>"#;
    let (code, _) = gen_tsx_script(source);

    // Positive: contains the tuple-param annotation with both params
    assert!(
        code.contains("...[startEvent, endEvent]"),
        "should contain both params in tuple: {code}"
    );
    // Negative: the original parens should NOT appear as a single overwrite
    // (i.e., the identifiers remain in the output)
    assert!(
        code.contains("startEvent"),
        "startEvent should be in output: {code}"
    );
    assert!(
        code.contains("endEvent"),
        "endEvent should be in output: {code}"
    );
}
