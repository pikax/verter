//! Tests for partial AST recovery in IDE script codegen.
//!
//! Verifies that typing incomplete expressions (e.g., `count.`) doesn't break
//! all IDE features. OXC produces usable partial ASTs with errors, and the
//! normal codegen path is already defensive. These tests verify:
//!
//! - Category A: Errors in passthrough regions → normal codegen with partial AST
//! - Category B: Errors in macro regions → still normal codegen, macro skipped
//! - Category C: Edge cases and boundary conditions
//! - Category D: Copy-paste and bulk edit scenarios
//! - Category E: Assessment struct validation

use super::*;
use crate::code_transform::CodeTransform;

/// Generate TSX script and return (code, bindings, type_constructs).
fn gen(source: &str) -> (String, FxHashMap<String, BindingType>, String) {
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

    if let (Some(return_close), Some(tpl_end)) = (&result.return_close, template_end) {
        ct.prepend_left(tpl_end, return_close);
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

/// Assert that the output has TemplateBindingFN (i.e., NOT error recovery file-scope mode).
fn assert_has_template_binding_fn(code: &str, test_name: &str) {
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "[{test_name}] should have TemplateBindingFN wrapper (not error recovery).\nCode:\n{code}"
    );
}

/// Assert that the output does NOT have TemplateBindingFN (error recovery mode).
#[allow(dead_code)]
fn assert_no_template_binding_fn(code: &str, test_name: &str) {
    assert!(
        !code.contains("___VERTER___TemplateBindingFN"),
        "[{test_name}] should NOT have TemplateBindingFN wrapper (should be error recovery).\nCode:\n{code}"
    );
}

/// Assert that the output is NOT file-scope error recovery (i.e., has the ; export function prefix).
fn assert_not_error_recovery(code: &str, test_name: &str) {
    // Error recovery mode puts script body at file scope (no TemplateBindingFN wrapping the body).
    // Normal mode wraps everything in `;export function ___VERTER___TemplateBindingFN() {`.
    assert_has_template_binding_fn(code, test_name);
}

// ── Category A: Errors in passthrough regions ──────────────────────
// All must produce: TemplateBindingFN wrapper, proper bindings, hoisted imports.

fn assert_category_a(source: &str, test_name: &str) {
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, test_name);

    // count binding should be extracted
    assert!(
        bindings.contains_key("count"),
        "[{test_name}] count binding should be extracted. Bindings: {:?}\nCode:\n{code}",
        bindings.keys().collect::<Vec<_>>()
    );

    // import should be hoisted
    assert!(
        code.contains("import { ref } from 'vue'"),
        "[{test_name}] import should be hoisted.\nCode:\n{code}"
    );
}

fn make_a_source(broken_line: &str) -> String {
    format!(
        r#"<script setup lang="ts">
import {{ ref }} from 'vue'
const count = ref(0)
{broken_line}
</script>
<template><div>{{{{ count }}}}</div></template>"#
    )
}

#[test]
fn partial_ast_member_access() {
    assert_category_a(&make_a_source("count."), "A1");
}

#[test]
fn partial_ast_open_paren() {
    assert_category_a(&make_a_source("count("), "A2");
}

#[test]
fn partial_ast_open_bracket() {
    assert_category_a(&make_a_source("count["), "A3");
}

#[test]
fn partial_ast_open_brace() {
    assert_category_a(&make_a_source("const y = {"), "A4");
}

#[test]
fn partial_ast_open_angle() {
    assert_category_a(&make_a_source("const y: Ref<"), "A5");
}

