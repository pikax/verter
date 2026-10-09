use super::*;

#[test]
pub(super) fn v_if_v_else_no_comment_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="show">yes</span><span v-else>no</span></div></template>"#,
    );
    // Full chain has v-else, so no comment fallback needed
    assert!(
        !code.contains("_createCommentVNode"),
        "v-if/v-else should not emit comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_v_else_if_v_else_complete_chain() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
    );
    // Complete chain, no comment fallback
    assert!(
        !code.contains("_createCommentVNode"),
        "complete v-if chain should not emit comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_after_sibling_has_comma_separator() {
    let code = compile_and_validate_template(
        r#"<template><div><p>text</p><span v-if="show">conditional</span></div></template>"#,
    );
    // The v-if should be separated from the previous sibling by a comma
    assert!(
        code.contains("_createCommentVNode"),
        "v-if without v-else should have comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_chain_after_sibling() {
    let code = compile_and_validate_template(
        r#"<template><div><p>text</p><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
    );
    // Should produce valid JS with comma before the ternary
    assert!(code.contains("function render("));
}

#[test]
pub(super) fn v_if_chain_without_v_else_after_sibling() {
    let code = compile_and_validate_template(
        r#"<template><div><p>text</p><span v-if="a">A</span><span v-else-if="b">B</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "incomplete chain after sibling should have comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_as_root_single_child() {
    let code =
        compile_and_validate_template(r#"<template><div v-if="show">hello</div></template>"#);
    assert!(code.contains("return "));
    assert!(
        code.contains("_createCommentVNode"),
        "root v-if should have comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_v_else_as_root() {
    let code = compile_and_validate_template(
        r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#,
    );
    assert!(code.contains("return "));
}

#[test]
pub(super) fn v_if_in_multi_root_fragment() {
    let code = compile_and_validate_template(
        r#"<template><p>first</p><div v-if="show">middle</div><p>last</p></template>"#,
    );
    assert!(code.contains("_Fragment"));
    assert!(
        code.contains("_createCommentVNode"),
        "v-if in fragment should have comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn multiple_v_if_chains_in_same_parent() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="a">A</span><span v-else>notA</span><span v-if="b">B</span><span v-else>notB</span></div></template>"#,
    );
    // Two independent v-if/v-else chains in the same parent
    assert!(code.contains("function render("));
}

#[test]
pub(super) fn v_if_with_whitespace_between_branches() {
    // Whitespace nodes between v-if/v-else should be skipped
    let code = compile_and_validate_template(
        "<template><div>\n  <span v-if=\"a\">A</span>\n  <span v-else>B</span>\n</div></template>",
    );
    assert!(code.contains("function render("));
}

#[test]
pub(super) fn v_if_nested_inside_v_for() {
    let code = compile_and_validate_template(
        r#"<template><div><div v-for="item in items" :key="item"><span v-if="item.show">{{ item.name }}</span></div></div></template>"#,
    );
    assert!(code.contains("_renderList"));
    assert!(
        code.contains("_createCommentVNode"),
        "v-if inside v-for should have comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_else_chain_with_whitespace_valid_output() {
    // v-if/v-else with whitespace between branches produces valid JS
    let code = compile_and_validate_template(
        r#"<script setup>
const a = ref(true)
</script>
<template><div>
  <div v-if="a">A</div>
  <div v-else>B</div>
</div></template>"#,
    );
    assert!(
        code.contains("($setup.a)"),
        "v-if condition should have setup prefix\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_inside_v_for_with_whitespace() {
    // v-if inside v-for with whitespace between branches
    let code = compile_and_validate_template(
        r#"<template><div><template v-for="item in items" :key="item.id"><span v-if="item.show">{{ item.name }}</span><span v-else>hidden</span></template></div></template>"#,
    );
    assert!(
        code.contains("_renderList"),
        "Should contain _renderList\n{}",
        code
    );
}

/// @ai-generated - Regression test for playground AnalysisPanel.vue build failure.
/// When a standalone v-if element (no v-else) is followed by another sibling,
/// the scope_close suffix and sibling comma are both prepended at the same
/// position (element's end). With sort_unstable_by_key, the ordering is not
/// guaranteed, producing `, : _createCommentVNode(...)` instead of the
/// correct `) : _createCommentVNode(...), `.
#[test]
pub(super) fn v_if_followed_by_sibling_valid_js() {
    // Minimal case: v-if without v-else, followed by a sibling
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="show">yes</span><p>after</p></div></template>"#,
    );
    assert!(
        code.contains(") : _createCommentVNode(\"v-if\", true), "),
        "scope_close should come before sibling comma\n{}",
        code
    );
}

#[test]
pub(super) fn nested_v_if_chains_no_overlap() {
    // Nested v-if chains should produce valid JS without panicking
    let code = compile_and_validate_template(
        "<template><div><div v-if=\"a\"><span v-if=\"b\">B</span><span v-else>C</span></div><div v-else>D</div></div></template>",
    );
    assert!(
        code.contains("function render("),
        "Should produce valid render function\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_with_comment_between_branches() {
    // HTML comment between v-if branches should be stripped
    let code = compile_and_validate_template(
        "<template><div><span v-if=\"a\">A</span><!-- comment --><span v-else>B</span></div></template>",
    );
    assert!(
        code.contains("function render("),
        "Should produce valid render function\n{}",
        code
    );
}

// ==================== v-if binding resolution ====================

#[test]
fn v_if_condition_has_setup_prefix_simple_ident() {
    // v-if="show" where `show` is a setup binding should emit $setup.show
    let code = compile_and_validate_template(
        r#"<script setup>
const show = ref(true)
</script>
<template><div><span v-if="show">yes</span></div></template>"#,
    );
    assert!(
        code.contains("$setup.show"),
        "v-if condition should use $setup. prefix for setup binding\n{}",
        code
    );
    assert!(
        !code.contains("(show)"),
        "v-if condition should not use bare identifier without prefix\n{}",
        code
    );
}

#[test]
fn v_if_condition_has_setup_prefix_member_expr() {
    // v-if="store.loading" where `store` is a setup binding should emit $setup.store.loading
    let code = compile_and_validate_template(
        r#"<script setup>
const store = useStore()
</script>
<template><div><span v-if="store.loading">loading...</span></div></template>"#,
    );
    assert!(
        code.contains("$setup.store"),
        "v-if member expression should use $setup. prefix for root identifier\n{}",
        code
    );
}

#[test]
fn v_for_iterable_has_setup_prefix() {
    // v-for="item in items" where `items` is a setup binding should emit $setup.items
    let code = compile_and_validate_template(
        r#"<script setup>
const items = ref([1, 2, 3])
</script>
<template><div><span v-for="item in items" :key="item">{{ item }}</span></div></template>"#,
    );
    assert!(
        code.contains("$setup.items"),
        "v-for iterable should use $setup. prefix for setup binding\n{}",
        code
    );
}

#[test]
fn vapor_v_if_v_else_produces_valid_js() {
    // v-if/v-else must produce correct _createIf(cond, ifBranch, elseBranch) structure
    let code = compile_and_validate_vapor_template(
        r#"<template><span v-if="ok">yes</span><span v-else>no</span></template>"#,
    );
    assert!(
        code.contains("_createIf"),
        "Should contain _createIf\n{}",
        code
    );
}

#[test]
fn vapor_v_if_v_else_if_v_else_produces_valid_js() {
    let code = compile_and_validate_vapor_template(
        r#"<template><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></template>"#,
    );
    assert!(
        code.contains("_createIf"),
        "Should contain _createIf\n{}",
        code
    );
}

#[test]
fn vapor_v_show_compound_expr() {
    let code = compile_and_validate_vapor_template(
        r#"<template><div v-show="isAdmin && visible">hi</div></template>"#,
    );
    assert!(
        code.contains("_ctx.isAdmin"),
        "v-show compound expression should prefix isAdmin\n{}",
        code
    );
    assert!(
        code.contains("_ctx.visible"),
        "v-show compound expression should prefix visible\n{}",
        code
    );
}

#[test]
fn vapor_v_if_compound_condition() {
    let code =
        compile_and_validate_vapor_template(r#"<template><div v-if="a && b">hi</div></template>"#);
    assert!(
        code.contains("_ctx.a && _ctx.b"),
        "v-if compound condition should prefix both identifiers\n{}",
        code
    );
}

#[test]
fn vapor_v_html_compound_expr() {
    let code = compile_and_validate_vapor_template(
        r#"<template><div v-html="getHtml(data)"></div></template>"#,
    );
    assert!(
        code.contains("_ctx.getHtml"),
        "v-html compound expression should prefix getHtml\n{}",
        code
    );
    assert!(
        code.contains("_ctx.data"),
        "v-html compound expression should prefix data\n{}",
        code
    );
}

#[test]
pub(super) fn template_v_if_renders_as_fragment() {
    // <template v-if> should render as _Fragment, not as "template" element
    let result = compile_sfc(
        r#"<script setup lang="ts">
const show = ref(true)
</script>

<template>
  <div>
<template v-if="show">
  <span>a</span>
  <span>b</span>
</template>
  </div>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should contain _Fragment (not "template")
    assert!(
        tpl.code.contains("_Fragment"),
        "template v-if should render as Fragment.\nOutput:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("\"template\""),
        "should NOT render 'template' as element tag.\nOutput:\n{}",
        tpl.code
    );
    // Should have STABLE_FRAGMENT patch flag
    assert!(
        tpl.code.contains("64"),
        "should have STABLE_FRAGMENT patch flag.\nOutput:\n{}",
        tpl.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &tpl.code, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        tpl.code,
        parsed.diagnostics
    );
}

#[test]
pub(super) fn template_v_for_with_v_if_children_renders_as_fragment() {
    // <template v-for> with v-if/v-else children should produce valid JS
    // Start with simplest failing case and bisect
    let result = compile_sfc(
        r#"<script setup lang="ts">
const items = ref([])
</script>

<template>
  <div>
<template v-for="item in items" :key="item.id">
  <span v-if="item.visible">{{ item.text }}</span>
  <MyCard v-else>
    <div class="flex">
      <span>{{ item.label }}</span>
      <template v-if="item.show">
        <Foo v-if="item.a" />
        <Bar v-else />
      </template>
    </div>
    <div
      :class="[
        'flex items-center',
        {
          'line-through':
            item.id === 'apr' && isLBP(pool.poolType),
        },
      ]"
    >
      <span :class="{ 'mr-2': item.tooltip }">{{ item.value }}</span>
      <BalTooltip v-if="item.tooltip" :text="item.tooltip">
        <template #activator>
          <BalIcon name="info" size="sm" class="text-gray-400" />
        </template>
      </BalTooltip>
    </div>
  </MyCard>
</template>
  </div>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should contain _Fragment and _renderList (v-for + Fragment)
    assert!(
        tpl.code.contains("_Fragment"),
        "template v-for should render as Fragment.\nOutput:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_renderList"),
        "template v-for should use _renderList.\nOutput:\n{}",
        tpl.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &tpl.code, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        tpl.code,
        parsed.diagnostics
    );
}

/// @ai-generated - force_js should strip TS from v-bind directive expressions
#[test]
fn force_js_strips_ts_from_v_bind_expression() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const cls = 'active'
</script>
<template><div :class="(cls as string)">hello</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("as string"),
        "force_js should strip 'as string' from v-bind expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip TS from v-if directive expressions
#[test]
fn force_js_strips_ts_from_v_if_expression() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const condition: boolean | null = true
</script>
<template><div v-if="(condition as boolean)">visible</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("as boolean"),
        "force_js should strip 'as boolean' from v-if expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — HTML comment between v-if branches must not leak into generated JS
/// when comments are disabled (production mode). Regression: interstitial comments were
/// skipped in visit_comment but not overwritten, and build_child_records excluded them
/// when options.comments=false, so strip_interstitial_condition_nodes couldn't find them.
#[test]
pub(super) fn comment_between_v_if_branches_does_not_leak_in_prod() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        is_production: true, // comments=false (default is !is_production)
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };

    // Test 1: comment between v-if branches inside a parent element
    let result = compile(
        r#"<template><div><span v-if="a">A</span><!-- interstitial --><span v-else-if="b">B</span><!-- another --><span v-else>C</span></div></template>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    let js_alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&js_alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template JS parse error (nested): {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    assert!(
        !tpl.code.contains("<!--"),
        "HTML comment in nested case:\n{}",
        tpl.code
    );

    // Test 2: comment between v-if branches at template root level
    let alloc2 = Allocator::new();
    let result2 = compile(
        r#"<template><span v-if="a">A</span><!-- root interstitial --><span v-else-if="b">B</span><!-- root another --><span v-else>C</span></template>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc2,
    );
    assert!(
        result2.errors.is_empty(),
        "compile errors: {:?}",
        result2.errors
    );
    let tpl2 = result2.template.as_ref().expect("template block");
    let js_alloc2 = Allocator::new();
    let wrapped2 = format!("import {{ }} from \"vue\";\n{}", tpl2.code);
    let parsed2 = verter_parser::oxc_parse::Parser::new(&js_alloc2, &wrapped2, source_type).parse();
    assert!(
        parsed2.diagnostics.is_empty(),
        "Template JS parse error (root): {:?}\n--- generated code ---\n{}",
        parsed2
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl2.code
    );
    assert!(
        !tpl2.code.contains("<!--"),
        "HTML comment at root level:\n{}",
        tpl2.code
    );
}

/// @ai-generated — Destructured prop in v-bind attribute
#[test]
pub(super) fn destructured_prop_in_v_bind() {
    let result = compile_sfc(
        r#"<template><div :class="color"></div></template>
<script setup lang="ts">const { color } = defineProps<{ color: string }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.color"),
        "destructured prop in v-bind should resolve to $props.color, got:\n{}",
        tpl.code
    );
}

#[test]
fn vue_js_ide_carrier_preserves_authored_check_directives_at_the_generated_header() {
    for directive in ["@ts-check", "@ts-nocheck"] {
        let source = format!(
            "<script setup>\n// {directive}\n/** @param {{PointerEvent}} event */\nfunction handlePointer(event) {{\n  return event.__verterMissingPointerMember;\n}}\n</script>\n<template><button @pointerdown=\"handlePointer\">Check</button></template>\n"
        );
        let result = compile_tsx_with_force_js(&source, true);
        let tsx = result.tsx.expect("Vue JS IDE carrier");
        let expected = format!("// {directive}\n/** @jsxImportSource vue */\n");

        assert!(tsx.is_jsx);
        assert!(
            tsx.code.starts_with(&expected),
            "the authored file-check pragma must lead the generated JSX carrier:\n{}",
            tsx.code
        );
    }
}

#[test]
fn tsx_template_ref_ref_variable_inside_v_for_becomes_array_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const itemRef = ref()
</script>
<template>
  <div v-for="item in items" :key="item" ref="itemRef"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const itemRef = ref<") && tsx.code.contains("[]|null>()"),
        "Expected ref() inside v-for scope to infer array element type, got:\n{}",
        tsx.code
    );
}

/// Adapted parity matrix for:
/// - template/plugins/directive/directive.spec.ts
#[test]
fn tsx_directive_v5_process_parity_matrix() {
    v_model_on_component_expands_to_props();
    v_model_named_on_component();
    v_model_on_unresolved_component();
    v_model_with_explicit_update_handler_merges_into_array();
    v_model_named_with_explicit_update_handler_merges_into_array();
    v_model_on_native_input_generates_with_directives();
    v_model_on_textarea_generates_with_directives();
    v_model_on_select_generates_with_directives();
    v_model_on_checkbox_generates_with_directives();
    v_model_on_radio_generates_with_directives();
    v_model_on_input_with_trim_modifier();
    v_model_on_dynamic_type_input_uses_dynamic();

    event_modifier_prevent_uses_with_modifiers();
    event_modifier_stop_prevent_combined();
    event_modifier_capture_goes_into_key();
    event_modifier_once_goes_into_key();
    event_modifier_passive_goes_into_key();
    event_modifier_keyup_enter_uses_with_keys();
    event_modifier_empty_handler_with_prevent();
    event_modifier_prevent_only_no_value();
    event_modifier_on_component_generates_import();

    duplicate_event_handlers_same_event_merged_into_array();
    multiple_event_handlers_same_event_merged_into_array();
    different_option_modifiers_produce_different_keys();
    key_modifiers_same_event_merged();
    mixed_duplicate_and_unique_events();
    single_event_handler_no_merge();
    mouse_left_right_as_runtime_modifiers_merged();
    handler_with_mixed_key_and_runtime_modifiers_merged();
    v_on_and_v_bind_on_same_event_merged();
    dynamic_event_names_not_merged();

    static_style_compiled_to_object();
    static_style_multiple_properties();
    static_and_dynamic_class_merged_into_single_prop();
    data_and_aria_attributes_not_camelized();
    literal_boolean_in_bind_no_ctx_prefix();
    html_entities_in_bind_value_decoded();
    test_vbind_template_literal_with_html_entities();
}

#[test]
fn tsx_v_for_with_index_and_destructure_params() {
    let result = compile_tsx(
        r#"<template>
<div v-for="(item, index) in items">{{ item + index }}</div>
<div v-for="({obj}, key, index) of items">{{ obj + key + index }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("{ const [item, index] = ___VERTER___flowEach2(___VERTER___v0);"),
        "v-for with (item, index) should preserve both aliases, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("{ const [{obj}, key, index] = ___VERTER___flowEach3(___VERTER___v1);"),
        "v-for with destructured value/key/index aliases should be preserved, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_on_template_tag_uses_fragment_children() {
    let result = compile_tsx(
        r#"<template><template v-for="item in items"><li>{{ item.msg }}</li><li class="divider" role="presentation"></li></template></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{ const item") && tsx.code.contains("); return ("),
        "template v-for should compile to a frame over item with statement body, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<><li></li>{ item.msg }</>")
            || tsx.code.contains("<><li></li>{ _ctx.item.msg }</>"),
        "template v-for branch should render li child content, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("</>); } })()"),
        "template v-for branch should close with fragment + statement-body syntax, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_v_for_with_v_if_combination_contains_condition_and_map() {
    let result =
        compile_tsx(r#"<template><li v-for="item in items" v-if="item.active"></li></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{ const item") && tsx.code.contains("); return ("),
        "v-for branch should still emit a frame with statement body when combined with v-if, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("item.active ?") || tsx.code.contains("_ctx.item.active ?"),
        "v-if condition should be emitted as lifted ternary for v-for + v-if, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_parent_v_if_with_child_v_for_contains_outer_condition() {
    let result = compile_tsx(
        r#"<script setup>
const show = true
const items = [1]
</script>
<template><div v-if="show"><div v-for="item in items">{{ item }}</div></div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("if(show)") || tsx.code.contains("if(_ctx.show)"),
        "Parent v-if should emit IIFE if-block, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ const item") && tsx.code.contains("); return ("),
        "Child v-for under parent v-if should still emit a frame with statement body, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_component_with_v_if_and_v_for_preserves_component_tags() {
    let result = compile_tsx(
        r#"<script setup>
const test = true
const items = [1]
</script>
<template>
  <Comp v-if="test"></Comp>
  <Comp v-for="item in items"></Comp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("if(test)") || tsx.code.contains("if(_ctx.test)"),
        "Component with v-if should keep Comp tag in IIFE if-block, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<Comp"),
        "Component tag should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ const item") && tsx.code.contains("); return (<Comp"),
        "Component with v-for should keep Comp tag inside the frame statement body, got:\n{}",
        tsx.code
    );
}

