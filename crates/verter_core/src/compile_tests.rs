use super::*;

fn compile_sfc(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_sfc_vapor(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_and_validate_vapor_template(source: &str) -> String {
    let result = compile_sfc_vapor(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Vapor template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

#[test]
fn format_import_specifier_strips_underscore_prefix() {
    assert_eq!(
        format_import_specifier("_defineComponent"),
        "defineComponent as _defineComponent"
    );
    assert_eq!(
        format_import_specifier("_useSlots"),
        "useSlots as _useSlots"
    );
    assert_eq!(
        format_import_specifier("_Fragment"),
        "Fragment as _Fragment"
    );
}

#[test]
fn format_import_specifier_preserves_non_prefixed() {
    assert_eq!(format_import_specifier("vue"), "vue");
    assert_eq!(format_import_specifier("ref"), "ref");
}

#[test]
fn basic_sfc_compiles() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
    assert!(result.template.is_some());
}

#[test]
fn script_imports_use_as_syntax() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The import should use "defineComponent as _defineComponent" syntax
    // because Vue exports "defineComponent" (no underscore prefix).
    assert!(
        script.code.contains("defineComponent as _defineComponent"),
        "Expected 'defineComponent as _defineComponent' in imports, got: {}",
        script.code
    );
    assert!(
        !script.code.contains("import { _defineComponent }"),
        "Should not import bare _defineComponent, got: {}",
        script.code
    );
}

#[test]
fn style_block_extracted() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    assert!(result.styles[0].scoped);
    assert!(!result.scope_id.is_empty());
}

#[test]
fn custom_blocks_extracted() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<i18n lang="json">
{ "en": { "hello": "Hello" } }
</i18n>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.custom_blocks.len(), 1);
    assert_eq!(result.custom_blocks[0].block_type, "i18n");
}

#[test]
fn empty_input_no_panic() {
    let result = compile_sfc("");
    // No script or template, but should not panic
    assert!(result.script.is_none());
    assert!(result.template.is_none());
}

#[test]
fn template_output_contains_render_function_vdom() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("function render("),
        "Expected render function in template output, got: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<div>"),
        "Template output should not contain raw HTML: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<script"),
        "Template output should not contain script tags: {}",
        tpl.code
    );
}

#[test]
fn template_output_contains_render_function_vapor() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
        &options,
        &verter_opts,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("function render("),
        "Expected render function in template output, got: {}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_template("),
        "Expected _template() call in vapor output, got: {}",
        tpl.code
    );
    // Vapor legitimately has <div> inside _template("...") string literals,
    // so check there's no raw <div> OUTSIDE of string contexts.
    // A raw <div> would appear as a line starting with `<div>` or after whitespace.
    assert!(
        !tpl.code.contains("<script"),
        "Template output should not contain script tags: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<template>"),
        "Template output should not contain raw template tags: {}",
        tpl.code
    );
}

#[test]
fn scoped_css_no_double_data_v_prefix() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div class="app">{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    let css = &result.styles[0].code;
    assert!(
        !css.contains("data-v-data-v-"),
        "CSS should not contain double data-v- prefix: {}",
        css
    );
    assert!(
        css.contains("[data-v-"),
        "CSS should contain scoped attribute selector: {}",
        css
    );
}

#[test]
fn timing_fields_populated() {
    let result = compile_sfc(
        r#"<script setup>
const x = 1
</script>
<template><div>{{ x }}</div></template>
"#,
    );
    assert!(result.parse_duration_ms >= 0.0);
    assert!(result.total_duration_ms >= 0.0);
    if let Some(ref s) = result.script {
        assert!(s.duration_ms >= 0.0);
    }
    if let Some(ref t) = result.template {
        assert!(t.duration_ms >= 0.0);
    }
}

/// Compile and assert template output is syntactically valid JS.
/// Returns the template code string for further assertion.
fn compile_and_validate_template(source: &str) -> String {
    let result = compile_sfc(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Template JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

// ==================== v-if / v-else-if / v-else ====================

#[test]
fn v_if_only_emits_comment_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="show">yes</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "v-if without v-else should emit comment fallback\n{}",
        code
    );
}

#[test]
fn v_if_v_else_no_comment_fallback() {
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
fn v_if_v_else_if_no_v_else_emits_comment_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="a">A</span><span v-else-if="b">B</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "v-if/v-else-if without v-else should emit comment fallback\n{}",
        code
    );
}

#[test]
fn v_if_v_else_if_v_else_complete_chain() {
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
fn v_if_after_sibling_has_comma_separator() {
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
fn v_if_chain_after_sibling() {
    let code = compile_and_validate_template(
        r#"<template><div><p>text</p><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
    );
    // Should produce valid JS with comma before the ternary
    assert!(code.contains("function render("));
}

#[test]
fn v_if_chain_without_v_else_after_sibling() {
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
fn v_if_as_root_single_child() {
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
fn v_if_v_else_as_root() {
    let code = compile_and_validate_template(
        r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#,
    );
    assert!(code.contains("return "));
}

#[test]
fn v_if_in_multi_root_fragment() {
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
fn multiple_v_if_chains_in_same_parent() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="a">A</span><span v-else>notA</span><span v-if="b">B</span><span v-else>notB</span></div></template>"#,
    );
    // Two independent v-if/v-else chains in the same parent
    assert!(code.contains("function render("));
}

#[test]
fn v_if_with_whitespace_between_branches() {
    // Whitespace nodes between v-if/v-else should be skipped
    let code = compile_and_validate_template(
        "<template><div>\n  <span v-if=\"a\">A</span>\n  <span v-else>B</span>\n</div></template>",
    );
    assert!(code.contains("function render("));
}

#[test]
fn v_if_nested_inside_v_for() {
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

// ==================== v-if / whitespace overlap TDD tests ====================

#[test]
fn v_if_standalone_emits_comment_vnode() {
    // Standalone v-if without v-else should produce a comment fallback
    let code =
        compile_and_validate_template("<template><div><div v-if=\"show\">A</div></div></template>");
    assert!(
        code.contains("_createCommentVNode(\"v-if\", true)"),
        "Standalone v-if should emit comment vnode\n{}",
        code
    );
}

#[test]
fn v_if_else_chain_with_whitespace_valid_output() {
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
fn v_if_inside_v_for_with_whitespace() {
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
fn v_if_followed_by_sibling_valid_js() {
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

/// @ai-generated - Regression test using the actual AnalysisPanel.vue template
/// structure that triggered the playground build failure. This complex template
/// produces enough prepends to trigger sort_unstable_by_key reordering.
#[test]
fn analysis_panel_regression_valid_js() {
    let code = compile_and_validate_template(include_str!(
        "../../../packages/playground/src/output/AnalysisPanel.vue"
    ));
    assert!(
        !code.contains(",  : _createCommentVNode"),
        "comma should not appear before ternary colon\n{}",
        code
    );
}

#[test]
fn component_whitespace_children_clean_output() {
    // Component with whitespace-only children should not leak close tag
    let code = compile_and_validate_template(
        "<template><div><Comp :foo=\"bar\">\n  </Comp></div></template>",
    );
    assert!(
        !code.contains("</Comp>"),
        "Component close tag should not appear in output\n{}",
        code
    );
}

#[test]
fn nested_v_if_chains_no_overlap() {
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
fn v_if_with_comment_between_branches() {
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
fn v_else_if_condition_has_setup_prefix() {
    // v-else-if should also resolve bindings
    let code = compile_and_validate_template(
        r#"<script setup>
const a = ref(true)
const b = ref(false)
</script>
<template><div><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
    );
    assert!(
        code.contains("$setup.a"),
        "v-if condition should use $setup. prefix\n{}",
        code
    );
    assert!(
        code.contains("$setup.b"),
        "v-else-if condition should use $setup. prefix\n{}",
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

// ==================== Multi-statement event handlers ====================

#[test]
fn multi_statement_event_handler_wrapped() {
    let code = compile_and_validate_template(
        r#"<template><button @click="emit('x'); doStuff();">go</button></template>"#,
    );
    assert!(
        code.contains("$event => {"),
        "Multi-statement handler should be wrapped in $event => {{ ... }}\n{}",
        code
    );
}

#[test]
fn assignment_event_handler_wrapped() {
    // Assignment expressions in event handlers need $event => { ... } wrapping
    // to be valid as object literal values.
    let code = compile_and_validate_template(
        r#"<template><button @click="dialog = true">link</button></template>"#,
    );
    assert!(
        code.contains("$event => {"),
        "Assignment handler should be wrapped in $event => {{ ... }}\n{}",
        code
    );
}

#[test]
fn assignment_event_handler_with_modifiers_and_hash_href() {
    // @click.stop.prevent="dialog = true" on an <a href="#"> element.
    let code = compile_and_validate_template(
        r##"<template><a href="#" @click.stop.prevent="dialog = true">link</a></template>"##,
    );
    assert!(
        code.contains("_withModifiers"),
        "Assignment handler with modifiers should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains("$event => {"),
        "Assignment handler with modifiers should be wrapped\n{}",
        code
    );
}

#[test]
fn empty_string_event_handler_outputs_noop() {
    // @click.stop="" has an empty string value. It should produce a no-op
    // function, not an empty value in the object literal, wrapped in _withModifiers.
    let code =
        compile_and_validate_template(r#"<template><div @click.stop="">text</div></template>"#);
    assert!(
        code.contains("_withModifiers"),
        "Empty string handler with .stop should use _withModifiers\n{}",
        code
    );
}

// ==================== VDOM props binding resolution ====================

#[test]
fn vdom_props_apply_ctx_prefix_to_bindings() {
    // Directive prop values like :foo="message" and @click="handler" should
    // have _ctx. prefix applied to identifiers, just like interpolation does.
    let code = compile_and_validate_template(
        r#"<template><button :click="increment" @click="increment" :foo="message"></button></template>"#,
    );
    assert!(
        code.contains("_ctx.increment") || code.contains("$setup.increment"),
        "Directive prop values should have binding prefix applied\n{}",
        code
    );
    assert!(
        code.contains("_ctx.message") || code.contains("$setup.message"),
        "Directive prop values should have binding prefix applied\n{}",
        code
    );
}

#[test]
fn vdom_event_handler_applies_ctx_prefix() {
    let code = compile_and_validate_template(
        r#"<template><button @click="handleClick">go</button></template>"#,
    );
    assert!(
        code.contains("_ctx.handleClick") || code.contains("$setup.handleClick"),
        "Event handler identifier should have binding prefix\n{}",
        code
    );
}

// ==================== Shorthand property expansion ====================

#[test]
fn shorthand_property_expanded_when_prefixed() {
    // When a shorthand property `{ searchTerm }` gets its identifier rewritten
    // to `$setup.searchTerm`, it must be expanded to `{ searchTerm: $setup.searchTerm }`.
    let code = compile_and_validate_template(
        r#"<template><div>{{ t('msg', { searchTerm }) }}</div></template>"#,
    );
    assert!(
        code.contains("searchTerm: "),
        "Shorthand property should be expanded to key: value form\n{}",
        code
    );
}

// ==================== v-for in text context ====================

#[test]
fn vfor_after_text_in_multi_root_template() {
    // Text followed by v-for in a multi-root template: the text and v-for
    // should be separate children in the Fragment array, not combined.
    let code = compile_and_validate_template(
        r#"<template>Text <template v-for="item in items"><span>{{ item }}</span></template></template>"#,
    );
    assert!(
        code.contains("_createTextVNode"),
        "Should have text node\n{}",
        code
    );
    assert!(
        code.contains("_renderList"),
        "Should have render list\n{}",
        code
    );
}

// ==================== Text with entities and newlines ====================

#[test]
fn pre_code_with_entities_no_unterminated_string() {
    // Text inside <pre><code> with HTML entities and newlines should produce
    // valid JS. The newlines must be escaped in string literals.
    let code = compile_and_validate_template(
        "<template><pre><code>\n&lt;html dir=\"rtl\"&gt;\n</code></pre></template>",
    );
    assert!(code.contains("function render("));
    // Should not contain raw unescaped newlines inside string literals
    assert!(
        !code.contains("\"\n"),
        "Should not have raw newline after opening quote\n{}",
        code
    );
}

#[test]
fn script_attrs_contain_lang() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    eprintln!("attrs: {:?}", script.attrs);
    let lang = script.attrs.iter().find(|(k, _)| k == "lang");
    assert!(
        lang.is_some(),
        "Expected 'lang' in attrs, got: {:?}",
        script.attrs
    );
    assert_eq!(lang.unwrap().1, "ts");
}

#[test]
fn custom_block_with_html_like_content_no_errors() {
    // A <docs> block containing `Array<string>` should not cause parse
    // errors — the tokenizer enters RCDATA mode for custom SFC blocks.
    let result = compile_sfc(
        r#"<docs>
## Title

Default to `@`, `Array<string>` also supported.

</docs>
<template><div>hello</div></template>
<script setup>
const x = 1
</script>"#,
    );
    assert!(
        result.errors.is_empty(),
        "SFC with <docs> block should not have errors: {:?}",
        result.errors
    );
    assert_eq!(result.custom_blocks.len(), 1);
    assert_eq!(result.custom_blocks[0].block_type, "docs");
    assert!(
        result.custom_blocks[0].content.contains("Array<string>"),
        "Custom block content should be raw text"
    );
}

// ==================== Vapor mode tests ====================

#[test]
fn vapor_interpolation_with_call_expression() {
    // Reproduces: $t("key") → _ctx.$t"key" (missing parentheses)
    let code = compile_and_validate_vapor_template(
        r#"<template><div>{{ $t("hello.world") }}</div></template>"#,
    );
    // The call expression must preserve parentheses
    assert!(
        code.contains("$t("),
        "Call expression $t() must preserve parentheses\n{}",
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
fn vapor_component_with_dotted_name() {
    // Component names like Calendar.Root should produce valid variable names
    let code = compile_and_validate_vapor_template(
        r#"<template><Calendar.Root locale="en" /></template>"#,
    );
    assert!(
        code.contains("_component_Calendar_Root"),
        "Dotted component names should use underscores in variable\n{}",
        code
    );
}

#[test]
fn vapor_static_prop_with_newline() {
    // Static prop values with newlines must be escaped in the JS string
    let code = compile_and_validate_vapor_template(
        "<template><MyComp content=\"line1\nline2\" /></template>",
    );
    assert!(
        code.contains("\\n"),
        "Newlines in static prop values should be escaped\n{}",
        code
    );
}

#[test]
fn vapor_interpolation_shorthand_property_expanded() {
    // { total } with prefix → { total: _ctx.total }
    let code = compile_and_validate_vapor_template(
        r#"<template><div>{{ fn({ total }) }}</div></template>"#,
    );
    assert!(
        code.contains("total: _ctx.total"),
        "Shorthand properties should be expanded when prefixed\n{}",
        code
    );
}

#[test]
fn vapor_component_event_with_hyphen_camelcased() {
    // @popup-block → onPopupBlock
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @popup-block="handler" /></template>"#,
    );
    assert!(
        code.contains("onPopupBlock"),
        "Hyphenated event should be camelCased\n{}",
        code
    );
}

#[test]
fn vapor_event_with_multi_statement_handler() {
    // Multi-statement handlers with semicolons must be wrapped in { }
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @click="a = 1; b = 2" /></template>"#,
    );
    assert!(
        code.contains("() => {"),
        "Multi-statement handler should be wrapped in block\n{}",
        code
    );
}

#[test]
fn vapor_event_with_trailing_semicolon() {
    // Trailing semicolons should be stripped
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @click="doStuff();" /></template>"#,
    );
    assert!(
        !code.contains(";"),
        "Trailing semicolons should be stripped\n{}",
        code
    );
}

#[test]
fn vapor_component_event_with_colon_camelcased() {
    // @update:modelValue → onUpdateModelValue
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @update:modelValue="handler" /></template>"#,
    );
    assert!(
        code.contains("onUpdateModelValue"),
        "Colon event should be camelCased\n{}",
        code
    );
}

#[test]
fn vapor_component_with_hyphenated_props() {
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp clear-icon="close" :void-icon="icon" /></template>"#,
    );
    // Hyphenated prop names must be quoted in object literals
    assert!(
        code.contains("\"clear-icon\""),
        "Static hyphenated prop should be quoted\n{}",
        code
    );
    assert!(
        code.contains("\"void-icon\""),
        "Dynamic hyphenated prop should be quoted\n{}",
        code
    );
}

// ======================== OXC binding resolution tests ========================
// These test that compound expressions get proper _ctx. prefixing via OXC data,
// not just simple identifiers.

#[test]
fn vapor_component_prop_compound_expr() {
    let code =
        compile_and_validate_vapor_template(r#"<template><MyComp :title="a + b" /></template>"#);
    assert!(
        code.contains("_ctx.a + _ctx.b"),
        "Compound expression in component prop should prefix both identifiers\n{}",
        code
    );
}

#[test]
fn vapor_native_event_compound_expr() {
    let code = compile_and_validate_vapor_template(
        r#"<template><div @click="count++, emit('x')"></div></template>"#,
    );
    assert!(
        code.contains("_ctx.count"),
        "Compound event handler should prefix count\n{}",
        code
    );
    assert!(
        code.contains("_ctx.emit"),
        "Compound event handler should prefix emit\n{}",
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

// ===== Component resolution tests =====

#[test]
fn component_resolves_to_setup_binding() {
    let result = compile_sfc(
        r#"<template><div><Header :store="store" /></div></template>
<script setup>import Header from "./Header.vue"; const store = ref(1);</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$setup[\"Header\"]") || tpl.code.contains("$setup.Header"),
        "component should resolve to $setup[\"Header\"], got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("createVNode(\"Header\""),
        "component should NOT be a string literal, got:\n{}",
        tpl.code
    );
}

#[test]
fn component_kebab_case_resolves_to_pascal_setup_binding() {
    let result = compile_sfc(
        r#"<template><div><my-header /></div></template>
<script setup>import MyHeader from "./MyHeader.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$setup[\"MyHeader\"]") || tpl.code.contains("$setup.MyHeader"),
        "kebab-case component should resolve to PascalCase $setup binding, got:\n{}",
        tpl.code
    );
}

#[test]
fn type_based_define_props_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ store.loading }}</div></template>
<script setup lang="ts">import type { Store } from "./store"; const props = defineProps<{ store: Store }>();</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.store"),
        "type-based defineProps prop should resolve to $props.store, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.store"),
        "type-based defineProps prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

#[test]
fn unknown_component_uses_resolve_component() {
    let result = compile_sfc(
        r#"<template><div><UnknownComp /></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_resolveComponent(\"UnknownComp\")")
            || tpl.code.contains("resolveComponent(\"UnknownComp\")"),
        "unknown component should use _resolveComponent, got:\n{}",
        tpl.code
    );
}

#[test]
fn self_referencing_component_uses_maybe_self_reference() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("TokenBreakdown.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><TokenBreakdown /></div></template>
<script setup>const x = 1;</script>"#,
        &options,
        &verter_opts,
        &alloc,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code
            .contains("_resolveComponent(\"TokenBreakdown\", true)"),
        "recursive self-reference should use _resolveComponent(name, true), got:\n{}",
        tpl.code
    );
}

#[test]
fn self_referencing_component_kebab_case() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("TokenBreakdown.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><token-breakdown /></div></template>
<script setup>const x = 1;</script>"#,
        &options,
        &verter_opts,
        &alloc,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code
            .contains("_resolveComponent(\"token-breakdown\", true)"),
        "recursive self-reference (kebab-case) should use _resolveComponent(name, true), got:\n{}",
        tpl.code
    );
}

// ==================== v-model on components ====================

// @ai-generated - Tests v-model on components expands to prop + update handler
#[test]
fn v_model_on_component_expands_to_props() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model="val" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("modelValue:"),
        "v-model on component should emit modelValue prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:modelValue""#),
        "v-model on component should emit onUpdate:modelValue handler, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$event"),
        "v-model update handler should use $event, got:
{}",
        tpl.code
    );
}

#[test]
fn v_model_named_on_component() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model:title="pageTitle" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const pageTitle = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("title:"),
        "v-model:title should emit title prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:title""#),
        "v-model:title should emit onUpdate:title handler, got:
{}",
        tpl.code
    );
}

#[test]
fn v_model_on_unresolved_component() {
    let result = compile_sfc(
        r#"<template><div><BalTabs v-model="activeTab" :tabs="tabs" /></div></template>
<script setup>
const activeTab = ref('tab1')
const tabs = []
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("modelValue:"),
        "v-model on unresolved component should emit modelValue prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:modelValue""#),
        "v-model on unresolved component should emit onUpdate handler, got:
{}",
        tpl.code
    );
}

// @ai-generated - v-model with explicit @update:modelValue should merge into array
#[test]
fn v_model_with_explicit_update_handler_merges_into_array() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model="val" @update:model-value="handler" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
function handler(v) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Vue merges v-model + explicit @update handler into an array:
    //   "onUpdate:modelValue": [$event => ((val) = $event), handler]
    // Verter must NOT produce duplicate "onUpdate:modelValue" keys in the props object.
    let code = &tpl.code;
    // The merged value should be an array
    assert!(
        code.contains(r#""onUpdate:modelValue": ["#),
        "merged handler should be an array, got:\n{}",
        code
    );
    // Must NOT have two separate "onUpdate:modelValue": entries in the props object
    // (one from v-model, one from @update:model-value)
    let count = code.matches(r#""onUpdate:modelValue": "#).count();
    assert_eq!(
        count, 1,
        "should have exactly one onUpdate:modelValue: assignment (merged), got {} in:\n{}",
        count, code
    );
}

// @ai-generated - v-model:title with explicit @update:title should merge into array
#[test]
fn v_model_named_with_explicit_update_handler_merges_into_array() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model:title="pageTitle" @update:title="onTitleChange" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const pageTitle = ref('')
function onTitleChange(v) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains(r#""onUpdate:title": ["#),
        "merged handler should be an array, got:\n{}",
        code
    );
    let count = code.matches(r#""onUpdate:title": "#).count();
    assert_eq!(
        count, 1,
        "should have exactly one onUpdate:title: assignment (merged), got {} in:\n{}",
        count, code
    );
}

// ==================== v-model on native elements ====================

// @ai-generated - Tests v-model on native <input> generates withDirectives + onUpdate:modelValue
#[test]
fn v_model_on_native_input_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input v-model="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on native input should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelText"),
        "v-model on native input should use _vModelText directive, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""onUpdate:modelValue""#),
        "v-model on native input should emit onUpdate:modelValue handler, got:\n{}",
        code
    );
    assert!(
        code.contains("$event"),
        "v-model update handler should use $event assignment, got:\n{}",
        code
    );
}