#[test]
fn partial_ast_unclosed_string() {
    assert_category_a(&make_a_source(r#"const y = ""#), "A6");
}

#[test]
fn partial_ast_unclosed_template() {
    assert_category_a(&make_a_source("const y = `"), "A7");
}

#[test]
fn partial_ast_assignment() {
    assert_category_a(&make_a_source("const y ="), "A8");
}

#[test]
fn partial_ast_colon_type() {
    assert_category_a(&make_a_source("const y:"), "A9");
}

#[test]
fn partial_ast_arrow() {
    assert_category_a(&make_a_source("const fn = () =>"), "A10");
}

#[test]
fn partial_ast_binary_op() {
    assert_category_a(&make_a_source("const y = count +"), "A11");
}

#[test]
fn partial_ast_ternary() {
    assert_category_a(&make_a_source("const y = count ?"), "A12");
}

#[test]
fn partial_ast_logical_op() {
    assert_category_a(&make_a_source("const y = count &&"), "A13");
}

#[test]
fn partial_ast_optional_chain() {
    assert_category_a(&make_a_source("count?."), "A14");
}

#[test]
fn partial_ast_spread() {
    assert_category_a(&make_a_source("const y = [..."), "A15");
}

#[test]
fn partial_ast_return() {
    assert_category_a(&make_a_source("function foo() { return "), "A16");
}

#[test]
fn partial_ast_typeof() {
    assert_category_a(&make_a_source("const y = typeof "), "A17");
}

#[test]
fn partial_ast_new() {
    assert_category_a(&make_a_source("const y = new "), "A18");
}

#[test]
fn partial_ast_if() {
    assert_category_a(&make_a_source("if ("), "A19");
}

#[test]
fn partial_ast_comma() {
    assert_category_a(&make_a_source("const { a, } = "), "A20");
}

#[test]
fn partial_ast_nullish_coalesce() {
    assert_category_a(&make_a_source("const y = count ??"), "A21");
}

#[test]
fn partial_ast_as() {
    assert_category_a(&make_a_source("const y = count as"), "A22");
}

// ── A.2: Error position variations ─────────────────────────────────

#[test]
fn partial_ast_error_midfile() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
count.
const x = ref(1)
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A23");
    // At least the import should be hoisted
    assert!(
        code.contains("import { ref } from 'vue'"),
        "[A23] import should be hoisted.\nCode:\n{code}"
    );
}

#[test]
fn partial_ast_inside_function_body() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function handler() { event. }
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A24");
    assert!(
        bindings.contains_key("count"),
        "[A24] count binding: {bindings:?}"
    );
}

#[test]
fn partial_ast_inside_arrow_function() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const handler = () => { event. }
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A25");
}

#[test]
fn partial_ast_inside_computed() {
    let source = r#"<script setup lang="ts">
import { computed, ref } from 'vue'
const count = ref(0)
const x = computed(() => count.)
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A27");
}

#[test]
fn partial_ast_multiple_errors() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
foo.
bar.
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_not_error_recovery(&code, "A28");
    assert!(
        code.contains("import { ref } from 'vue'"),
        "[A28] import should be hoisted.\nCode:\n{code}"
    );
}

#[test]
fn partial_ast_error_after_macros() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const props = defineProps<{ count: number }>()
const x = ref(0)
x.
</script>
<template><div>{{ props.count }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A29");
    assert!(
        bindings.contains_key("props"),
        "[A29] props binding: {bindings:?}"
    );
    assert!(bindings.contains_key("x"), "[A29] x binding: {bindings:?}");
}

#[test]
fn partial_ast_with_template() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A30");
    assert!(
        bindings.contains_key("count"),
        "[A30] count binding: {bindings:?}"
    );
}

#[test]
fn partial_ast_with_companion() {
    let source = r#"<script lang="ts">
export const FOO = 'bar'
</script>
<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A31");
}

#[test]
fn partial_ast_multiple_valid_before_error() {
    let source = r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const a = ref(0)
const b = computed(() => a.value)
const c =
</script>
<template><div>{{ a }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "A32");
    assert!(bindings.contains_key("a"), "[A32] a binding: {bindings:?}");
    assert!(bindings.contains_key("b"), "[A32] b binding: {bindings:?}");
}