/// @ai-generated — TSX source map: v-if directive expression maps back
#[test]
fn tsx_sourcemap_v_if_directive() {
    let source = r#"<script setup>
const show = true
</script>

<template>
  <div v-if="show">visible</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: v-for directive maps back
#[test]
fn tsx_sourcemap_v_for_directive() {
    let source = r#"<script setup>
const items = [1, 2, 3]
</script>

<template>
  <div v-for="item in items" :key="item">{{ item }}</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: compound v-if with operators in tail maps correctly.
///
/// Regression test: when a v-if expression has bindings resolved by OXC, the
/// "tail" text after the last binding (e.g., ` && 1 ===2`) was emitted unmapped.
/// This test verifies that ALL positions in the template expression — including
/// non-binding positions like `===` — can be mapped back to the correct Vue SFC
/// positions via the source map.
#[test]
fn tsx_sourcemap_v_if_expression_tail_maps_correctly() {
    let source = r#"<script lang="ts" setup>
let isLoggedIn = false;
let hasPermission = false;

function onclick() {
  isLoggedIn = true
}

if ( 1 === 2 ) {

}

</script>

<template>
  <div v-if="isLoggedIn && hasPermission && 1 ===2">Full  {{isLoggedIn}}</div>
  <div v-else-if="isLoggedIn && !hasPermission">Limited Access</div>
  <div v-else>No Access</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Helper: assert that a TSX position maps to the expected Vue (line, col)
    let assert_tsx_maps_to_vue = |tsx_line: u32,
                                  tsx_col: u32,
                                  expected_vue_line: u32,
                                  expected_vue_col: u32,
                                  label: &str| {
        let token = sm.lookup_token(&lookup, tsx_line, tsx_col);
        assert!(
            token.is_some(),
            "[{label}] No source map token at TSX {tsx_line}:{tsx_col}\nTSX:\n{}",
            tsx.code
        );
        let token = token.unwrap();
        assert!(
            token.get_source_id().is_some(),
            "[{label}] Token at TSX {tsx_line}:{tsx_col} has no source mapping (unmapped)\nTSX:\n{}",
            tsx.code
        );

        // Compute the mapped-back Vue position using the same interpolation as tsx_to_vue
        let vue_line = token.get_src_line();
        let mut vue_col = token.get_src_col();
        if token.get_dst_line() == tsx_line && tsx_col > token.get_dst_col() {
            vue_col += tsx_col - token.get_dst_col();
        }

        assert_eq!(
            vue_line, expected_vue_line,
            "[{label}] TSX {tsx_line}:{tsx_col} mapped to Vue line {vue_line}, expected {expected_vue_line}\nTSX:\n{}",
            tsx.code
        );
        assert_eq!(
            vue_col, expected_vue_col,
            "[{label}] TSX {tsx_line}:{tsx_col} mapped to Vue col {vue_col}, expected {expected_vue_col}\nTSX:\n{}",
            tsx.code
        );
    };

    // ── Script body positions (should map to original Vue positions) ──

    // `let isLoggedIn` in script: Vue line 1, col 0
    let (vue_let_line, vue_let_col) = find_line_col(source, "let isLoggedIn");
    let (tsx_let_line, tsx_let_col) = find_line_col(&tsx.code, "let isLoggedIn");
    assert_tsx_maps_to_vue(
        tsx_let_line,
        tsx_let_col,
        vue_let_line,
        vue_let_col,
        "let isLoggedIn in script",
    );

    // `===` in script's `if (1 === 2)`: should map to Vue line 8
    // Find === that's in the script (first occurrence), not the template
    let (vue_script_eq_line, vue_script_eq_col) = find_line_col(source, "1 === 2");
    let vue_script_eq_col = vue_script_eq_col + 2; // point at ===, not 1
    let (tsx_script_eq_line, tsx_script_eq_col) = find_line_col(&tsx.code, "1 === 2");
    let tsx_script_eq_col = tsx_script_eq_col + 2; // point at ===
    assert_tsx_maps_to_vue(
        tsx_script_eq_line,
        tsx_script_eq_col,
        vue_script_eq_line,
        vue_script_eq_col,
        "=== in script if-statement",
    );

    // ── Template expression positions ──

    // Find the template v-if expression in TSX output.
    // In TSX it becomes: if(isLoggedIn && hasPermission && 1 ===2)
    let tsx_v_if = tsx
        .code
        .find("if(isLoggedIn && hasPermission && 1 ===2)")
        .expect("TSX should contain v-if condition expression");

    // `isLoggedIn` in v-if expression: should map to Vue line 15, within the v-if attribute
    // Find the exact column of "isLoggedIn" inside v-if attribute value
    let vif_attr_start = source.find(r#"v-if="isLoggedIn"#).unwrap() + 6; // skip v-if="
    let (vue_vif_islogged_line, vue_vif_islogged_col) = find_line_col_at(source, vif_attr_start);

    let tsx_vif_islogged_offset = tsx.code[tsx_v_if..].find("isLoggedIn").unwrap() + tsx_v_if;
    let (tsx_vif_islogged_line, tsx_vif_islogged_col) =
        find_line_col_at(&tsx.code, tsx_vif_islogged_offset);
    assert_tsx_maps_to_vue(
        tsx_vif_islogged_line,
        tsx_vif_islogged_col,
        vue_vif_islogged_line,
        vue_vif_islogged_col,
        "isLoggedIn in template v-if",
    );

    // `hasPermission` in v-if: should map to same Vue line
    let tsx_vif_hasperm_offset = tsx.code[tsx_v_if..].find("hasPermission").unwrap() + tsx_v_if;
    let (tsx_vif_hasperm_line, tsx_vif_hasperm_col) =
        find_line_col_at(&tsx.code, tsx_vif_hasperm_offset);

    let vue_vif_hasperm_offset =
        source[vif_attr_start..].find("hasPermission").unwrap() + vif_attr_start;
    let (vue_vif_hasperm_line, vue_vif_hasperm_col) =
        find_line_col_at(source, vue_vif_hasperm_offset);
    assert_tsx_maps_to_vue(
        tsx_vif_hasperm_line,
        tsx_vif_hasperm_col,
        vue_vif_hasperm_line,
        vue_vif_hasperm_col,
        "hasPermission in template v-if",
    );

    // `===` in v-if tail (the KEY regression test):
    // The tail " && 1 ===2" after the last binding was previously unmapped.
    // The `===` must map back to its Vue position within the v-if attribute.
    let tsx_vif_eq_offset = tsx.code[tsx_v_if..].find("1 ===2").unwrap() + tsx_v_if + 2; // +2 to point at ===
    let (tsx_vif_eq_line, tsx_vif_eq_col) = find_line_col_at(&tsx.code, tsx_vif_eq_offset);

    let vue_vif_eq_offset = source[vif_attr_start..].find("1 ===2").unwrap() + vif_attr_start + 2;
    let (vue_vif_eq_line, vue_vif_eq_col) = find_line_col_at(source, vue_vif_eq_offset);
    assert_tsx_maps_to_vue(
        tsx_vif_eq_line,
        tsx_vif_eq_col,
        vue_vif_eq_line,
        vue_vif_eq_col,
        "=== in template v-if tail",
    );

    // `isLoggedIn` in {{isLoggedIn}} interpolation
    let interp_search = r#"{"Full"}"#;
    let tsx_interp_area = tsx.code.find(interp_search).unwrap();
    let tsx_interp_islogged =
        tsx.code[tsx_interp_area..].find("isLoggedIn").unwrap() + tsx_interp_area;
    let (tsx_interp_line, tsx_interp_col) = find_line_col_at(&tsx.code, tsx_interp_islogged);

    let vue_interp_islogged = source.find("{{isLoggedIn}}").unwrap() + 2; // skip {{
    let (vue_interp_line, vue_interp_col) = find_line_col_at(source, vue_interp_islogged);
    assert_tsx_maps_to_vue(
        tsx_interp_line,
        tsx_interp_col,
        vue_interp_line,
        vue_interp_col,
        "isLoggedIn in {{interpolation}}",
    );
}

/// @ai-generated — Binding occurrences with v-if and v-for expressions
#[test]
fn binding_occurrence_spans_directives() {
    let source = r#"<script setup>
const show = true
const items = [1, 2]
</script>

<template>
  <div v-if="show">
    <span v-for="item in items" :key="item">{{ item }}</span>
  </div>
</template>
"#;
    let result = compile_tsx_with_template_data(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tpl_data = result.template_data.as_ref().expect("template data");
    for occ in &tpl_data.binding_occurrences {
        let start = occ.span.start as usize;
        let end = occ.span.end as usize;
        if end > source.len() {
            panic!(
                "Binding '{}' span {}..{} exceeds source length {}",
                occ.name,
                start,
                end,
                source.len()
            );
        }
        let slice = &source[start..end];
        assert_eq!(
            slice, occ.name,
            "Binding '{}' span {}..{} contains '{}' instead",
            occ.name, start, end, slice
        );
    }
}

/// @ai-generated — E2E: v-if with defineProps and whitespace between elements
/// Verifies both __props declaration and valid IIFE chain in full TSX output
#[test]
fn tsx_v_if_with_define_props_and_whitespace() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "render",
            false,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_tsx_with_runtime(
        r#"<script setup lang="ts">
const props = defineProps<{ render: 'svg' | 'img' }>()
</script>
<template>
  <img v-if="render === 'svg'" class="icon" />
  <span v-else>fallback</span>
</template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Bug 2 fix: __props must be declared so TSGO can resolve it
    assert!(
        tsx.code.contains("const __props = "),
        "__props alias must be declared in TSX output, got:\n{}",
        tsx.code
    );

    // Bug 1 fix: the v-if/v-else chain must be a single valid IIFE, not split by whitespace
    assert!(
        tsx.code.contains("else{"),
        "v-if/v-else must be in a single IIFE chain, got:\n{}",
        tsx.code
    );

    // Negative: Vue directives must not leak into TSX
    assert!(
        !tsx.code.contains("v-if"),
        "v-if attribute must be removed from TSX, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("v-else"),
        "v-else attribute must be removed from TSX, got:\n{}",
        tsx.code
    );

    // Verify the template IIFE region is valid JSX by extracting and parsing it.
    // (Full TSX validation is not possible here because the return object uses
    // `as unknown as typeof x` casts that OXC's parser rejects.)
    let iife_start = tsx.code.find("{(()=>{if(").expect("IIFE should exist");
    let iife_end = tsx.code[iife_start..].find("}})()}").expect("IIFE close") + iife_start + 6;
    let iife_region = &tsx.code[iife_start..iife_end];
    // Wrap in a JSX expression context for parsing
    let wrapper = format!("const x = <>{}</>", iife_region);
    let alloc = oxc_allocator::Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapper, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "IIFE region has syntax errors: {:?}\n--- IIFE ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        iife_region
    );
}

/// @ai-generated — Element with v-if is NOT hoisted
#[test]
fn static_hoist_v_if_not_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div v-if="show">hello</div></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "element with v-if should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// A `v-if`/`v-else` branch root with NO user props (just the synthetic
/// branch `key`) still hoists its `{ key: N }` props object to a
/// `_hoisted_N` constant, exactly like a static class/attrs object. Official
/// `@vue/compiler-core`'s `hoistStatic` transform treats the injected key
/// property the same as any other fully-static props object — it is not
/// exempted just because it originated from `injectProp` rather than an
/// authored attribute. Verter's `process_element_leave` special-cased the
/// no-other-props branch (`injected_key.is_some()` with `has_props ==
/// false`) as a separate code path that always inlined the key object,
/// bypassing `can_hoist_props` entirely — this is the whole-object hoist
/// mechanism (`_hoisted_N` consts), a DIFFERENT optimization from the
/// whole-subtree `_createStaticVNode`/`_cache[N]` mechanism the sibling
/// tests above cover.
#[test]
fn static_hoist_v_if_branch_key_object_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><p v-if="count > 0">{{ count }}</p><p v-else>zero</p></div></template>"#,
    );
    assert!(
        code.contains("const _hoisted_1 = { key: 0 }"),
        "v-if branch's synthetic key object should hoist to _hoisted_1\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("const _hoisted_2 = { key: 1 }"),
        "v-else branch's synthetic key object should hoist to _hoisted_2\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("{ key: 0 }") || code.matches("{ key: 0 }").count() == 1,
        "the inline `{{ key: 0 }}` object should appear only once — in the \
         hoisted const declaration itself, not again inline in the branch\n\
         --- code ---\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_1)") || code.contains("_hoisted_1,"),
        "the v-if branch should reference the hoisted const, not inline the key object\n\
         --- code ---\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_2)") || code.contains("_hoisted_2,"),
        "the v-else branch should reference the hoisted const, not inline the key object\n\
         --- code ---\n{}",
        code
    );
}