// @ai-generated - v-model on <textarea> uses _vModelText
#[test]
fn v_model_on_textarea_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><textarea v-model="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on textarea should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelText"),
        "v-model on textarea should use _vModelText, got:\n{}",
        code
    );
}

// @ai-generated - v-model on <select> uses _vModelSelect
#[test]
fn v_model_on_select_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><select v-model="choice"><option>A</option></select></div></template>
<script setup>
const choice = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on select should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelSelect"),
        "v-model on select should use _vModelSelect, got:\n{}",
        code
    );
}

// @ai-generated - v-model on checkbox input uses _vModelCheckbox
#[test]
fn v_model_on_checkbox_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input type="checkbox" v-model="checked" /></div></template>
<script setup>
const checked = ref(false)
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on checkbox should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelCheckbox"),
        "v-model on checkbox should use _vModelCheckbox, got:\n{}",
        code
    );
}

// @ai-generated - v-model on radio input uses _vModelRadio
#[test]
fn v_model_on_radio_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input type="radio" v-model="picked" value="a" /></div></template>
<script setup>
const picked = ref('a')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on radio should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelRadio"),
        "v-model on radio should use _vModelRadio, got:\n{}",
        code
    );
}

// @ai-generated - v-model with .trim modifier generates modifier object in directive
#[test]
fn v_model_on_input_with_trim_modifier() {
    let result = compile_sfc(
        r#"<template><div><input v-model.trim="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model.trim should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("trim: true"),
        "v-model.trim should have modifier object with trim: true, got:\n{}",
        code
    );
}

// @ai-generated - v-model on dynamic input type uses _vModelDynamic
#[test]
fn v_model_on_dynamic_type_input_uses_dynamic() {
    let result = compile_sfc(
        r#"<template><div><input :type="inputType" v-model="val" /></div></template>
<script setup>
const inputType = ref('text')
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on dynamic type should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelDynamic"),
        "v-model on dynamic type input should use _vModelDynamic, got:\n{}",
        code
    );
}

// ==================== Component PatchFlags ====================