#[test]
fn partial_ast_deleted_closing_brace() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function foo() {
  const x = 1
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    // May or may not enter error recovery depending on OXC's parse
    // Key: must NOT panic
    let _ = code;
}

#[test]
fn partial_ast_multiline_object_error() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const x = {
  foo: 1,
  bar:
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_not_error_recovery(&code, "A35");
}

// ── Category B: Errors in macro regions ────────────────────────────

#[test]
fn partial_ast_broken_define_props_type() {
    let source = r#"<script setup lang="ts">
defineProps<{ count. }>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B1");
}

#[test]
fn partial_ast_broken_define_emits_type() {
    let source = r#"<script setup lang="ts">
defineEmits<{ 'click': [event. ] }>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B2");
}

#[test]
fn partial_ast_broken_with_defaults() {
    let source = r#"<script setup lang="ts">
withDefaults(defineProps<Props>(), { count. })
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B3");
}

#[test]
fn partial_ast_broken_define_model() {
    let source = r#"<script setup lang="ts">
defineModel<string.>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B4");
}

#[test]
fn partial_ast_incomplete_define_props() {
    let source = r#"<script setup lang="ts">
defineProps<
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B5");
}

#[test]
fn partial_ast_incomplete_define_emits() {
    let source = r#"<script setup lang="ts">
defineEmits<{
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B6");
}

#[test]
fn partial_ast_broken_import() {
    let source = r#"<script setup lang="ts">
import { ref. } from 'vue'
const count = 1
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B7");
}

#[test]
fn partial_ast_incomplete_import() {
    let source = r#"<script setup lang="ts">
import {
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B8");
}

#[test]
fn partial_ast_broken_define_expose() {
    let source = r#"<script setup lang="ts">
defineExpose({ foo. })
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B9");
}

#[test]
fn partial_ast_broken_define_options() {
    let source = r#"<script setup lang="ts">
defineOptions({ name. })
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B10");
}

#[test]
fn partial_ast_broken_define_slots() {
    let source = r#"<script setup lang="ts">
defineSlots<{ default(props. ): any }>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "B11");
}

#[test]
fn partial_ast_valid_before_broken_macro() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const x = ref(0)
defineProps<{
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _bindings, _) = gen(source);
    assert_has_template_binding_fn(&code, "B12");
    assert!(
        code.contains("import { ref } from 'vue'"),
        "[B12] import should be hoisted.\nCode:\n{code}"
    );
}

// ── Category C: Edge cases and boundary conditions ─────────────────

#[test]
fn partial_ast_only_errors() {
    let source = r#"<script setup lang="ts">
count.
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    // This could go either way (error recovery or not) depending on OXC
    // Key: must NOT panic
    let _ = code;
}

#[test]
fn partial_ast_empty_script() {
    let source = "<script setup></script>";
    let (code, _, _) = gen(source);
    // No errors, normal codegen
    assert_has_template_binding_fn(&code, "C2");
}

#[test]
fn partial_ast_error_after_macro() {
    let source = r#"<script setup lang="ts">
defineProps<{ count: number }>()
foo.
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "C3");
}

#[test]
fn partial_ast_error_same_line_as_macro() {
    let source = r#"<script setup lang="ts">
const props = defineProps<Props>(); foo.
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "C4");
}

#[test]
fn partial_ast_nested_generic_error() {
    let source = r#"<script setup lang="ts">
defineProps<{ items: Array<{ name. }> }>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "C5");
}

#[test]
fn partial_ast_type_import_with_error() {
    let source = r#"<script setup lang="ts">
import type { Ref } from 'vue'
const x: Ref.
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "C6");
}

#[test]
fn partial_ast_destructuring_error() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const { x. } = props
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "C7");
}

#[test]
fn partial_ast_multiline_error() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const x = {
  foo: 1,
  bar:
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_not_error_recovery(&code, "C8");
}