#[test]
fn tsx_parse_valid_v_if_v_for_combined() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const show = ref(true)
const items = ref([{ id: 1, name: 'a' }, { id: 2, name: 'b' }])
</script>
<template>
  <div v-if="show">
    <ul>
      <li v-for="item in items" :key="item.id">
        <span v-if="item.name">{{ item.name }}</span>
        <span v-else>unnamed</span>
      </li>
    </ul>
  </div>
  <div v-else>hidden</div>
</template>"#,
        "v-if + v-for nested",
    );
}

#[test]
fn tsx_parse_valid_v_html_v_text() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const html = ref('<b>bold</b>')
const text = ref('plain')
</script>
<template>
  <div v-html="html"></div>
  <div v-text="text"></div>
</template>"#,
        "v-html and v-text",
    );
}

#[test]
fn tsx_parse_valid_v_bind_dynamic() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const attrs = ref({ id: 'foo', class: 'bar' })
</script>
<template>
  <div v-bind="attrs">content</div>
</template>"#,
        "v-bind object spread",
    );
}

#[test]
fn tsx_parse_valid_v_show() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const visible = ref(true)
</script>
<template>
  <div v-show="visible">shown</div>
</template>"#,
        "v-show",
    );
}

#[test]
fn jsx_compile_v_if_v_for() {
    assert_jsx_parses(
        r#"<script setup>
import { ref } from 'vue'
const show = ref(true)
const items = ref([1, 2, 3])
</script>
<template>
  <div v-if="show">shown</div>
  <ul><li v-for="item in items" :key="item">{{ item }}</li></ul>
</template>"#,
        "JS SFC with v-if and v-for",
    );
}
