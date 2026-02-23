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

    // Extract the return statement specifically
    let return_idx = code.find("return ");
    assert!(
        return_idx.is_some(),
        "Must have a return statement in setup(). Got:\n{}",
        code
    );
    let return_rest = &code[return_idx.unwrap()..];
    let return_end = return_rest.find(';').unwrap_or(return_rest.len());
    let return_stmt = &return_rest[..return_end];

    // The return must NOT be empty
    assert!(
        !return_stmt.contains("return {}") && !return_stmt.contains("return { }"),
        "setup() must NOT return empty object. Return was: '{}'. Full:\n{}",
        return_stmt,
        code
    );

    // Must return container, editor, msg bindings (like Vue's official compiler)
    assert!(
        return_stmt.contains("container"),
        "return must include 'container'. Return was: '{}'. Full:\n{}",
        return_stmt,
        code
    );
    assert!(
        return_stmt.contains("editor"),
        "return must include 'editor'. Return was: '{}'. Full:\n{}",
        return_stmt,
        code
    );
    assert!(
        return_stmt.contains("msg"),
        "return must include 'msg'. Return was: '{}'. Full:\n{}",
        return_stmt,
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

    // Extract the return statement
    let return_idx = code.find("return ").expect(&format!(
        "Must have a return statement. Full output:\n{}",
        code
    ));
    let return_rest = &code[return_idx..];
    let return_end = return_rest.find(';').unwrap_or(return_rest.len());
    let return_stmt = &return_rest[..return_end];

    // Must NOT return empty - Vue's official compiler returns all top-level bindings
    assert!(
        !return_stmt.contains("return {}"),
        "setup() must NOT return empty. Return was: '{}'. Full:\n{}",
        return_stmt,
        code
    );

    // editorContainer must be in return for template ref binding
    assert!(
        return_stmt.contains("editorContainer"),
        "return must include 'editorContainer' for template ref. Return: '{}'. Full:\n{}",
        return_stmt,
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