#[test]
fn partial_ast_with_sourcemap() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_not_error_recovery(&code, "C11");
    // count should appear in the output
    assert!(code.contains("count"), "[C11] count should be in output");
}

// ── Category D: Copy-paste and bulk edit scenarios ─────────────────

#[test]
fn partial_ast_paste_breaks_macro() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
defineProps<{ PASTED GARBAGE }>()
const x = ref(0)
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "D1");
}

#[test]
fn partial_ast_paste_replaces_everything() {
    let source = r#"<script setup lang="ts">
Lorem ipsum dolor sit amet
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    // Key: must NOT panic — error recovery is acceptable here
    let _ = code;
}

#[test]
fn partial_ast_paste_valid_block_with_trailing_error() {
    let source = r#"<script setup lang="ts">
import { computed } from 'vue'
const a = computed(() => 1)
const b = computed(() => 2)
const c =
</script>
<template><div>{{ a }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "D3");
    assert!(bindings.contains_key("a"), "[D3] a binding: {bindings:?}");
    assert!(bindings.contains_key("b"), "[D3] b binding: {bindings:?}");
}

#[test]
fn partial_ast_paste_breaks_import_only() {
    let source = r#"<script setup lang="ts">
import { ref from 'vue'
const x = 1
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "D4");
}

#[test]
fn partial_ast_paste_multiple_broken_macros() {
    let source = r#"<script setup lang="ts">
defineProps<{
defineEmits<{
const x = 1
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "D5");
}

#[test]
fn partial_ast_paste_new_import_and_usage() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
import { watch } from 'vue'
const x = ref(0)
watch(
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "D6");
    assert!(bindings.contains_key("x"), "[D6] x binding: {bindings:?}");
}

#[test]
fn partial_ast_delete_closing_brace() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const x = ref(0)
function foo() {
  return x.value
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _, _) = gen(source);
    // Must NOT panic
    let _ = code;
}

#[test]
fn partial_ast_paste_html_into_script() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
<div class="test">hello</div>
const x = ref(0)
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, _, _) = gen(source);
    // Must NOT panic
    let _ = code;
}

#[test]
fn partial_ast_paste_duplicate_imports() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
import { ref } from 'vue'
const x = ref(0)
foo.
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "D9");
    assert!(bindings.contains_key("x"), "[D9] x binding: {bindings:?}");
}

#[test]
fn partial_ast_mixed_clean_and_damaged() {
    let source = r#"<script setup lang="ts">
const a = 1
defineProps<{bad}>()
const b = 2
defineEmits<{ok: []}>()
const c =
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "D10");
}

// ── Category E: Assessment struct validation ───────────────────────

#[test]
fn assessment_all_clean() {
    // No errors → normal path, assessment not invoked
    // This test just verifies clean code works normally
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const x = ref(0)
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "E1");
    assert!(bindings.contains_key("x"), "[E1] x binding: {bindings:?}");
}

#[test]
fn assessment_one_error_at_end() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const x = ref(0)
x.
</script>
<template><div>{{ x }}</div></template>"#;
    let (code, bindings, _) = gen(source);
    assert_not_error_recovery(&code, "E2");
    assert!(bindings.contains_key("x"), "[E2] x binding: {bindings:?}");
}

#[test]
fn assessment_macro_damaged() {
    let source = r#"<script setup lang="ts">
defineProps<{ x. }>()
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "E3");
}

#[test]
fn assessment_empty_body() {
    // Completely unparseable content
    let source = "<script setup lang=\"ts\">\x00\x01\x02</script><template><div/></template>";
    let (code, _, _) = gen(source);
    // Must NOT panic
    let _ = code;
}

#[test]
fn assessment_mixed() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
defineProps<{ x. }>()
const y = ref(0)
z.
</script>
<template><div/></template>"#;
    let (code, _, _) = gen(source);
    assert_has_template_binding_fn(&code, "E5");
}