// @ai-generated - Components with dynamic props should emit PATCH_PROPS flag
#[test]
fn component_with_dynamic_prop_emits_patch_props() {
    let result = compile_sfc(
        r#"<template><div><MyComp :msg="val" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should have patch flag 8 (PROPS)
    assert!(
        tpl.code.contains("8 /* PROPS */") || tpl.code.contains(", 8,"),
        "component with dynamic prop should emit PATCH_PROPS (8), got:\n{}",
        tpl.code
    );
    // Should have dynamic props array
    assert!(
        tpl.code.contains(r#"["msg"]"#),
        "component with dynamic prop should list dynamic props, got:\n{}",
        tpl.code
    );
}

#[test]
fn component_with_default_slot_and_dynamic_props_emits_patch_flags() {
    let result = compile_sfc(
        r#"<template><div><MyComp :show="visible">content</MyComp></div></template>
<script setup>
import MyComp from './MyComp.vue'
const visible = ref(true)
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("8 /* PROPS */") || tpl.code.contains(", 8,"),
        "component with default slot and dynamic props should emit PATCH_PROPS, got:\n{}",
        tpl.code
    );
}

// ==================== v-for block scoping ====================

// @ai-generated - Native elements with v-for need their own block scope
#[test]
fn vfor_native_element_uses_block_scope() {
    let result = compile_sfc(
        r#"<template><div><div v-for="item in items" :key="item.id">{{ item.name }}</div></div></template>
<script setup>const items = ref([])</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_openBlock()") && tpl.code.contains("_createElementBlock("),
        "v-for native element should use (_openBlock(), _createElementBlock()), got:\n{}",
        tpl.code
    );
}

// ==================== Inline event handler wrapping ====================

// @ai-generated - Inline event handlers with function calls need $event => () wrapping
#[test]
fn inline_event_handler_gets_arrow_wrapping() {
    let result = compile_sfc(
        r#"<template><div><button @click="onClick(tab)">click</button></div></template>
<script setup>
const tab = ref('a')
function onClick(t) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$event => ("),
        "inline event handler call should be wrapped with $event => (), got:\n{}",
        tpl.code
    );
}

#[test]
fn member_expression_event_handler_not_wrapped() {
    let result = compile_sfc(
        r#"<template><div><button @click="onClick">click</button></div></template>
<script setup>function onClick() {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("$event =>"),
        "simple member expression event handler should NOT be wrapped, got:\n{}",
        tpl.code
    );
}

// ==================== Slot outlet ====================

// @ai-generated - TDD tests for slot outlet codegen
#[test]
fn slot_outlet_default_compiles_to_render_slot() {
    let result = compile_sfc(
        r#"<template><div><slot></slot></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_renderSlot(") && tpl.code.contains("$slots"),
        "<slot> should compile to _renderSlot($slots, ...), got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("\"default\""),
        "<slot> without name should use \"default\", got:\n{}",
        tpl.code
    );
}

#[test]
fn slot_outlet_named_compiles_to_render_slot() {
    let result = compile_sfc(
        r#"<template><div><slot name="header"></slot></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_renderSlot(") && tpl.code.contains("\"header\""),
        "<slot name=\"header\"> should compile to _renderSlot($slots, \"header\"), got:\n{}",
        tpl.code
    );
}

#[test]
fn slot_outlet_self_closing() {
    let result = compile_sfc(
        r#"<template><div><slot /></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_renderSlot(") && tpl.code.contains("$slots"),
        "self-closing <slot /> should compile to _renderSlot, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: slot outlet with v-if gets ternary wrapping
#[test]
fn slot_outlet_with_v_if_gets_ternary() {
    let result = compile_sfc(
        r#"<template><div><slot v-if="$slots.default"></slot></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The slot with v-if should produce a ternary:
    // ($slots.default) ? _renderSlot(...) : _createCommentVNode("v-if", true)
    assert!(
        tpl.code.contains("_renderSlot("),
        "<slot v-if> should compile to _renderSlot, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_createCommentVNode(\"v-if\", true)"),
        "<slot v-if> should have _createCommentVNode fallback, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: slot outlet with fallback content
#[test]
fn slot_outlet_with_fallback_children() {
    let result = compile_sfc(
        r#"<template><div><slot name="center"><span></span></slot></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Fallback should be passed as callback: _renderSlot(..., {}, () => [...])
    assert!(
        tpl.code.contains("_renderSlot("),
        "<slot> with fallback should use _renderSlot, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("() => ["),
        "<slot> with fallback should have fallback callback, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("\"center\""),
        "named slot should use \"center\", got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: slot outlet with v-for gets renderList wrapping
#[test]
fn slot_outlet_with_v_for_gets_render_list() {
    let result = compile_sfc(
        r#"<template><div><slot :item="item" v-for="item in list"></slot></div></template>
<script setup>const list = [1,2,3];</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The slot with v-for should produce _renderList wrapping
    assert!(
        tpl.code.contains("_renderList("),
        "<slot v-for> should have _renderList wrapping, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_renderSlot("),
        "<slot v-for> should contain _renderSlot, got:\n{}",
        tpl.code
    );
}

// ==================== Named slots on component ====================

// @ai-generated - TDD tests for component named slot codegen
#[test]
fn component_named_slots_compiled_as_slot_object() {
    let result = compile_sfc(
        r#"<template><Comp><template #header><div>head</div></template><template #footer><span>foot</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Named slots should be passed as a slots object, not createBaseVNode("template", ...)
    assert!(
        !tpl.code.contains("\"template\""),
        "named slots should NOT compile to createBaseVNode(\"template\"), got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("header:") && tpl.code.contains("footer:"),
        "named slots should produce slot function keys (header:, footer:), got:\n{}",
        tpl.code
    );
}

#[test]
fn component_default_slot_compiled_as_slot_object() {
    let result = compile_sfc(
        r#"<template><Comp><div>content</div></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Default slot content should be passed as the default slot function
    assert!(
        tpl.code.contains("default:"),
        "implicit default slot should produce default: slot function, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: component with whitespace-only children doesn't leak close tag
#[test]
fn component_whitespace_only_children_no_close_tag_leak() {
    let result = compile_sfc(
        r#"<template><div><Comp :foo="bar">
  </Comp></div></template>
<script setup>import Comp from "./Comp.vue"; const bar = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The close tag </Comp> must NOT appear in the output
    assert!(
        !tpl.code.contains("</Comp>"),
        "component close tag should not leak into JS output, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_createVNode("),
        "should have _createVNode call, got:\n{}",
        tpl.code
    );
}

// ==================== Conditional slots (_createSlots) ====================

// @ai-generated - TDD test: conditional slot with v-if uses _createSlots
#[test]
fn conditional_slot_v_if_uses_create_slots() {
    let result = compile_sfc(
        r#"<template><Comp><template #header>Head</template><template #footer v-if="show">Foot</template></Comp></template>
<script setup>import Comp from "./Comp.vue"; const show = true;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_createSlots("),
        "conditional slot should use _createSlots, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_: 2"),
        "conditional slot should have DYNAMIC flag (_: 2), got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("{ name: \"header\", fn:"),
        "header slot should be in dynamic format, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("{ name: \"footer\", fn:"),
        "footer slot should be in dynamic format, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(": undefined"),
        "v-if slot without v-else should have : undefined fallback, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: conditional slot v-if/v-else-if/v-else chain
#[test]
fn conditional_slot_v_if_else_chain() {
    let result = compile_sfc(
        r#"<template><Comp><template #a v-if="cond1">A</template><template #b v-else-if="cond2">B</template><template #c v-else>C</template></Comp></template>
<script setup>import Comp from "./Comp.vue"; const cond1 = true; const cond2 = false;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_createSlots("),
        "conditional chain should use _createSlots, got:\n{}",
        tpl.code
    );
    // Should NOT have : undefined because chain ends with v-else
    assert!(
        !tpl.code.contains(": undefined"),
        "chain ending with v-else should NOT have : undefined, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: all static slots should NOT use _createSlots
#[test]
fn static_slots_no_create_slots() {
    let result = compile_sfc(
        r#"<template><Comp><template #header>Head</template><template #footer>Foot</template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("_createSlots"),
        "static slots should NOT use _createSlots, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("header:") && tpl.code.contains("footer:"),
        "static slots should use name: format, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_: 1"),
        "static slots should have STABLE flag (_: 1), got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: hyphenated slot names are quoted in object literal
#[test]
fn component_hyphenated_slot_names_quoted() {
    let result = compile_sfc(
        r#"<template><Comp><template #pool-summary><div>content</div></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("\"pool-summary\":"),
        "hyphenated slot name should be quoted, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: component with named slot + default text content
#[test]
fn component_named_slot_plus_default_text() {
    let result = compile_sfc(
        r#"<template><Comp><template #prefix><img /></template>hello</Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("prefix:"),
        "should have prefix slot, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("default: _withCtx(() => ["),
        "default text should be wrapped in default: _withCtx, got:\n{}",
        tpl.code
    );
}

// @ai-generated - TDD test: scoped slot parameters should be passed to _withCtx arrow
#[test]
fn scoped_slot_parameters_passed_to_withctx() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #page="{ text }">{{ text }}</template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        code.contains("_withCtx(({ text }) => ["),
        "scoped slot params should be in _withCtx arrow function, got:\n{}",
        code
    );
}

// ==================== Empty template slots ====================

// @ai-generated - TDD: empty named slot should not leak </template> into JS output
#[test]
fn empty_named_slot_no_close_tag_leak() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #title></template><template #default><span>content</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "empty slot should not leak </template> into JS, got:\n{}",
        code
    );
    assert!(
        code.contains("title:") && code.contains("_withCtx(() => [])"),
        "empty slot should produce name: _withCtx(() => []), got:\n{}",
        code
    );
}

// @ai-generated - TDD: empty named slot with whitespace-only content
#[test]
fn empty_named_slot_whitespace_only() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header>   </template><template #default><span>ok</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "whitespace-only slot should not leak </template>, got:\n{}",
        code
    );
}

// @ai-generated - TDD: multiple empty named slots
#[test]
fn multiple_empty_named_slots() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header></template><template #footer></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "empty slots should not leak </template>, got:\n{}",
        code
    );
    assert!(
        code.contains("header:") && code.contains("footer:"),
        "should have both slot keys, got:\n{}",
        code
    );
}

// @ai-generated - TDD: empty scoped slot (with params but no children)
#[test]
fn empty_scoped_slot_no_children() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #item="{ data }"></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "empty scoped slot should not leak </template>, got:\n{}",
        code
    );
    assert!(
        code.contains("_withCtx(({ data }) => [])"),
        "empty scoped slot should have params and empty array, got:\n{}",
        code
    );
}

// @ai-generated - TDD: empty slot with v-if in dynamic _createSlots mode
#[test]
fn empty_slot_with_v_if_dynamic() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header v-if="show"></template><template #footer><span>foot</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue"; const show = true;</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "empty conditional slot should not leak </template>, got:\n{}",
        code
    );
    assert!(
        code.contains("_createSlots("),
        "should use _createSlots for conditional slots, got:\n{}",
        code
    );
}

// @ai-generated - TDD: empty slot mixed with non-empty (real-world pattern from avava)
#[test]
fn empty_slot_mixed_with_content_slots() {
    let code = compile_and_validate_template(
        r#"<template><Tab><template #title></template><div>content</div></Tab></template>
<script setup>import Tab from "./Tab.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>"),
        "empty title slot should not leak </template>, got:\n{}",
        code
    );
}

// @ai-generated - TDD: self-closing named slot (counterpart to empty_named_slot_no_close_tag_leak)
#[test]
fn self_closing_template_slot() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #title /><template #default><span>ok</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("</template>") && !code.contains("<template"),
        "self-closing slot should not leak any template tags, got:\n{}",
        code
    );
    assert!(
        code.contains("title:") && code.contains("_withCtx(() => [])"),
        "self-closing slot should produce empty slot function, got:\n{}",
        code
    );
}

// @ai-generated - TDD: self-closing with whitespace (counterpart to empty_named_slot_whitespace_only)
// Note: self-closing `<template #header />` can't have inner whitespace — this tests
// that the self-closing form still works in a context with other normal slots.
#[test]
fn self_closing_slot_with_other_normal_slot() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header /><template #default><span>ok</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("<template"),
        "self-closing slot should not leak template tags, got:\n{}",
        code
    );
    assert!(
        code.contains("header:") && code.contains("default:"),
        "should have both slot keys, got:\n{}",
        code
    );
}

// @ai-generated - TDD: multiple self-closing slots (counterpart to multiple_empty_named_slots)
#[test]
fn multiple_self_closing_named_slots() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header /><template #footer /></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("<template"),
        "self-closing slots should not leak template tags, got:\n{}",
        code
    );
    assert!(
        code.contains("header:") && code.contains("footer:"),
        "should have both slot keys, got:\n{}",
        code
    );
}

// @ai-generated - TDD: self-closing scoped slot (counterpart to empty_scoped_slot_no_children)
#[test]
fn self_closing_scoped_template_slot() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #item="{ row }" /></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("<template") && !code.contains("/>"),
        "self-closing scoped slot should not leak template syntax, got:\n{}",
        code
    );
    assert!(
        code.contains("_withCtx(({ row }) => [])"),
        "self-closing scoped slot should have params and empty array, got:\n{}",
        code
    );
}

// @ai-generated - TDD: self-closing slot with v-if (counterpart to empty_slot_with_v_if_dynamic)
#[test]
fn self_closing_slot_with_v_if_dynamic() {
    let code = compile_and_validate_template(
        r#"<template><Comp><template #header v-if="show" /><template #footer><span>foot</span></template></Comp></template>
<script setup>import Comp from "./Comp.vue"; const show = true;</script>"#,
    );
    assert!(
        !code.contains("<template"),
        "self-closing conditional slot should not leak template tags, got:\n{}",
        code
    );
    assert!(
        code.contains("_createSlots("),
        "should use _createSlots for conditional slots, got:\n{}",
        code
    );
}

// @ai-generated - TDD: self-closing slot mixed with default content (counterpart to empty_slot_mixed_with_content_slots)
#[test]
fn self_closing_slot_mixed_with_content() {
    let code = compile_and_validate_template(
        r#"<template><Tab><template #title /><div>content</div></Tab></template>
<script setup>import Tab from "./Tab.vue";</script>"#,
    );
    assert!(
        !code.contains("<template"),
        "self-closing title slot should not leak template tags, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: literal boolean/null in v-bind should not get _ctx. prefix
#[test]
fn literal_boolean_in_bind_no_ctx_prefix() {
    let code = compile_and_validate_template(
        r#"<template><div><Comp :show="false" :active="true" /></div></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    assert!(
        !code.contains("_ctx.false"),
        "literal false should NOT get _ctx. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.true"),
        "literal true should NOT get _ctx. prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("show: false") && code.contains("active: true"),
        "literal booleans should appear as-is in props, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: HTML entities in v-bind expressions should be decoded
#[test]
fn html_entities_in_bind_value_decoded() {
    let code = compile_and_validate_template(
        r#"<template><div :data="{&quot;key&quot;:&quot;value&quot;}"></div></template>"#,
    );
    assert!(
        !code.contains("&quot;"),
        "HTML entities should be decoded in v-bind expressions, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"{"key":"value"}"#),
        "decoded expression should contain normal quotes, got:\n{}",
        code
    );
}

// ==================== Dynamic props array ====================

// @ai-generated - TDD tests for dynamicProps output with PROPS patchFlag
#[test]
fn element_with_event_handler_has_dynamic_props_array() {
    let result = compile_sfc(
        r#"<template><button @click="handler">text</button></template>
<script setup>const handler = () => {};</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // When patchFlag includes PROPS (8), the dynamicProps array must be present.
    // Vue expects: createElementVNode("button", { onClick: handler }, "text", 8, ["onClick"])
    assert!(
        tpl.code.contains("[\"onClick\"]"),
        "event handler should produce dynamicProps array [\"onClick\"], got:\n{}",
        tpl.code
    );
}

#[test]
fn element_with_dynamic_bind_and_event_has_dynamic_props_array() {
    // Use both :disabled and @click so PATCH_PROPS is set (events trigger it)
    let result = compile_sfc(
        r#"<template><button :disabled="isDisabled" @click="handler">go</button></template>
<script setup>const isDisabled = true; const handler = () => {};</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Dynamic bind + event should produce dynamicProps array with both
    assert!(
        tpl.code.contains("\"disabled\"") && tpl.code.contains("\"onClick\""),
        "dynamic bind + event should produce dynamicProps array, got:\n{}",
        tpl.code
    );
}

#[test]
fn element_with_multiple_dynamic_props_has_all_in_array() {
    let result = compile_sfc(
        r#"<template><button @click="handler" :disabled="off">go</button></template>
<script setup>const handler = () => {}; const off = false;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Both onClick and disabled should be in the dynamicProps array
    assert!(
        tpl.code.contains("\"onClick\"") && tpl.code.contains("\"disabled\""),
        "multiple dynamic props should all be in dynamicProps array, got:\n{}",
        tpl.code
    );
}

#[test]
fn element_with_only_static_props_no_dynamic_props_array() {
    let result = compile_sfc(
        r#"<template><div class="foo" id="bar">text</div></template>
<script setup></script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Static-only props should NOT have a dynamicProps array
    assert!(
        !tpl.code.contains("[\"class\"]") && !tpl.code.contains("[\"id\"]"),
        "static-only props should NOT produce dynamicProps array, got:\n{}",
        tpl.code
    );
}

#[test]
fn dual_script_preserves_named_exports() {
    // Companion <script> with runtime named exports alongside <script setup>
    let result = compile_sfc(
        r#"<script lang="ts">
export enum SwapSettingsContext {
  swap,
  invest,
}
</script>

<script setup lang="ts">
const props = defineProps({ context: String })
</script>

<template><div>{{ props.context }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The enum should be preserved in the output (downleveled from TS)
    assert!(
        script.code.contains("SwapSettingsContext"),
        "companion script named export should be preserved.\nOutput:\n{}",
        script.code
    );
    // Should still have the setup wrapper
    assert!(
        script.code.contains("_defineComponent"),
        "setup wrapper should be present.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "default export should be present.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn with_defaults_merges_defaults_into_props() {
    // withDefaults(defineProps<{ color?: string, size?: string, label?: string }>(), { color: 'primary', size: 'md' })
    // should produce props: { color: { type: String, default: 'primary' }, size: { type: String, default: 'md' }, label: { type: String } }
    let result = compile_sfc(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  color?: string
  size?: string
  label?: string
}>(), {
  color: 'primary',
  size: 'md'
})
</script>

<template><div>{{ props.color }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // Should have props section with defaults merged
    assert!(
        script.code.contains("default: 'primary'"),
        "should merge color default.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: 'md'"),
        "should merge size default.\nOutput:\n{}",
        script.code
    );
    // label has no default and is optional — should NOT have required: true
    assert!(
        !script.code.contains("label") || !script.code.contains("required: true"),
        "optional label without default should not be required.\nOutput:\n{}",
        script.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = oxc_parser::Parser::new(&alloc, &script.code, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        script.code,
        parsed.errors
    );
}

#[test]
fn with_defaults_type_reference() {
    // withDefaults(defineProps<Props>(), { color: 'primary' })
    // where Props is a type alias — type resolution should still work
    let result = compile_sfc(
        r#"<script setup lang="ts">
type Props = {
  color?: string
  size?: string
}

const props = withDefaults(defineProps<Props>(), {
  color: 'primary',
})
</script>

<template><div>{{ props.color }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // Should have props section with default
    assert!(
        script.code.contains("default: 'primary'"),
        "should merge color default.\nOutput:\n{}",
        script.code
    );
    // Should have props section at all
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn define_props_type_with_imported_types() {
    // defineProps<Props>() where Props has imported types — all props should
    // still appear in the runtime props section (with null type for unknown)
    let result = compile_sfc(
        r#"<script setup lang="ts">
type Props = {
  pool: Pool
  loading: boolean
  items?: string[]
}

const props = defineProps<Props>()
</script>

<template><div>{{ props.loading }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // All props should be present (even Pool which is unresolvable)
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("pool:"),
        "pool prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("loading:"),
        "loading prop should be in props section.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn with_defaults_imported_types_all_props_present() {
    // withDefaults with Props that has imported types — all props must be present
    let result = compile_sfc(
        r#"<script setup lang="ts">
type Props = {
  pool: Pool
  loading: boolean
  titleTokens: PoolToken[]
  color?: string
}

const props = withDefaults(defineProps<Props>(), {
  color: 'primary',
})
</script>

<template><div>{{ props.loading }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    eprintln!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("pool:"),
        "pool prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("loading:"),
        "loading prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("titleTokens:"),
        "titleTokens prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: 'primary'"),
        "color should have default.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable imported type (no declarator)
/// should emit runtime prop declarations from the defaults variable.
/// Vue's `mergeDefaults({}, defaults)` does NOT create new prop declarations
/// (it only merges into existing ones), so we must create them ourselves.
/// This is the exact pattern from oku-primitives Label.vue.
#[test]
fn with_defaults_unresolvable_type_no_declarator() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { LabelProps } from './Label.ts'
import { DEFAULT_LABEL_PROPS } from './Label.ts'

defineOptions({
  name: 'RadixLabel',
  inheritAttrs: false,
})

withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template><div /></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    // Must have props section that creates prop declarations from defaults
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("DEFAULT_LABEL_PROPS"),
        "should reference the defaults variable.\nOutput:\n{}",
        script.code
    );
    // Must NOT use mergeDefaults (it doesn't create new prop declarations)
    assert!(
        !script.code.contains("mergeDefaults"),
        "should NOT use mergeDefaults (it doesn't create new props).\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable imported type WITH declarator
/// should emit runtime prop declarations and `const props = __props`.
#[test]
fn with_defaults_unresolvable_type_with_declarator() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { LabelProps } from './Label.ts'
import { DEFAULT_LABEL_PROPS } from './Label.ts'

const props = withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template><div>{{ props.as }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("DEFAULT_LABEL_PROPS"),
        "should reference the defaults variable.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__props"),
        "should have __props assignment.\nOutput:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("mergeDefaults"),
        "should NOT use mergeDefaults (it doesn't create new props).\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable type + object literal defaults
/// should emit inline prop declarations from the parsed object literal.
#[test]
fn with_defaults_unresolvable_type_object_literal_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { AffixProps } from './affix'
const props = withDefaults(defineProps<AffixProps>(), {
  zIndex: 100,
  target: '',
  position: 'top',
})
</script>
<template><div>{{ props.zIndex }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    // Should have inline prop declarations with defaults
    assert!(
        script.code.contains("zIndex:"),
        "should declare zIndex prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "should have default values.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable type + function call defaults
/// e.g., `withDefaults(defineProps<Props>(), getDefaults())`
#[test]
fn with_defaults_unresolvable_type_function_call_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { Props } from './types'
import { getDefaults } from './defaults'

const props = withDefaults(defineProps<Props>(), getDefaults())
</script>
<template><div>{{ props.foo }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("getDefaults()"),
        "should reference the function call.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable type without any defaults
/// should emit empty props `{}`
#[test]
fn with_defaults_unresolvable_type_no_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    // Should still have props: {} for unresolvable type without defaults
    // (or may omit props section entirely — both are acceptable)
    // But should NOT crash or produce invalid JS
}

/// @ai-generated — withDefaults + resolvable inline type + object defaults
/// should NOT use the IIFE pattern — should resolve types normally
#[test]
fn with_defaults_resolvable_type_still_works() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  color?: string
  size?: number
}>(), {
  color: 'red',
  size: 42,
})
</script>
<template><div>{{ props.color }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("color:"),
        "should have color prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "should have defaults.\nOutput:\n{}",
        script.code
    );
    // Should NOT use the IIFE pattern for resolvable types
    assert!(
        !script.code.contains("for(const k in d)"),
        "should NOT use IIFE pattern for resolvable types.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults with mixed: some props have defaults, some don't
/// with unresolvable type + object literal defaults
#[test]
fn with_defaults_unresolvable_type_mixed_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { FormProps } from './form'
const props = withDefaults(defineProps<FormProps>(), {
  method: 'POST',
  action: '/api/submit',
})
</script>
<template><div>{{ props.method }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("method:"),
        "should declare method prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("action:"),
        "should declare action prop.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated — withDefaults + unresolvable type + spread/computed property defaults
/// Tests that the IIFE runtime approach handles any expression as defaults
#[test]
fn with_defaults_unresolvable_type_complex_expression_defaults() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { Props } from './types'
import { baseDefaults } from './defaults'

withDefaults(defineProps<Props>(), { ...baseDefaults, extra: true })
</script>
<template><div /></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    // Should have props section — object literal with spread still parsed as MacroObjectArg
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn template_v_if_renders_as_fragment() {
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
    let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        tpl.code,
        parsed.errors
    );
}

#[test]
fn template_v_for_with_v_if_children_renders_as_fragment() {
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
    let parsed = oxc_parser::Parser::new(&alloc, &tpl.code, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        tpl.code,
        parsed.errors
    );
}

#[test]
fn v_for_locals_no_ctx_prefix_in_slot() {
    // v-for destructured variables should NOT get _ctx. prefix
    // even inside component slots
    let result = compile_sfc(
        r#"<script setup lang="ts">
const items = ref([])
</script>

<template>
  <div>
<MyComp v-for="({ name, id }, i) in items" :key="i">
  <span>{{ name }}</span>
  <span>{{ id }}</span>
</MyComp>
  </div>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Loop vars should NOT have _ctx. prefix
    assert!(
        !tpl.code.contains("_ctx.name"),
        "v-for local 'name' should NOT have _ctx. prefix.\nOutput:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.id"),
        "v-for local 'id' should NOT have _ctx. prefix.\nOutput:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.i"),
        "v-for local 'i' should NOT have _ctx. prefix.\nOutput:\n{}",
        tpl.code
    );
}

#[test]
fn dynamic_class_with_array_uses_normalize_class() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const cls = ref({})
</script>
<template>
  <div :class="['foo', cls]">hello</div>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_normalizeClass"),
        "Dynamic :class should use _normalizeClass().\nOutput:\n{}",
        tpl.code
    );
}

#[test]
fn component_is_uses_resolve_dynamic_component() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const tag = ref('a')
</script>
<template>
  <component :is="tag" class="link">click</component>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_resolveDynamicComponent"),
        "<component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        tpl.code
    );
    // The :is prop should NOT appear in the props object
    assert!(
        !tpl.code.contains("is:") && !tpl.code.contains("\"is\":"),
        ":is should not be in props object.\nOutput:\n{}",
        tpl.code
    );
}

#[test]
fn component_is_self_closing_uses_resolve_dynamic_component() {
    // Self-closing <component :is> with no children should use _resolveDynamicComponent
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
</script>
<template>
  <component :is="tag" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Self-closing <component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
    assert!(
        !code.contains("_resolveComponent(\"component\")"),
        "Should NOT use _resolveComponent(\"component\").\nOutput:\n{}",
        code
    );
    // The :is prop should NOT appear in the props object
    assert!(
        !code.contains("is:") && !code.contains("\"is\""),
        ":is should not be in props object.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_is_empty_uses_resolve_dynamic_component() {
    // Empty <component :is> (open + close, no children) should use _resolveDynamicComponent
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
</script>
<template>
  <component :is="tag"></component>
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Empty <component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_is_self_closing_with_props() {
    // <component :is> with extra props but no children
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
const cls = ref('active')
</script>
<template>
  <component :is="tag" :class="cls" id="main" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "<component :is> with extra props should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
    // :is should be excluded, but :class and id should remain
    assert!(
        !code.contains("is:") && !code.contains("\"is\""),
        ":is should not be in props object.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_static_is_uses_resolve_dynamic_component() {
    // Static <component is="div"> (without colon binding)
    let code = compile_and_validate_template(
        r#"<template>
  <component is="div" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Static <component is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_is_with_prop_binding_and_vbind() {
    // Matches BalLink.vue pattern: <component :is="tag" :class="[classes]" v-bind="attrs_">
    let result = compile_sfc(
        r#"<script setup lang="ts">
const tag = withDefaults(defineProps<{ tag?: string }>(), { tag: 'a' }).tag
const attrs_ = computed(() => ({}))
const classes = computed(() => ({ link: true }))
</script>
<template>
  <component :is="tag" :class="[classes]" v-bind="attrs_">
<slot />
  </component>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    eprintln!("BalLink-style output:\n{}", tpl.code);
    assert!(
        tpl.code.contains("_resolveDynamicComponent"),
        "<component :is> with prop binding should use _resolveDynamicComponent.\nOutput:\n{}",
        tpl.code
    );
}

#[test]
fn imported_component_uses_setup_binding_not_resolve_component() {
    // When a component is imported in <script setup>, the template should
    // reference it via $setup["TokenBreakdown"] (standalone mode), NOT via
    // _resolveComponent("TokenBreakdown"). This is critical for component
    // resolution at runtime.
    let code = compile_and_validate_template(
        r#"<script setup>
import TokenBreakdown from './components/TokenBreakdown.vue'
</script>
<template><div><TokenBreakdown :token="item" /></div></template>"#,
    );
    assert!(
        !code.contains("_resolveComponent"),
        "Imported component should NOT use _resolveComponent.\nOutput:\n{}",
        code
    );
    assert!(
        code.contains("$setup[\"TokenBreakdown\"]") || code.contains("$setup.TokenBreakdown"),
        "Imported component should use $setup binding.\nOutput:\n{}",
        code
    );
}

#[test]
fn static_and_dynamic_class_merged_into_single_prop() {
    // When an element has both a static `class` and a dynamic `:class`,
    // they must be merged into a single `class` property using _normalizeClass.
    // Having two separate `class:` keys causes the second to override the first.
    let code = compile_and_validate_template(
        r#"<template><div class="static-class" :class="[dynamic ? 'a' : 'b']">text</div></template>"#,
    );
    // Should have exactly ONE class key
    let class_count = code.matches("class:").count();
    assert_eq!(
        class_count, 1,
        "Should have exactly one `class:` key, got {}.\nOutput:\n{}",
        class_count, code
    );
    // Should include both static and dynamic in _normalizeClass
    assert!(
        code.contains("static-class"),
        "Should include static class.\nOutput:\n{}",
        code
    );
    assert!(
        code.contains("_normalizeClass"),
        "Should use _normalizeClass.\nOutput:\n{}",
        code
    );
}

#[test]
fn dual_script_export_default_merged_as_options() {
    // Companion <script> with `export default { inheritAttrs: false }`
    // should be merged into the setup wrapper (no duplicate export default)
    let result = compile_sfc(
        r#"<script lang="ts">
export default {
  inheritAttrs: false,
};
</script>

<script lang="ts" setup>
const msg = 'hello'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // `inheritAttrs: false` should be merged into the component definition
    assert!(
        script.code.contains("inheritAttrs: false"),
        "companion export default options should be merged.\nOutput:\n{}",
        script.code
    );
    // Should NOT have two `export default`
    let export_default_count = script.code.matches("export default").count();
    assert_eq!(
        export_default_count, 1,
        "should have exactly one export default, got {}.\nOutput:\n{}",
        export_default_count, script.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = oxc_parser::Parser::new(&alloc, &script.code, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        script.code,
        parsed.errors
    );
}

/// @ai-generated - Scoped style scope_id in script must include data-v- prefix
/// and must match the scope_id used in CSS selectors.
#[test]
fn scoped_style_scope_id_matches_between_script_and_css() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
<style scoped>.app { color: red; }</style>"#,
    );

    let script = result.script.as_ref().expect("script block");
    let style = result.styles.first().expect("should have a style block");

    // Script must emit __scopeId with data-v- prefix
    let scope_marker = "__scopeId = \"";
    let scope_pos = script.code.find(scope_marker).expect(&format!(
        "Script should contain __scopeId assignment, got:\n{}",
        script.code
    ));
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("should have closing quote")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];

    assert!(
        script_scope_id.starts_with("data-v-"),
        "Script __scopeId must start with 'data-v-', got: '{}'\nFull script:\n{}",
        script_scope_id,
        script.code
    );

    // CSS must use the same scope_id in its selectors
    let css_marker = "[data-v-";
    let css_pos = style.code.find(css_marker).expect(&format!(
        "CSS should contain [data-v-...] selector, got:\n{}",
        style.code
    ));
    let css_id_start = css_pos + 1; // skip '['
    let css_id_end = style.code[css_id_start..]
        .find(']')
        .expect("should have closing ]")
        + css_id_start;
    let css_scope_id = &style.code[css_id_start..css_id_end];

    assert_eq!(
        script_scope_id, css_scope_id,
        "Script __scopeId and CSS selector scope_id must match.\nScript: {}\nCSS: {}",
        script.code, style.code
    );
}

// @ai-generated - Tests that template ref attribute is emitted in VDOM render output
#[test]
fn template_ref_emitted_in_vdom_render() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const container = ref()
</script>
<template><div ref="container">hello</div></template>"#,
    );
    assert!(
        code.contains("ref: \"container\""),
        "Template ref should be emitted as ref: \"container\" in props. Got:\n{}",
        code
    );
}

// @ai-generated - Tests that template ref works alongside other props
#[test]
fn template_ref_with_other_props() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const el = ref()
</script>
<template><div ref="el" class="box">content</div></template>"#,
    );
    assert!(
        code.contains("ref: \"el\""),
        "Template ref should be in props object. Got:\n{}",
        code
    );
    assert!(
        code.contains("class: \"box\""),
        "Class prop should also be present. Got:\n{}",
        code
    );
}

// @ai-generated - Tests that CSS child combinator is preserved in scoped styles
#[test]
fn scoped_css_child_combinator_preserved() {
    let result = compile_sfc(
        r#"<script setup>
const x = 1
</script>
<template><div class="parent"><span class="child">{{ x }}</span></div></template>
<style scoped>
.parent > .child { color: red; }
</style>"#,
    );
    let css = &result.styles[0].code;
    // Must NOT have dangling > after the scope attr
    assert!(
        !css.contains("]>"),
        "Scope attr must not be followed by dangling > combinator.\nCSS: {}",
        css
    );
}

#[test]
fn ts_return_type_annotation_in_computed() {
    // Regression test: vue-vben-admin app.vue panics on type annotation in computed()
    let result = compile_sfc(
        r#"<script lang="ts" setup>
import type { GlobalThemeOverrides } from 'naive-ui';
import { computed } from 'vue';

defineOptions({ name: 'App' });

const themeOverrides = computed((): GlobalThemeOverrides => {
  return {
common: {},
  };
});
</script>

<template>
  <div>{{ themeOverrides }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
}

#[test]
fn ts_return_type_no_strip_mode() {
    // Test compilation with force_js=false (host mode) to match NAPI behavior
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("app.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script lang="ts" setup>
import type { GlobalThemeOverrides } from 'naive-ui';
import { computed } from 'vue';

import {
  darkTheme,
  dateEnUS,
  dateZhCN,
  enUS,
  lightTheme,
  NConfigProvider,
  NMessageProvider,
  NNotificationProvider,
  zhCN,
} from 'naive-ui';

defineOptions({ name: 'App' });

const { commonTokens } = useNaiveDesignTokens();

const tokenLocale = computed(() =>
  preferences.app.locale === 'zh-CN' ? zhCN : enUS,
);
const tokenDateLocale = computed(() =>
  preferences.app.locale === 'zh-CN' ? dateZhCN : dateEnUS,
);
const tokenTheme = computed(() =>
  preferences.theme.mode === 'dark' ? darkTheme : lightTheme,
);

const themeOverrides = computed((): GlobalThemeOverrides => {
  return {
common: commonTokens,
  };
});
</script>

<template>
  <NConfigProvider
:date-locale="tokenDateLocale"
:locale="tokenLocale"
:theme="tokenTheme"
:theme-overrides="themeOverrides"
class="h-full"
  >
<NNotificationProvider>
  <NMessageProvider>
    <RouterView />
  </NMessageProvider>
</NNotificationProvider>
  </NConfigProvider>
</template>
"#,
        &options,
        &verter_opts,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
}

#[test]
fn setup_returns_bindings_for_template_refs() {
    // Regression: setup() returned {} instead of exposing bindings,
    // causing template refs (ref="editorContainer") to not bind to
    // the ref() variable, making Vue unable to set .value.
    //
    // Vue's official compiler for this input returns:
    //   return { container, editor, msg }
    // from the setup function so that template refs can bind.
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Editor.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: false,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup lang="ts">
import { ref, onMounted, shallowRef } from 'vue'

const container = ref<HTMLElement>()
const editor = shallowRef()
const msg = ref('hello')

onMounted(() => {
  if (!container.value) return
  console.log('mounted', container.value)
})
</script>

<template>
  <div class="wrapper">
<div ref="container" class="editor" />
<span>{{ msg }}</span>
  </div>
</template>
"#,
        &options,
        &verter_opts,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.unwrap();
    let code = &script.code;

    // Extract the __returned__ assignment (matches Vue's official compiler pattern)
    let returned_idx = code.find("const __returned__ = ");
    assert!(
        returned_idx.is_some(),
        "Must have __returned__ in setup(). Got:\n{}",
        code
    );
    let returned_rest = &code[returned_idx.unwrap()..];
    let returned_end = returned_rest.find(';').unwrap_or(returned_rest.len());
    let returned_stmt = &returned_rest[..returned_end];

    // The returned object must NOT be empty
    assert!(
        !returned_stmt.contains("= {}") && !returned_stmt.contains("= { }"),
        "setup() must NOT return empty object. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );

    // Must include __isScriptSetup marker (matches Vue's official compiler)
    assert!(
        code.contains("__isScriptSetup"),
        "Must have __isScriptSetup marker. Full:\n{}",
        code
    );

    // Must return container, editor, msg bindings (like Vue's official compiler)
    assert!(
        returned_stmt.contains("container"),
        "return must include 'container'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
    assert!(
        returned_stmt.contains("editor"),
        "return must include 'editor'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
    assert!(
        returned_stmt.contains("msg"),
        "return must include 'msg'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
}

#[test]
fn setup_returns_bindings_with_define_props() {
    // Test with defineProps to match the actual Editor.vue pattern
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Editor.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: false,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup lang="ts">
import { ref, onMounted, shallowRef } from 'vue'
import * as monaco from 'monaco-editor-core'

const props = defineProps<{
  store: any
}>()

const editorContainer = ref<HTMLElement>()
const editor = shallowRef()
const pendingCode = ref<string | null>(null)

onMounted(() => {
  if (!editorContainer.value) return
  editor.value = monaco.editor.create(editorContainer.value, {})
})
</script>

<template>
  <div class="editor-wrapper">
<div ref="editorContainer" class="editor-container" />
  </div>
</template>
"#,
        &options,
        &verter_opts,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.unwrap();
    let code = &script.code;

    // Extract the __returned__ assignment
    let returned_idx = code.find("const __returned__ = ").expect(&format!(
        "Must have __returned__ assignment. Full output:\n{}",
        code
    ));
    let returned_rest = &code[returned_idx..];
    let returned_end = returned_rest.find(';').unwrap_or(returned_rest.len());
    let returned_stmt = &returned_rest[..returned_end];

    // Must NOT return empty - Vue's official compiler returns all top-level bindings
    assert!(
        !returned_stmt.contains("= {}"),
        "setup() must NOT return empty. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );

    // editorContainer must be in return for template ref binding
    assert!(
        returned_stmt.contains("editorContainer"),
        "return must include 'editorContainer' for template ref. Returned: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
}

#[test]
fn optional_tuple_element_in_define_emits() {
    // Regression: menu.vue from vue-vben-admin panics on TSTupleElement
    // with optional tuple elements like `[string, string?]` in defineEmits.
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("menu.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script lang="ts" setup>
import type { MenuRecordRaw } from '@vben/types';

import type { MenuProps } from '@vben-core/menu-ui';

import { Menu } from '@vben-core/menu-ui';

interface Props extends MenuProps {
  menus?: MenuRecordRaw[];
}

const props = withDefaults(defineProps<Props>(), {
  accordion: true,
  menus: () => [],
});

const emit = defineEmits<{
  open: [string, string[]];
  select: [string, string?];
}>();

function handleMenuSelect(key: string) {
  emit('select', key, props.mode);
}

function handleMenuOpen(key: string, path: string[]) {
  emit('open', key, path);
}
</script>

<template>
  <Menu
:accordion="accordion"
:collapse="collapse"
:collapse-show-title="collapseShowTitle"
:default-active="defaultActive"
:menus="menus"
:mode="mode"
:rounded="rounded"
scroll-to-active
:theme="theme"
@open="handleMenuOpen"
@select="handleMenuSelect"
  />
</template>
"#,
        &options,
        &verter_opts,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
    let script = result.script.unwrap();
    assert!(
        script.code.contains("_defineComponent"),
        "should contain _defineComponent"
    );
}

/// @ai-generated — Template literal with HTML entities in v-bind should produce valid JS
/// with correct _ctx. prefixing. Regression test for a bug where `&quot;` entities in
/// template literals caused binding patches to be applied at wrong byte offsets,
/// producing mangled identifiers like `useri_ctx.userinfoname` instead of
/// `_ctx.userinfo.nickname`.
#[test]
fn test_vbind_template_literal_with_html_entities() {
    let code = compile_and_validate_template(
        r#"<template><div :subtitle="`&quot;${userinfo.nickname}&quot;共获得${_formatNumber(userinfo.aweme_count)}个赞`"></div></template>"#,
    );

    // _ctx. should appear before each unresolved identifier
    assert!(
        code.contains("_ctx.userinfo.nickname"),
        "Should prefix userinfo.nickname with _ctx., got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx._formatNumber"),
        "Should prefix _formatNumber with _ctx., got:\n{}",
        code
    );
    // Identifiers must not be split by _ctx. insertion
    assert!(
        !code.contains("useri_ctx"),
        "Identifiers should not be split by _ctx. insertion, got:\n{}",
        code
    );
    assert!(
        !code.contains("_formatNum_ctx"),
        "Identifiers should not be split by _ctx. insertion, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: component-level v-slot params should be passed to default slot _withCtx
#[test]
fn component_v_slot_params_in_default_slot() {
    let code = compile_and_validate_template(
        r#"<template><NuxtLink v-slot="{ href, navigate, route: linkRoute, isActive, ...rest }" :to="to" custom>
  <a :href="href" @click="navigate">{{ linkRoute }}</a>
</NuxtLink></template>
<script setup>
import NuxtLink from "./NuxtLink.vue";
const to = "/about";
</script>"#,
    );
    assert!(
        code.contains("_withCtx(({ href, navigate, route: linkRoute, isActive, ...rest }) => ["),
        "component-level v-slot params should be in _withCtx arrow function, got:\n{}",
        code
    );
    // Slot scope variables should NOT get $setup. prefix
    assert!(
        !code.contains("$setup.href") && !code.contains("$setup.linkRoute"),
        "slot scope variables should not get $setup. prefix, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: data-* and aria-* attributes should NOT be camelized in dynamic binds
#[test]
fn data_and_aria_attributes_not_camelized() {
    let code = compile_and_validate_template(
        r#"<template><div :data-orientation="orientation" :aria-expanded="expanded" :data-state="state"></div></template>
<script setup>
const orientation = "vertical";
const expanded = true;
const state = "open";
</script>"#,
    );
    assert!(
        code.contains("\"data-orientation\""),
        "data-* bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        code.contains("\"aria-expanded\""),
        "aria-* bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        code.contains("\"data-state\""),
        "data-state bind should preserve hyphenated name with quotes, got:\n{}",
        code
    );
    assert!(
        !code.contains("dataOrientation")
            && !code.contains("ariaExpanded")
            && !code.contains("dataState"),
        "data-*/aria-* should NOT be camelized, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: type-only defineProps with type reference from companion <script> block
#[test]
fn cross_block_type_resolution_for_define_props() {
    let result = compile_sfc(
        r#"<script lang="ts">
export interface AlertProps {
  title?: string
  description?: string
  color?: string
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<AlertProps>(), {
  color: 'primary'
})
</script>
<template><div>{{ props.title }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // All three props should be declared in the runtime props object
    assert!(
        script.code.contains("title:")
            && script.code.contains("description:")
            && script.code.contains("color:"),
        "All props from companion-block interface should be in runtime props, got:\n{}",
        script.code
    );
}

// @ai-generated - TDD test: defineModel declares runtime prop and emit
#[test]
fn define_model_declares_prop_and_emit() {
    let result = compile_sfc(
        r#"<script setup>
const modelValue = defineModel()
</script>
<template><div>{{ modelValue }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel() should declare a `modelValue` prop in the component definition
    assert!(
        script.code.contains("modelValue"),
        "defineModel() should declare modelValue prop.\nGot:\n{}",
        script.code
    );
    // Should declare 'update:modelValue' emit
    assert!(
        script.code.contains("update:modelValue"),
        "defineModel() should declare 'update:modelValue' emit.\nGot:\n{}",
        script.code
    );
}

// @ai-generated - TDD test: named defineModel declares correct prop and emit
#[test]
fn define_model_named_declares_prop_and_emit() {
    let result = compile_sfc(
        r#"<script setup>
const count = defineModel('count')
</script>
<template><div>{{ count }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel('count') should declare a `count` prop
    assert!(
        script.code.contains("props:") && script.code.contains("count"),
        "defineModel('count') should declare 'count' prop in props section.\nGot:\n{}",
        script.code
    );
    // Should declare 'update:count' emit
    assert!(
        script.code.contains("update:count"),
        "defineModel('count') should declare 'update:count' emit.\nGot:\n{}",
        script.code
    );
}

// @ai-generated - TDD test: HTML entities in text content must be decoded
#[test]
fn html_entity_nbsp_decoded_in_text() {
    let result = compile_sfc(
        r#"<script setup>
</script>
<template><span>&nbsp;</span></template>"#,
    );
    let template = result.template.as_ref().expect("template block");
    // Vue's compiler decodes &nbsp; to the literal U+00A0 character in the JS string.
    // Verter must do the same — outputting "&nbsp;" literally causes double-escaping
    // in the DOM (&amp;nbsp;).
    assert!(
        !template.code.contains("&nbsp;"),
        "HTML entity &nbsp; should be decoded, not left as literal text.\nGot:\n{}",
        template.code
    );
    // Should contain the actual non-breaking space character (\u{00A0})
    assert!(
        template.code.contains('\u{00A0}'),
        "Template should contain decoded non-breaking space (U+00A0).\nGot:\n{}",
        template.code
    );
}

// ==================== Event modifier handling ====================

#[test]
fn event_modifier_prevent_uses_with_modifiers() {
    // @click.prevent="handler" should wrap with _withModifiers
    let code = compile_and_validate_template(
        r#"<template><div @click.prevent="handler">text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "Event with .prevent modifier should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains(r#""prevent""#),
        "Should include 'prevent' in modifier list\n{}",
        code
    );
}

#[test]
fn event_modifier_stop_prevent_combined() {
    // @click.stop.prevent should wrap handler with _withModifiers(handler, ["stop", "prevent"])
    let code = compile_and_validate_template(
        r#"<template><div @click.stop.prevent="handler">text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "Multiple modifiers should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains(r#""stop""#) && code.contains(r#""prevent""#),
        "Should include both 'stop' and 'prevent' in modifier list\n{}",
        code
    );
}

#[test]
fn event_modifier_capture_goes_into_key() {
    // @click.capture="handler" should produce onClickCapture: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.capture="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickCapture"),
        "Capture modifier should be appended to event key name\n{}",
        code
    );
    // Should NOT use _withModifiers for capture (it's an option modifier)
    assert!(
        !code.contains("_withModifiers"),
        "Capture modifier should not use _withModifiers\n{}",
        code
    );
}

#[test]
fn event_modifier_once_goes_into_key() {
    // @click.once="handler" should produce onClickOnce: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.once="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickOnce"),
        "Once modifier should be appended to event key name\n{}",
        code
    );
}

#[test]
fn event_modifier_passive_goes_into_key() {
    // @click.passive="handler" should produce onClickPassive: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.passive="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickPassive"),
        "Passive modifier should be appended to event key name\n{}",
        code
    );
}

#[test]
fn event_modifier_keyup_enter_uses_with_keys() {
    // @keyup.enter="handler" should wrap with _withKeys
    let code =
        compile_and_validate_template(r#"<template><input @keyup.enter="handler" /></template>"#);
    assert!(
        code.contains("_withKeys"),
        "Key modifier should use _withKeys\n{}",
        code
    );
    assert!(
        code.contains(r#""enter""#),
        "Should include 'enter' in key list\n{}",
        code
    );
}

#[test]
fn event_modifier_empty_handler_with_prevent() {
    // @click.prevent="" should produce _withModifiers(() => {}, ["prevent"])
    let code =
        compile_and_validate_template(r#"<template><div @click.prevent="">text</div></template>"#);
    assert!(
        code.contains("_withModifiers"),
        "Empty handler with .prevent should still use _withModifiers\n{}",
        code
    );
}

#[test]
fn event_modifier_prevent_only_no_value() {
    // @contextmenu.prevent (no value) should produce _withModifiers(() => {}, ["prevent"])
    let code = compile_and_validate_template(
        r#"<template><div @contextmenu.prevent>text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "No-value handler with .prevent should use _withModifiers\n{}",
        code
    );
}

// ==================== Static style → object ====================

#[test]
fn static_style_compiled_to_object() {
    // Vue compiles static style="margin-top: 15px" into an object { "margin-top": "15px" }
    // so that SSR can serialize it compactly as margin-top:15px;
    let code = compile_and_validate_template(
        r#"<template><div style="margin-top: 15px">text</div></template>"#,
    );
    // Should produce an object, not a string
    assert!(
        code.contains(r#"{ "margin-top": "15px" }"#),
        "Static style should be compiled to a JS object, not a string\n{}",
        code
    );
    assert!(
        !code.contains(r#"style: "margin-top"#),
        "Static style should NOT be emitted as a string\n{}",
        code
    );
}

#[test]
fn static_style_multiple_properties() {
    let code = compile_and_validate_template(
        r#"<template><div style="color: red; font-size: 14px">text</div></template>"#,
    );
    assert!(
        code.contains(r#"color: "red""#),
        "Should parse color property\n{}",
        code
    );
    assert!(
        code.contains(r#""font-size": "14px""#),
        "Should parse font-size property (quoted because of hyphen)\n{}",
        code
    );
}

#[test]
fn event_modifier_on_component_generates_import() {
    // When a component has @click.stop="handler", the compiled output
    // should include _withModifiers AND the import for it.
    let result = compile_sfc(r#"<template><MyComponent @click.stop="handler" /></template>"#);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_withModifiers"),
        "Component event with .stop modifier should use _withModifiers\n{}",
        tpl.code
    );
    // The import must be present — without it, _withModifiers is a ReferenceError at runtime
    assert!(
        tpl.imports.contains(&"_withModifiers"),
        "Component event with modifiers should import withModifiers from vue\nimports: {:?}\n{}",
        tpl.imports,
        tpl.code
    );
}

// ==================== Type-based defineEmits ====================

#[test]
fn type_based_define_emits_generates_emits_option() {
    // defineEmits<{ mousedown: [event: MouseEvent] }>() should generate
    // emits: ["mousedown"] in the component definition.
    let result = compile_sfc(
        r#"<script setup lang="ts">
const emit = defineEmits<{ mousedown: [event: MouseEvent] }>()
</script>
<template><div>test</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains(r#"emits:"#) || script.code.contains(r#"emits :"#),
        "Type-based defineEmits should generate emits option in component definition\n{}",
        script.code
    );
    assert!(
        script.code.contains("mousedown"),
        "Emits option should include 'mousedown'\n{}",
        script.code
    );
}

#[test]
fn type_based_define_emits_call_signature_generates_emits_option() {
    // defineEmits<{ (e: 'change', value: string): void }>() should generate
    // emits: ["change"] in the component definition.
    let result = compile_sfc(
        r#"<script setup lang="ts">
const emit = defineEmits<{ (e: 'change', value: string): void }>()
</script>
<template><div>test</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains(r#"emits:"#) || script.code.contains(r#"emits :"#),
        "Type-based defineEmits with call signature should generate emits option\n{}",
        script.code
    );
    assert!(
        script.code.contains("change"),
        "Emits option should include 'change'\n{}",
        script.code
    );
}

// ==================== Template binding resolution ====================

#[test]
fn imported_function_in_template_gets_setup_prefix() {
    // When a function is imported in <script setup> and used in the template,
    // it should be resolved to $setup.fn or _ctx.fn (not left as bare identifier)
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { isNullish } from './utils'
const val = ref(null)
</script>
<template><div v-if="isNullish(val)">empty</div><div v-else>has value</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let script = result.script.as_ref().expect("script block");
    // The function should be prefixed with $setup. to be accessible in the render context
    assert!(
        tpl.code.contains("$setup.isNullish"),
        "Imported function used in template should have $setup prefix\ntemplate:\n{}\nscript:\n{}",
        tpl.code,
        script.code
    );
    // The import must also be returned from setup
    assert!(
        script.code.contains("isNullish"),
        "Imported function should be returned from setup\n{}",
        script.code
    );
}

#[test]
fn companion_script_import_available_in_template() {
    // When a function is imported in the companion <script> block (not <script setup>),
    // it should still be available in the template via $setup prefix, and returned from setup.
    let result = compile_sfc(
        r#"<script lang="ts">
import { isNullish } from './utils'
export interface MyProps { value?: string }
</script>
<script setup lang="ts">
const props = defineProps<MyProps>()
</script>
<template><div v-if="isNullish(props.value)">empty</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let script = result.script.as_ref().expect("script block");
    // The companion import should be returned from setup so it's available at runtime
    assert!(
        script.code.contains("return") && script.code.contains("isNullish"),
        "Companion script import should be returned from setup\nscript:\n{}",
        script.code
    );
    // The companion import should be resolved in the template (not _ctx. which won't work)
    assert!(
        tpl.code.contains("$setup.isNullish"),
        "Companion script import used in template should have $setup prefix\ntemplate:\n{}",
        tpl.code
    );
}

// ═══════════════════════════════════════════════════════════════
// force_js: template expression TS stripping
// ═══════════════════════════════════════════════════════════════

/// @ai-generated - force_js should strip `as` type assertions from template expressions
#[test]
fn force_js_strips_as_expression_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref('hello')
</script>
<template><div>{{ (foo as string) }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("as string"),
        "force_js should strip 'as string' from template expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip non-null assertions from template expressions
#[test]
fn force_js_strips_non_null_assertion_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref<string | null>('hello')
</script>
<template><div>{{ foo! }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The output should not contain the `!` non-null assertion
    // foo! should become just foo
    assert!(
        !tpl.code.contains("$setup.foo!"),
        "force_js should strip '!' non-null assertion from template expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip type arguments from template call expressions
#[test]
fn force_js_strips_type_arguments_from_template_call() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
function generic<T>(val: T): T { return val }
</script>
<template><div>{{ generic<string>('hello') }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("<string>"),
        "force_js should strip '<string>' type argument from template call expression, got:\n{}",
        tpl.code
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

/// @ai-generated - force_js should strip satisfies expressions from template
#[test]
fn force_js_strips_satisfies_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const x = { a: 1 }
</script>
<template><div>{{ (x satisfies Record<string, number>) }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("satisfies"),
        "force_js should strip 'satisfies' from template expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — HTML comment between v-if branches must not leak into generated JS
/// when comments are disabled (production mode). Regression: interstitial comments were
/// skipped in visit_comment but not overwritten, and build_child_records excluded them
/// when options.comments=false, so strip_interstitial_condition_nodes couldn't find them.
#[test]
fn comment_between_v_if_branches_does_not_leak_in_prod() {
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
    let parsed = oxc_parser::Parser::new(&js_alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Template JS parse error (nested): {:?}\n--- generated code ---\n{}",
        parsed
            .errors
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
    let parsed2 = oxc_parser::Parser::new(&js_alloc2, &wrapped2, source_type).parse();
    assert!(
        parsed2.errors.is_empty(),
        "Template JS parse error (root): {:?}\n--- generated code ---\n{}",
        parsed2
            .errors
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

/// @ai-generated — withDefaults + cross-block type reference from companion <script>
/// should produce correct prop names (not garbled from wrong source offsets).
/// Regression: macros.rs used span extraction into SFC source for external types
/// where key spans reference the companion block, producing corrupted prop names.
#[test]
fn with_defaults_cross_block_type_uses_key_name() {
    let result = compile_sfc(
        r#"<script lang="ts">
export interface ExternalProps {
  title?: string
  description?: string
  color?: string
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<ExternalProps>(), {
  color: 'primary'
})
</script>
<template><div>{{ props.title }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The withDefaults path must use key_name (pre-resolved) for cross-block types,
    // not span extraction which indexes into the wrong source region.
    assert!(
        script.code.contains("title:"),
        "title prop should appear with correct name in withDefaults output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("description:"),
        "description prop should appear with correct name in withDefaults output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("color:") && script.code.contains("default: 'primary'"),
        "color prop should have default: 'primary' in withDefaults output, got:\n{}",
        script.code
    );
}

// ======================== defineModel + withDefaults (runtime variable) ========================

/// @ai-generated - defineModel + withDefaults with resolvable type uses _mergeModels.
/// Vue's official compiler always uses _mergeModels when both defineProps and defineModel
/// are present in the same component.
#[test]
fn define_model_with_defaults_resolved_type() {
    let result = compile_sfc_keep_ts(
        r#"<script setup lang="ts">
interface ChatInputProps {
  placeholder?: string
  maxLength?: number
}

const DEFAULT_PROPS = { placeholder: 'Type...', maxLength: 50 }

const props = withDefaults(defineProps<ChatInputProps>(), DEFAULT_PROPS)
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div>{{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Vue uses _mergeModels to merge typed props with model props
    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels to merge model props with withDefaults props.\nGot:\n{}",
        script.code
    );
    // Model prop and modifiers must appear in the second arg to _mergeModels
    assert!(
        script.code.contains("visible"),
        "Model prop 'visible' should be declared.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("visibleModifiers"),
        "Model modifiers prop should be declared.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel + withDefaults with unresolvable type + runtime variable
/// must produce valid JS (IIFE fallback). The model props must NOT be inserted inside
/// the IIFE body — _mergeModels wraps the IIFE.
#[test]
fn define_model_with_defaults_runtime_var_iife() {
    // Simulate: imported type (unresolvable in standalone compile) + runtime defaults
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { ChatInputProps } from './types'

const DEFAULT_PROPS = { placeholder: 'Type...' }

const props = withDefaults(defineProps<ChatInputProps>(), DEFAULT_PROPS)
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div>{{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Even with IIFE fallback, _mergeModels must wrap the result
    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels to merge model props with withDefaults IIFE.\nGot:\n{}",
        script.code
    );
    // Must NOT have the invalid pattern: `return p, visible: {}`
    assert!(
        !script.code.contains("return p,"),
        "Model props must NOT be inserted inside the IIFE body.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel + defineProps with object literal merges correctly
/// When defineProps uses an object literal (not IIFE), static merge is fine,
/// but Vue still uses _mergeModels, so we should too.
#[test]
fn define_model_with_define_props_object_uses_merge_models() {
    let result = compile_sfc(
        r#"<script setup>
const props = defineProps({ title: String })
const visible = defineModel('visible')
</script>
<template><div>{{ props.title }} {{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Vue uses _mergeModels even for object literal props + models
    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels for props + model merge.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("update:visible"),
        "Should declare 'update:visible' emit.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel + type-based withDefaults merges correctly
#[test]
fn define_model_with_typed_with_defaults() {
    let result = compile_sfc_keep_ts(
        r#"<script setup lang="ts">
interface Props {
  placeholder?: string
}

const props = withDefaults(defineProps<Props>(), { placeholder: 'Type...' })
const open = defineModel('open', { type: Boolean })
</script>
<template><div>{{ props.placeholder }} {{ open }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels for typed withDefaults + defineModel.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("open:") || script.code.contains("open "),
        "Model prop 'open' should be declared.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel emits section also uses _mergeModels when defineEmits present
#[test]
fn define_model_with_define_emits_uses_merge_models_for_emits() {
    let result = compile_sfc(
        r#"<script setup>
const emit = defineEmits(['click'])
const visible = defineModel('visible')
</script>
<template><div @click="emit('click')">{{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // The emits section should merge ['click'] with ["update:visible"]
    assert!(
        script.code.contains("update:visible"),
        "Model emit 'update:visible' should be present.\nGot:\n{}",
        script.code
    );
}

// ======================== export type stripping (force_js) ========================

/// @ai-generated - export type inside script setup must be stripped when force_js: true
#[test]
fn export_type_stripped_when_force_js() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload =
  | { type: 'notification'; to: string }
  | { type: 'menu-item'; to: string }

interface SideMenuProps {
  visible?: boolean
}

const props = defineProps<SideMenuProps>()
const isOpen = computed(() => props.visible)
</script>

<template><div>{{ isOpen }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    // export type must be completely removed in force_js mode
    assert!(
        !script.code.contains("export type NavigatePayload"),
        "export type should be stripped when force_js: true, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("NavigatePayload"),
        "NavigatePayload should not appear at all in JS output, got:\n{}",
        script.code
    );
}

/// @ai-generated - export interface inside script setup must be stripped when force_js: true
#[test]
fn export_interface_stripped_when_force_js() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
export interface FooProps {
  title: string
  count: number
}

const props = defineProps<FooProps>()
</script>

<template><div>{{ props.title }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    assert!(
        !script.code.contains("export interface FooProps"),
        "export interface should be stripped when force_js: true, got:\n{}",
        script.code
    );
}

/// @ai-generated - bare type and interface (no export) stripped when force_js: true
#[test]
fn bare_type_and_interface_stripped_when_force_js() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
type LocalType = { a: string }

interface LocalInterface {
  b: number
}

const x = 1
</script>

<template><div>{{ x }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    assert!(
        !script.code.contains("type LocalType"),
        "type alias should be stripped when force_js: true, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("interface LocalInterface"),
        "interface should be stripped when force_js: true, got:\n{}",
        script.code
    );
}

// ======================== export type hoisting (keep TS) ========================

fn compile_sfc_keep_ts(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// @ai-generated - export type is hoisted outside setup wrapper when keeping TS types
#[test]
fn export_type_hoisted_when_keep_ts() {
    let result = compile_sfc_keep_ts(
        r#"<script setup lang="ts">
import { computed } from 'vue'

export type NavigatePayload = { type: string; to: string }

const x = computed(() => 1)
</script>

<template><div>{{ x }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    // export type should be hoisted BEFORE the setup wrapper (before const __sfc__)
    let type_pos = script
        .code
        .find("export type NavigatePayload")
        .expect("export type should be present when keeping TS");
    let wrapper_pos = script
        .code
        .find("const __sfc__")
        .expect("const __sfc__ should be present");
    assert!(
        type_pos < wrapper_pos,
        "export type should be hoisted before const __sfc__.\ntype_pos={}, wrapper_pos={}\ncode:\n{}",
        type_pos, wrapper_pos, script.code
    );
    // Must NOT appear inside setup() body
    let setup_start = script.code.find("setup(").expect("setup function");
    assert!(
        type_pos < setup_start,
        "export type should be outside setup function, got:\n{}",
        script.code
    );
}

// ==================== JS string escaping in codegen ====================

#[test]
fn style_with_newlines_produces_valid_js() {
    // Regression: ant-design-vue horizontal.vue has style with literal newlines.
    // Verter must escape newlines in style property names to produce valid JS.
    let code = compile_and_validate_template(
        "<template><div style=\"\n  {\n    padding: '20px'\n  }\n\"></div></template>",
    );
    // The output must not contain raw newlines inside string literals
    assert!(
        !code.contains("\"{\n"),
        "style object key must have newlines escaped\n{}",
        code
    );
}

#[test]
fn style_normal_css_produces_valid_js() {
    let code = compile_and_validate_template(
        r#"<template><div style="margin-top: 15px; color: red"></div></template>"#,
    );
    assert!(
        code.contains("\"margin-top\""),
        "hyphenated CSS prop should be quoted\n{}",
        code
    );
    assert!(
        code.contains("\"15px\""),
        "value should be quoted\n{}",
        code
    );
}

#[test]
fn style_with_multiline_value_produces_valid_js() {
    // Multi-line style attribute values are common in formatted templates
    let code = compile_and_validate_template(
        "<template><div style=\"\n  margin-top: 15px;\n  color: red;\n\"></div></template>",
    );
    assert!(
        code.contains("\"margin-top\""),
        "hyphenated prop should be parsed from multiline style\n{}",
        code
    );
}

#[test]
fn title_attr_with_newline_produces_valid_js() {
    // Static attributes with newlines must be properly escaped
    let code =
        compile_and_validate_template("<template><div title=\"line1\nline2\"></div></template>");
    // The output must parse as valid JS (compile_and_validate_template checks this)
    assert!(!code.is_empty());
}

// ==================== Vapor mode: __vapor flag and _ctx. prefix ====================

#[test]
fn vapor_script_contains_vapor_flag() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        script.code.contains("__vapor = true") || script.code.contains("__vapor: true"),
        "Vapor script should contain __vapor flag, got:\n{}",
        script.code
    );
}

#[test]
fn vapor_template_uses_ctx_prefix_not_setup() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor template should not use $setup. prefix, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_ctx.msg"),
        "Vapor template should use _ctx. prefix for bindings, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_event_handler_uses_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst onClick = () => {}\n</script>\n<template><button @click=\"onClick\">click</button></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor event handler should not use $setup. prefix, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_template_attr_uses_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst title = 'hello'\n</script>\n<template><div :title=\"title\"></div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor dynamic attr should not use $setup. prefix, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_ctx.title"),
        "Vapor dynamic attr should use _ctx. prefix, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_with_template_vapor_attr_contains_vapor_flag() {
    // Using <template vapor> attribute (not force_vapor option)
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template vapor><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        script.code.contains("__vapor = true") || script.code.contains("__vapor: true"),
        "Component with <template vapor> should contain __vapor flag, got:\n{}",
        script.code
    );
}

#[test]
fn vapor_props_use_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst props = defineProps({ msg: String })\n</script>\n<template><div>{{ props.msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup.")
            && !tpl.code.contains("$props.")
            && !tpl.code.contains("__props."),
        "Vapor props should not use $setup./$props./__props. prefix, got:\n{}",
        tpl.code
    );
}

// ══════════════════════════════════════════════════════════════════════
// Bug 3: Destructured props binding — template binding resolution tests
// ══════════════════════════════════════════════════════════════════════

/// @ai-generated — Destructured defineProps should resolve to $props. prefix in template
#[test]
fn destructured_define_props_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = defineProps<{ msg: string }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "destructured defineProps prop should resolve to $props.msg, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.msg"),
        "destructured defineProps prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Aliased destructured defineProps: `const { msg: m }` → $props.m
#[test]
fn aliased_destructured_define_props_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ m }}</div></template>
<script setup lang="ts">const { msg: m } = defineProps<{ msg: string }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.m"),
        "aliased destructured prop should resolve to $props.m, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.m"),
        "aliased destructured prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured withDefaults should resolve to $props. prefix
#[test]
fn destructured_with_defaults_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = withDefaults(defineProps<{ msg?: string }>(), { msg: 'hello' })</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "destructured withDefaults prop should resolve to $props.msg, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.msg"),
        "destructured withDefaults prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Multiple destructured props mixed with setup bindings
#[test]
fn destructured_props_mixed_with_setup_bindings() {
    let result = compile_sfc(
        r#"<template><div>{{ a }} {{ b }}</div></template>
<script setup lang="ts">
import { ref } from 'vue'
const { a } = defineProps<{ a: string }>()
const b = ref(0)
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.a"),
        "destructured prop 'a' should resolve to $props.a, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$setup.b"),
        "setup ref 'b' should resolve to $setup.b, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured prop in v-bind attribute
#[test]
fn destructured_prop_in_v_bind() {
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

/// @ai-generated — Destructured prop in event handler expression
#[test]
fn destructured_prop_in_event_handler() {
    let result = compile_sfc(
        r#"<template><button @click="handler">click</button></template>
<script setup lang="ts">const { handler } = defineProps<{ handler: () => void }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.handler"),
        "destructured prop in event handler should resolve to $props.handler, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured props should NOT appear in setup return object
#[test]
fn destructured_props_not_in_setup_return() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = defineProps<{ msg: string }>()</script>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The setup return should be empty or not contain 'msg' (props use $props, not $setup)
    assert!(
        !script.code.contains("return { msg }") && !script.code.contains("return { msg,"),
        "destructured prop 'msg' should NOT be in setup return, got:\n{}",
        script.code
    );
}

/// @ai-generated — Destructured withDefaults with multiple props including rest
#[test]
fn destructured_with_defaults_multiple_props() {
    let result = compile_sfc(
        r#"<template><div>{{ a }} {{ b }}</div></template>
<script setup lang="ts">const { a, b } = withDefaults(defineProps<{ a?: string, b?: number }>(), { a: 'x', b: 1 })</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.a"),
        "destructured withDefaults prop 'a' should resolve to $props.a, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$props.b"),
        "destructured withDefaults prop 'b' should resolve to $props.b, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured withDefaults with unresolvable imported type
/// should still resolve destructured props to $props. prefix (the oku-primitives bug)
#[test]
fn destructured_with_defaults_unresolvable_type_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ label }}</div></template>
<script setup lang="ts">
import type { LabelProps } from './Label.ts'
const { label } = withDefaults(defineProps<LabelProps>(), { label: 'hello' })
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.label"),
        "destructured prop with unresolvable type should resolve to $props.label, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.label"),
        "destructured prop with unresolvable type should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

// ══════════════════════════════════════════════════════════════════════
// Bug 1: Duplicate event handler keys — merge into arrays
// ══════════════════════════════════════════════════════════════════════

/// @ai-generated — Two handlers on same event with different modifiers merged into array
#[test]
fn duplicate_event_handlers_same_event_merged_into_array() {
    let result = compile_sfc(
        r#"<template><div @keydown="a" @keydown.stop="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should have merged array syntax: onKeydown: [handler1, handler2]
    assert!(
        tpl.code.contains("onKeydown: ["),
        "should have merged array syntax onKeydown: [...], got:\n{}",
        tpl.code
    );
    // Should NOT have duplicate keys (two separate "onKeydown:" entries)
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "should have exactly one onKeydown: key (merged), got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Three+ handlers on same event all merged into array
#[test]
fn multiple_event_handlers_same_event_merged_into_array() {
    let result = compile_sfc(
        r#"<template><div @keydown="a" @keydown.stop="b" @keydown.prevent="c"></div></template>
<script setup>const a = () => {}; const b = () => {}; const c = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "should have merged array syntax, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "should have exactly one onKeydown: key (all merged), got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Different option modifiers produce DIFFERENT keys, not merged
#[test]
fn different_option_modifiers_produce_different_keys() {
    let result = compile_sfc(
        r#"<template><div @click="a" @click.capture="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick") && tpl.code.contains("onClickCapture"),
        "@click and @click.capture should produce two different keys, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Key modifiers on same event merged into array
#[test]
fn key_modifiers_same_event_merged() {
    let result = compile_sfc(
        r#"<template><div @keydown.enter="a" @keydown.tab="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "key modifier handlers should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "key modifier handlers should be merged, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Mixed: some events have duplicates, others don't
#[test]
fn mixed_duplicate_and_unique_events() {
    let result = compile_sfc(
        r#"<template><div @click="a" @keydown="b" @keydown.stop="c" @mouseenter="d"></div></template>
<script setup>const a = () => {}; const b = () => {}; const c = () => {}; const d = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // onKeydown should be merged into array
    assert!(
        tpl.code.contains("onKeydown: ["),
        "onKeydown should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "onKeydown should appear as one key (merged), got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("onClick:"),
        "onClick should be present, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("onMouseenter:"),
        "onMouseenter should be present, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Single handler with modifier (no merge needed, regression test)
#[test]
fn single_event_handler_no_merge() {
    let result = compile_sfc(
        r#"<template><div @click.stop="a"></div></template>
<script setup>const a = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick"),
        "single handler should still produce onClick key, got:\n{}",
        tpl.code
    );
    // Should NOT be wrapped in array
    assert!(
        !tpl.code.contains("[_withModifiers") && !tpl.code.contains("[withModifiers"),
        "single handler should NOT be wrapped in array, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Mouse left/right on non-keyboard event → runtime modifiers, same key
#[test]
fn mouse_left_right_as_runtime_modifiers_merged() {
    let result = compile_sfc(
        r#"<template><div @click.left="a" @click.right="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick: ["),
        "@click.left and @click.right should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onClick:").count(),
        1,
        "@click.left and @click.right should produce one onClick: key, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Handler with both key and runtime modifiers sharing same key
#[test]
fn handler_with_mixed_key_and_runtime_modifiers_merged() {
    let result = compile_sfc(
        r#"<template><div @keydown.enter.prevent="a" @keydown.enter.stop="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "handlers with mixed modifiers should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "handlers with mixed modifiers sharing same key should produce one key, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — @input and :onInput produce the same key, must be merged (Vue official behavior)
#[test]
fn v_on_and_v_bind_on_same_event_merged() {
    let result = compile_sfc(
        r#"<template><input @input="foo" :onInput="bar" /></template>
<script setup>const foo = () => {}; const bar = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onInput: ["),
        "@input and :onInput should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onInput:").count(),
        1,
        "@input and :onInput should produce one onInput: key, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Dynamic event names cannot be pre-computed, should NOT be merged
#[test]
fn dynamic_event_names_not_merged() {
    let result = compile_sfc(
        r#"<template><div @[eventName]="a" @[eventName]="b"></div></template>
<script setup>const eventName = 'click'; const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Dynamic event names should both appear (can't pre-compute key)
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.message.contains("duplicate")),
        "dynamic event names should not trigger duplicate errors"
    );
}

#[test]
fn vdom_mode_no_vapor_flag() {
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        !script.code.contains("__vapor"),
        "VDOM mode should NOT contain __vapor flag, got:\n{}",
        script.code
    );
}

#[test]
fn vdom_mode_uses_setup_prefix() {
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        tpl.code.contains("$setup.msg"),
        "VDOM mode should use $setup. prefix for setup bindings, got:\n{}",
        tpl.code
    );
}

// ==================== Template-only + scoped styles ====================

/// Template-only component with `<style scoped>` should emit a synthetic
/// script block containing `__scopeId` so Vue's runtime applies the
/// scoped `data-v-*` attributes to DOM elements.
#[test]
fn template_only_scoped_style_emits_scope_id_in_script() {
    let result = compile_sfc(
        "<template><div class=\"app\">hello</div></template>\n<style scoped>\n.app { color: red; }\n</style>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(!result.scope_id.is_empty(), "should have scope_id");

    let script = result
        .script
        .as_ref()
        .expect("template-only component with scoped style should emit a synthetic script block");
    assert!(
        script.code.contains("__scopeId"),
        "script should contain __scopeId assignment, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains(&result.scope_id),
        "script should reference the scope_id '{}', got:\n{}",
        result.scope_id,
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "script should export __sfc__, got:\n{}",
        script.code
    );
}

/// Template-only component WITHOUT scoped styles should NOT emit a
/// synthetic script block (no __scopeId needed).
#[test]
fn template_only_no_scoped_style_no_script_block() {
    let result = compile_sfc("<template><div>hello</div></template>");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        result.script.is_none(),
        "template-only without scoped style should not have a script block",
    );
}

/// Template-only with scoped style: CSS should contain scoped selectors.
#[test]
fn template_only_scoped_style_css_is_scoped() {
    let result = compile_sfc(
        "<template><div class=\"app\">hello</div></template>\n<style scoped>\n.app { color: red; }\n</style>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    assert!(
        result.styles[0].code.contains("[data-v-"),
        "scoped CSS should contain [data-v-] selector, got:\n{}",
        result.styles[0].code
    );
}

/// @ai-generated - Template-only component with scoped grid CSS: scope IDs must
/// match between script and CSS, and CSS selectors must all be scoped.
#[test]
fn template_only_scoped_style_grid_layout_scope_id_consistency() {
    let source = r#"<template>
  <div class="dashboard">
    <header class="header">
      <h1>Title</h1>
    </header>
    <aside class="sidebar">
      <ul class="menu">
        <li class="menu-item active"><span>Overview</span></li>
        <li class="menu-item"><span>Settings</span></li>
      </ul>
    </aside>
    <main class="content">
      <section class="stats-grid">
        <div class="stat-card">
          <h3>Total Users</h3>
          <p class="stat-value">12,345</p>
          <span class="stat-change positive">+12.5%</span>
        </div>
      </section>
      <section class="recent-activity">
        <table class="activity-table">
          <thead><tr><th>User</th><th>Action</th></tr></thead>
          <tbody>
            <tr>
              <td>John</td>
              <td><span class="badge success">Done</span></td>
            </tr>
          </tbody>
        </table>
      </section>
    </main>
    <footer class="footer">
      <p>&copy; 2026</p>
    </footer>
  </div>
</template>

<style scoped>
.dashboard {
  display: grid;
  grid-template-areas:
    "header header"
    "sidebar content"
    "footer footer";
  grid-template-columns: 250px 1fr;
  grid-template-rows: auto 1fr auto;
  min-height: 100vh;
}
.header {
  grid-area: header;
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
  background: #fff;
  border-bottom: 1px solid #ddd;
}
.sidebar { grid-area: sidebar; background: #f8f9fa; padding: 1rem; }
.content { grid-area: content; padding: 2rem; background: #f5f5f5; }
.footer {
  grid-area: footer;
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
  background: #fff;
  border-top: 1px solid #ddd;
}
.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 1rem; }
.stat-card {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}
.activity-table { width: 100%; border-collapse: collapse; }
.activity-table th { text-align: left; padding: 0.75rem; border-bottom: 2px solid #ddd; }
.activity-table td { padding: 0.75rem; border-bottom: 1px solid #eee; }
.badge { padding: 0.25rem 0.5rem; border-radius: 4px; font-size: 0.875rem; }
.badge.success { background: #d4edda; color: #155724; }
</style>"#;

    let result = compile_sfc(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );

    // 1. Script must have __scopeId
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script block");
    assert!(
        script.code.contains("__scopeId"),
        "script should contain __scopeId assignment, got:\n{}",
        script.code
    );

    // 2. Template must have a render function
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    assert!(
        template.code.contains("function render"),
        "template should contain render function, got:\n{}",
        template.code
    );

    // 3. Extract scope ID from script
    let scope_marker = "__scopeId = \"";
    let scope_pos = script
        .code
        .find(scope_marker)
        .expect("scope marker in script");
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("closing quote for scope ID")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];
    assert!(
        script_scope_id.starts_with("data-v-"),
        "scope ID must start with data-v-, got: '{}'",
        script_scope_id
    );

    // 4. CSS must have matching scope selectors
    assert_eq!(result.styles.len(), 1);
    let css = &result.styles[0].code;
    let css_scope_attr = format!("[{}]", script_scope_id);
    assert!(
        css.contains(&css_scope_attr),
        "CSS must contain scope selector '{}', got:\n{}",
        css_scope_attr,
        css
    );

    // 5. ALL CSS selectors that should be scoped must have the scope attribute.
    //    Check every non-at-rule selector in the CSS output.
    let expected_scoped_selectors = [
        ".dashboard",
        ".header",
        ".sidebar",
        ".content",
        ".footer",
        ".stats-grid",
        ".stat-card",
        // descendant selectors — only last part gets scope
        "th", // from ".activity-table th"
        "td", // from ".activity-table td"
        ".badge",
    ];
    for sel in expected_scoped_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // 6. Compound selector `.badge.success` should have scope on the compound
    let compound_scoped = format!(".badge.success{}", css_scope_attr);
    assert!(
        css.contains(&compound_scoped),
        "CSS should contain scoped compound selector '{}', got:\n{}",
        compound_scoped,
        css
    );

    // 7. Validate render function is valid JS
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", template.code);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Render function should be valid JS:\n{}\nErrors: {:?}",
        template.code,
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );

    // 8. Verify template imports contain all referenced helpers.
    // Extract helpers used in the render function (identifiers starting with _)
    let re_helpers: Vec<&str> = template
        .code
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|tok| tok.starts_with('_') && tok.len() > 1)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter(|h| {
            // Only check Vue runtime helpers (e.g. _createElementVNode, _openBlock)
            // Skip _ctx, _cache, etc.
            !["_ctx", "_cache"].contains(h)
        })
        .collect();
    for helper in &re_helpers {
        // Each _helper should map to a Vue export (strip leading _)
        let import_name = helper.strip_prefix('_').unwrap_or(helper);
        assert!(
            template
                .imports
                .iter()
                .any(|imp| &**imp == *helper || imp.ends_with(import_name)),
            "Template uses '{}' but it's not in imports {:?}",
            helper,
            template.imports
        );
    }

    // Debug: print imports only (CSS/script verified above)
    eprintln!("=== TEMPLATE IMPORTS ===\n{:?}", template.imports);
}

/// @ai-generated - Full template-heavy.vue integration test: compile the exact
/// fixture content and verify every CSS selector is properly scoped.
#[test]
fn template_heavy_vue_full_css_scoping() {
    let source = include_str!("../../../packages/benchmark/src/fixtures/template-heavy.vue");
    let result = compile_sfc(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );

    // Must have exactly one scoped style block
    assert_eq!(result.styles.len(), 1, "expected 1 style block");
    let css = &result.styles[0].code;

    // Extract scope ID from script
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script block");
    let scope_marker = "__scopeId = \"";
    let scope_pos = script
        .code
        .find(scope_marker)
        .expect("scope marker in script");
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("closing quote for scope ID")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];
    let css_scope_attr = format!("[{}]", script_scope_id);

    // Print full CSS for inspection
    eprintln!("=== FULL SCOPED CSS OUTPUT ===\n{}", css);
    eprintln!("=== SCOPE ATTR: {} ===", css_scope_attr);

    // Every original class selector must be scoped in the output.
    // For descendant selectors (e.g., .activity-table th), only the last
    // compound selector gets the scope attribute.
    let expected_scoped_selectors = [
        // Layout
        ".dashboard",
        ".header",
        ".sidebar",
        ".content",
        ".footer",
        // Stats
        ".stats-grid",
        ".stat-card",
        // Charts
        ".charts",
        ".chart-container",
        ".chart-placeholder",
        ".bar",
        // Activity
        ".recent-activity",
        ".activity-table",
        // Descendant selectors — last compound gets scope
        "th", // from ".activity-table th"
        "td", // from ".activity-table td"
        // Badges (simple)
        ".badge",
        // Widgets
        ".widgets",
        ".widget",
        ".indicator",
    ];
    for sel in expected_scoped_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // Compound selectors — the scope attribute goes at the END of the compound
    let expected_compound_selectors = [
        ".badge.success",
        ".badge.danger",
        ".badge.warning",
        ".badge.info",
        ".indicator.online",
        ".indicator.warning",
    ];
    for sel in expected_compound_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped compound selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // Key CSS properties must be preserved (not dropped or collapsed)
    let preserved_properties = [
        "grid-template-areas",
        "grid-template-columns",
        "grid-template-rows",
        "min-height",
        "grid-area",
        "border-collapse",
        "box-shadow",
        "border-radius",
    ];
    for prop in preserved_properties {
        assert!(
            css.contains(prop),
            "CSS must preserve property '{}', got:\n{}",
            prop,
            css
        );
    }

    // Scope attribute count: every rule must have at least one scoped selector.
    // Count number of `{` that are NOT inside @-rules, and count scope attributes.
    let scope_count = css.matches(&css_scope_attr).count();
    assert!(
        scope_count >= 20, // template-heavy.vue has ~25 selectors
        "Expected at least 20 scoped selectors, found {} in:\n{}",
        scope_count,
        css
    );

    // Validate template render function is valid JS
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    eprintln!("=== TEMPLATE CODE ===\n{}", template.code);

    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", template.code);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Template render function should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        template.code
    );

    // Validate script is valid JS
    eprintln!("=== SCRIPT CODE ===\n{}", script.code);
    let alloc2 = Allocator::new();
    let parsed2 = oxc_parser::Parser::new(&alloc2, &script.code, source_type).parse();
    assert!(
        parsed2.errors.is_empty(),
        "Script should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed2
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        script.code
    );

    // Simulate the playground's mergeRenderIntoComponent:
    // script + "\n" + template (with import prepended by host)
    let assembled = format!("{}\n{}", script.code, {
        if template.imports.is_empty() {
            template.code.clone()
        } else {
            let specifiers: Vec<String> = template
                .imports
                .iter()
                .map(|name| {
                    if name.starts_with('_') {
                        format!("{} as {}", &name[1..], name)
                    } else {
                        name.to_string()
                    }
                })
                .collect();
            format!(
                "import {{ {} }} from \"vue\"\n{}",
                specifiers.join(", "),
                template.code
            )
        }
    });
    eprintln!("=== ASSEMBLED CODE (script + template) ===\n{}", assembled);

    // Verify the assembled code is valid JS
    let alloc3 = Allocator::new();
    let parsed3 = oxc_parser::Parser::new(&alloc3, &assembled, source_type).parse();
    assert!(
        parsed3.errors.is_empty(),
        "Assembled code should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed3
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        assembled
    );
}

// ==================== Async setup: _withAsyncContext ====================

#[test]
fn async_setup_wraps_await_with_async_context() {
    // Vue wraps top-level await in <script setup> with _withAsyncContext
    // to preserve component instance context across async boundaries.
    let result = compile_sfc(
        r#"<script setup>
const data = await fetch('/api').then(r => r.json());
</script>
<template>
  <div />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // Should have async setup
    assert!(
        script.code.contains("async setup("),
        "setup should be async, got:\n{}",
        script.code
    );

    // Should use _withAsyncContext wrapper
    assert!(
        script.code.contains("_withAsyncContext"),
        "await should be wrapped with _withAsyncContext, got:\n{}",
        script.code
    );

    // Should declare __temp and __restore
    assert!(
        script.code.contains("__temp") && script.code.contains("__restore"),
        "should declare __temp and __restore, got:\n{}",
        script.code
    );

    // Should import withAsyncContext from vue
    assert!(
        script.code.contains("withAsyncContext"),
        "should import withAsyncContext from vue, got:\n{}",
        script.code
    );
}

#[test]
fn async_setup_wraps_dynamic_import_await() {
    // The Editor.vue pattern: const editor = await import(...)
    let result = compile_sfc(
        r#"<script setup>
const props = defineProps(['type']);
const editor = await import(`./editors/${props.type}`).then(x => x.default);
</script>
<template>
  <component :is="editor" />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // The await argument should be wrapped in an arrow function for _withAsyncContext
    assert!(
        script.code.contains("_withAsyncContext("),
        "dynamic import await should use _withAsyncContext, got:\n{}",
        script.code
    );
}

// ==================== Import elision for type-only usage ====================

#[test]
fn import_specifier_used_only_as_type_should_be_elided() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { doSomething, AuthError } from "some-lib";
import type { UserCredential } from "some-lib";

const emit = defineEmits({
  error(error: AuthError) {
    return true;
  },
  linked(credential: UserCredential) {
    return true;
  },
});

doSomething();
</script>
<template>
  <div />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // Extract the __returned__ object to verify import elision
    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // AuthError should NOT appear anywhere — it's only used as a type annotation
    // in the defineEmits validator. After type stripping, it has no runtime references,
    // so it's elided from both the import statement and __returned__.
    assert!(
        !script.code.contains("AuthError"),
        "AuthError should be fully elided (only used as type), got:\n{}",
        script.code
    );
    // doSomething should be in the import (used at runtime in script body)
    assert!(
        script.code.contains("doSomething"),
        "doSomething should remain in imports (used at runtime), got:\n{}",
        script.code
    );
    // doSomething should NOT be in __returned__ (not referenced in template)
    assert!(
        !returned_obj.contains("doSomething"),
        "doSomething should not be in __returned__ (not used in template), got:\n{}",
        returned_obj
    );
    // import type should be fully stripped
    assert!(
        !script.code.contains("UserCredential"),
        "import type should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn import_used_in_template_should_be_in_returned() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { formatDate } from "./utils";
import MyComponent from "./MyComponent.vue";
import { helperFn } from "./helpers";
</script>
<template>
  <MyComponent>{{ formatDate(new Date()) }}</MyComponent>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // Imports used in template should be in __returned__
    assert!(
        returned_obj.contains("formatDate"),
        "formatDate (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
    assert!(
        returned_obj.contains("MyComponent"),
        "MyComponent (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
    // helperFn is NOT used in the template — should be excluded
    assert!(
        !returned_obj.contains("helperFn"),
        "helperFn (not used in template) should NOT be in __returned__, got:\n{}",
        returned_obj
    );
}

// ==================== Companion script import elision ====================

#[test]
fn companion_script_type_only_import_not_in_returned() {
    // Companion <script> imports that are only used as type assertions should NOT
    // be in __returned__. This matches Vue's official compiler behavior.
    // Regression test for CurrencyCodes build failure in judis-app.
    let result = compile_sfc(
        r#"<script lang="ts">
import { computed, defineComponent } from "vue";
import { CurrencyCodes, isArray } from "vue-composable";
import { CustomField as CustomFieldType } from "@judis/shared";

function getDefaultValue(field: CustomFieldType) {
  return { currency: "EUR" as CurrencyCodes, value: 0 };
}

export default defineComponent({});
</script>
<script setup lang="ts">
import { HButton } from "@judis/ui";

const props = defineProps<{ items: string[] }>();

function doStuff() {
  if (isArray(props.items)) {
    return getDefaultValue({ type: "money" } as CustomFieldType);
  }
}
</script>
<template>
  <HButton :label="doStuff()" />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // CurrencyCodes is only used as a type assertion in the companion body.
    // It should NOT be in __returned__ (would cause Rollup error if it's
    // a type-only export from the source package).
    assert!(
        !returned_obj.contains("CurrencyCodes"),
        "CurrencyCodes (type-only in companion) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // CustomFieldType is only used as a type annotation.
    // Should NOT be in __returned__.
    assert!(
        !returned_obj.contains("CustomFieldType"),
        "CustomFieldType (type-only in companion) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // computed and defineComponent are from vue and not used in template.
    // Should NOT be in __returned__.
    assert!(
        !returned_obj.contains("computed"),
        "computed (companion import, not used in template) should NOT be in __returned__, got:\n{}",
        returned_obj
    );
    assert!(
        !returned_obj.contains("defineComponent"),
        "defineComponent (companion import, not used in template) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // isArray is used in setup body but NOT in template.
    // Should NOT be in __returned__ (matches Vue's behavior).
    assert!(
        !returned_obj.contains("isArray"),
        "isArray (companion import, not used in template) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // HButton IS used in template — should be in __returned__
    assert!(
        returned_obj.contains("HButton"),
        "HButton (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );

    // doStuff IS a setup declaration used in template — should be in __returned__
    assert!(
        returned_obj.contains("doStuff"),
        "doStuff (setup function used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
}

#[test]
fn companion_script_import_used_in_template_in_returned() {
    // Companion <script> imports that ARE used in the template should be in __returned__.
    let result = compile_sfc(
        r#"<script lang="ts">
import { formatCurrency } from "./utils";
import { unusedHelper } from "./helpers";

export default {};
</script>
<script setup lang="ts">
const msg = "hello";
</script>
<template>
  <div>{{ formatCurrency(42) }}</div>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // formatCurrency is used in template — should be in __returned__
    assert!(
        returned_obj.contains("formatCurrency"),
        "formatCurrency (companion import used in template) should be in __returned__, got:\n{}",
        returned_obj
    );

    // unusedHelper is NOT used in template — should NOT be in __returned__
    assert!(
        !returned_obj.contains("unusedHelper"),
        "unusedHelper (companion import, not used in template) should NOT be in __returned__, got:\n{}",
        returned_obj
    );
}

// ==================== Reserved word props ====================

#[test]
fn class_prop_uses_bracket_notation_vdom() {
    let code = compile_and_validate_template(
        r#"<script setup>
defineProps<{ class?: string }>()
</script>
<template>
  <div :class="class"></div>
</template>"#,
    );
    // Must use bracket notation for JS reserved word "class"
    assert!(
        code.contains(r#"$props["class"]"#),
        "Expected $props[\"class\"] in VDOM output, got:\n{}",
        code
    );
}

#[test]
fn class_prop_on_component_uses_bracket_notation_vdom() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
const props = defineProps<{ class?: string }>()
</script>
<template>
  <Comp :class="class" />
</template>"#,
    );
    // Must use bracket notation for JS reserved word "class"
    assert!(
        code.contains(r#"$props["class"]"#),
        "Expected $props[\"class\"] in VDOM output, got:\n{}",
        code
    );
}

#[test]
fn class_prop_uses_bracket_notation_vapor() {
    let code = compile_and_validate_vapor_template(
        r#"<script setup>
defineProps<{ class?: string }>()
</script>
<template>
  <div :class="class"></div>
</template>"#,
    );
    // Vapor uses _ctx prefix; must use bracket notation for "class"
    assert!(
        code.contains(r#"_ctx["class"]"#),
        "Expected _ctx[\"class\"] in Vapor output, got:\n{}",
        code
    );
}

// ==================== Single element children must be array-wrapped ====================

/// @ai-generated - When an element has a single element child (e.g., <li><button>text</button></li>),
/// Vue requires the child VNode to be wrapped in an array. Passing a bare VNode as children
/// causes Vue to misinterpret it as a slots object and render nothing.
#[test]
fn single_element_child_wrapped_in_array() {
    let code = compile_and_validate_template(
        r#"<template>
  <ul>
    <li><button>Create User</button></li>
  </ul>
</template>"#,
    );
    eprintln!("=== SINGLE ELEMENT CHILD OUTPUT ===\n{}", code);
    // The button VNode must be wrapped in an array: [_createElementVNode("button", ...)]
    // NOT passed directly: _createElementVNode("button", ...)
    assert!(
        code.contains(r#"[_createElementVNode("button""#),
        "Single element child must be wrapped in array. Got:\n{}",
        code
    );
}

/// @ai-generated - Multiple <li><button>...</button></li> all must have array-wrapped children.
#[test]
fn single_element_children_in_list() {
    let code = compile_and_validate_template(
        r#"<template>
  <ul>
    <li><button>Create User</button></li>
    <li><button>Generate Report</button></li>
    <li><button>Export Data</button></li>
  </ul>
</template>"#,
    );
    eprintln!("=== LIST SINGLE ELEMENT CHILDREN ===\n{}", code);
    // Each <li> must wrap its single <button> child in an array
    // Count occurrences of [_createElementVNode("button"
    let array_wrapped_count = code.matches(r#"[_createElementVNode("button""#).count();
    assert_eq!(
        array_wrapped_count, 3,
        "Expected 3 array-wrapped button children. Got {} in:\n{}",
        array_wrapped_count, code
    );
}

/// @ai-generated - Single element child in <td><span>...</span></td> must be array-wrapped.
#[test]
fn single_element_child_in_td() {
    let code = compile_and_validate_template(
        r#"<template>
  <table><tbody><tr>
    <td><span class="badge">Done</span></td>
  </tr></tbody></table>
</template>"#,
    );
    eprintln!("=== TD SINGLE ELEMENT CHILD ===\n{}", code);
    assert!(
        code.contains(r#"[_createElementVNode("span""#),
        "Single element child in <td> must be wrapped in array. Got:\n{}",
        code
    );
}

// ==================== HTML entity decoding ====================

/// @ai-generated - HTML named entity &copy; must be decoded to © in render output.
#[test]
fn html_entity_copy_decoded() {
    let code = compile_and_validate_template(
        r#"<template>
  <p>&copy; 2026</p>
</template>"#,
    );
    eprintln!("=== HTML ENTITY OUTPUT ===\n{}", code);
    assert!(
        code.contains("\u{00A9}") || code.contains("©"),
        "&copy; must be decoded to © character. Got:\n{}",
        code
    );
    assert!(
        !code.contains("&copy;"),
        "&copy; must NOT appear as literal string in JS output. Got:\n{}",
        code
    );
}

// ==================== Top-level await ====================

#[test]
fn top_level_await_produces_async_setup() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const props = defineProps<{
  id: string
}>()

const item = (await getById(props.id))!

const name = item.name
</script>
<template>
  <div>{{ name }}</div>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("async setup("),
        "Expected async setup() for top-level await, got:\n{}",
        script.code
    );
}

// ── TSX codegen integration tests ─────────────────────────────────

fn compile_tsx(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        include_tsx: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

#[test]
fn tsx_basic_sfc() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx_script = result.tsx_script.as_ref().expect("tsx_script block");
    assert!(
        !tsx_script.code.is_empty(),
        "TSX script code should not be empty"
    );
    assert!(
        tsx_script.code.contains("function __verter_tsx_App"),
        "Should contain component wrapper function, got: {}",
        tsx_script.code
    );
    assert!(
        tsx_script.code.contains("const msg = 'hello'"),
        "Should preserve setup content, got: {}",
        tsx_script.code
    );
    assert!(
        !tsx_script.source_map.is_empty(),
        "Source map should be generated"
    );

    let tsx_template = result.tsx_template.as_ref().expect("tsx_template block");
    assert!(
        !tsx_template.code.is_empty(),
        "TSX template code should not be empty"
    );
    assert!(
        tsx_template.code.contains("<div>"),
        "Template should contain JSX, got: {}",
        tsx_template.code
    );
}

#[test]
fn tsx_script_with_imports() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import type { Foo } from './types'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx_script = result.tsx_script.as_ref().expect("tsx_script block");
    // Imports should be hoisted above the wrapper function
    let fn_pos = tsx_script
        .code
        .find("function __verter_tsx_App")
        .expect("wrapper function");
    let ref_pos = tsx_script
        .code
        .find("import { ref } from 'vue'")
        .expect("ref import");
    let type_pos = tsx_script
        .code
        .find("import type { Foo } from './types'")
        .expect("type import");
    assert!(ref_pos < fn_pos, "ref import should be hoisted");
    assert!(type_pos < fn_pos, "type import should be hoisted");
}

#[test]
fn tsx_template_interpolation_with_bindings() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const msg = 'hello'
</script>

<template>
  <div>{{ count }} {{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx_template = result.tsx_template.as_ref().expect("tsx_template block");
    // count is a SetupRef → gets .value suffix in inline mode
    assert!(
        tsx_template.code.contains("count.value"),
        "SetupRef should get .value, got: {}",
        tsx_template.code
    );
}

#[test]
fn tsx_not_generated_when_disabled() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.tsx_script.is_none(), "TSX script should be None");
    assert!(result.tsx_template.is_none(), "TSX template should be None");
}

#[test]
fn tsx_template_comment() {
    let result = compile_tsx(r#"<template><!-- hello --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx_template = result.tsx_template.as_ref().expect("tsx_template block");
    assert!(
        tsx_template.code.contains("{/*"),
        "Comment should be converted to JSX, got: {}",
        tsx_template.code
    );
}

#[test]
fn tsx_no_template() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    // TSX script should still be generated
    assert!(result.tsx_script.is_some(), "TSX script should be present");
    // No template → no TSX template
    assert!(
        result.tsx_template.is_none(),
        "TSX template should be None when no template block"
    );
}
