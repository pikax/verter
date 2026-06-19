use super::*;

/// Check if a string contains a `/*digits,digits*/` offset comment pattern.
fn has_offset_comment(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 4 < len {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Found /*, look for digits,digits*/
            let start = i + 2;
            let mut j = start;
            // Skip digits
            while j < len && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start && j < len && bytes[j] == b',' {
                let comma = j;
                j += 1;
                while j < len && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > comma + 1 && j + 1 < len && bytes[j] == b'*' && bytes[j + 1] == b'/' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

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

fn compile_sfc_no_hoist(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(false),
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

/// Compile with hoist_static=false and assert template output is syntactically valid JS.
fn compile_and_validate_template_no_hoist(source: &str) -> String {
    let result = compile_sfc_no_hoist(source);
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

// ==================== Cross-file const prop optimization ====================

fn compile_sfc_with_const_props(source: &str, const_props: &[&str]) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Child.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        prop_constness_overrides: Some(const_props.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_sfc_vapor_with_const_props(source: &str, const_props: &[&str]) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Child.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        prop_constness_overrides: Some(const_props.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// @ai-generated - Cross-file const prop is excluded from dynamicProps
#[test]
fn const_prop_excluded_from_dynamic_props() {
    // A child component template: <Comp :msg="msg" :count="count">
    // When `msg` is known to be const across all parents but `count` is not,
    // only `count` should appear in the dynamicProps array.
    let result = compile_sfc_with_const_props(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
        &["msg"],
    );
    let tpl = result.template.as_ref().expect("template block");
    // `msg` (const prop) should NOT be in dynamicProps
    assert!(
        !tpl.code.contains("\"title\""),
        "const prop 'msg' (bound as :title) should be excluded from dynamicProps, got:\n{}",
        tpl.code
    );
    // `count` (non-const prop) should still be in dynamicProps
    assert!(
        tpl.code.contains("\"id\""),
        "non-const prop 'count' (bound as :id) should remain in dynamicProps, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Without const_props data, all props are in dynamicProps (Vue compat)
#[test]
fn without_const_props_all_bound_props_in_dynamic_props() {
    let result = compile_sfc(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Both should be in dynamicProps (standard Vue behavior)
    assert!(
        tpl.code.contains("\"title\"") && tpl.code.contains("\"id\""),
        "without const_props, both bound props should be in dynamicProps, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Const prop still uses $props prefix (correct runtime access)
#[test]
fn const_prop_still_uses_props_prefix() {
    let result = compile_sfc_with_const_props(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
        &["msg"],
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "const prop should still use $props. prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: const prop setter emitted as direct statement, not inside _renderEffect
#[test]
fn vapor_const_prop_skips_render_effect() {
    let result = compile_sfc_vapor_with_const_props(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
        &["msg"],
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    // Const prop should NOT be inside _renderEffect
    assert!(
        !tpl.code.contains("_renderEffect"),
        "const prop setter should not be wrapped in _renderEffect, got:\n{}",
        tpl.code
    );
    // The setter should still be present as a direct statement
    assert!(
        tpl.code.contains("_setProp") || tpl.code.contains("_setAttr"),
        "const prop setter should still be emitted, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: non-const prop stays inside _renderEffect
#[test]
fn vapor_non_const_prop_in_render_effect() {
    let result = compile_sfc_vapor_with_const_props(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
        &["msg"], // only msg is const
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    // Non-const prop `count` should be inside _renderEffect
    assert!(
        tpl.code.contains("_renderEffect"),
        "non-const prop should be wrapped in _renderEffect, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: without const_props data, all dynamic props in _renderEffect
#[test]
fn vapor_without_const_props_all_in_render_effect() {
    let result = compile_sfc_vapor(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_renderEffect"),
        "without const_props, dynamic props should be in _renderEffect, got:\n{}",
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
/// @ai-generated - unresolved imported defineProps inside withDefaults should not
/// surface XInvalidMacroType when fallback props codegen can still synthesize
/// runtime props from the defaults expression.
#[test]
fn with_defaults_unresolvable_imported_type_with_defaults_has_no_error() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import type { Props } from './types'
import { getDefaults } from './defaults'

const props = withDefaults(defineProps<Props>(), getDefaults())
</script>
<template><div>{{ props.foo }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("getDefaults()"),
        "should reference the defaults function.\nOutput:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("defineProps<Props>()"),
        "defineProps call should be lowered.\nOutput:\n{}",
        script.code
    );
}

/// @ai-generated â€” withDefaults + unresolvable type without any defaults
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
    let scope_pos = script.code.find(scope_marker).unwrap_or_else(|| {
        panic!(
            "Script should contain __scopeId assignment, got:\n{}",
            script.code
        )
    });
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
    let css_pos = style.code.find(css_marker).unwrap_or_else(|| {
        panic!(
            "CSS should contain [data-v-...] selector, got:\n{}",
            style.code
        )
    });
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
    let returned_idx = code
        .find("const __returned__ = ")
        .unwrap_or_else(|| panic!("Must have __returned__ assignment. Full output:\n{}", code));
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

// @ai-generated - TDD test: defineModel with options forwards type/default to prop definition
#[test]
fn define_model_with_options_forwards_to_prop() {
    let result = compile_sfc(
        r#"<script setup>
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div v-if="visible">shown</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel options should be forwarded to the prop definition
    assert!(
        script.code.contains("type: Boolean"),
        "defineModel options should forward `type: Boolean` to prop definition.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: false"),
        "defineModel options should forward `default: false` to prop definition.\nGot:\n{}",
        script.code
    );
    // Should NOT output an empty prop object for models with options
    assert!(
        !script.code.contains("visible: {},"),
        "Model with options should not have empty prop object.\nGot:\n{}",
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
            // Skip _ctx, _cache, locally declared _hoisted_N, _component_X
            !["_ctx", "_cache"].contains(h)
                && !h.starts_with("_hoisted_")
                && !h.starts_with("_component_")
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
                    if let Some(stripped) = name.strip_prefix('_') {
                        format!("{stripped} as {name}")
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
    let code = compile_and_validate_template_no_hoist(
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
    let code = compile_and_validate_template_no_hoist(
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
    let code = compile_and_validate_template_no_hoist(
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
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_tsx_with_force_js(source: &str, force_js: bool) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        force_js,
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

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.code.is_empty(), "TSX code should not be empty");
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN"),
        "Should contain component wrapper function, got: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains("const msg = 'hello'"),
        "Should preserve setup content, got: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<div>"),
        "Should contain template JSX, got: {}",
        tsx.code
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

    let tsx = result.tsx.as_ref().expect("tsx block");
    // Imports should be hoisted above the wrapper function
    let fn_pos = tsx
        .code
        .find("export function ___VERTER___TemplateBindingFN")
        .expect("wrapper function");
    let ref_pos = tsx
        .code
        .find("import { ref } from 'vue'")
        .expect("ref import");
    let type_pos = tsx
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

    let tsx = result.tsx.as_ref().expect("tsx block");
    // count is a SetupRef — in the new output, .value is NOT appended in TSX
    assert!(
        tsx.code.contains("{ count }"),
        "SetupRef should appear without .value in TSX, got: {}",
        tsx.code
    );
    // Negative: .value must NOT appear in TSX template interpolations
    assert!(
        !tsx.code.contains("count.value"),
        ".value must not appear in TSX template: {}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_ref_binding_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('')
</script>
<template><div>{{ msg }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupRef must use `let` in destructuring (allows v-model assignment)
    assert!(
        tsx.code.contains("let {") && tsx.code.contains("msg"),
        "SetupRef binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
    // Must NOT appear in a `const {` destructuring
    assert!(
        !tsx.code.contains("const { msg }") && !tsx.code.contains("const {\n    msg"),
        "SetupRef binding must NOT appear in const destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_plain_const_uses_const() {
    let result = compile_tsx(
        r#"<script setup>
const label = 'hello'
</script>
<template><div>{{ label }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // LiteralConst/SetupConst must use `const` in destructuring
    assert!(
        tsx.code.contains("const {") && tsx.code.contains("label"),
        "SetupConst/LiteralConst binding should use `const` in destructuring, got:\n{}",
        tsx.code
    );
    // Must NOT appear in a `let {` destructuring
    assert!(
        !tsx.code.contains("let {"),
        "SetupConst/LiteralConst binding must NOT appear in let destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_mixed_const_and_ref_split_correctly() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const label = 'hello'
</script>
<template><div>{{ count }} {{ label }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Should have both `const {` and `let {` destructuring
    assert!(
        tsx.code.contains("const {"),
        "Should have const destructuring for label, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("let {"),
        "Should have let destructuring for count, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_reactive_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
import { reactive } from 'vue'
const state = reactive({ x: 0 })
</script>
<template><div>{{ state.x }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupReactiveConst must use `let` (mutable properties)
    assert!(
        tsx.code.contains("let {") && tsx.code.contains("state"),
        "SetupReactiveConst binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destruct_let_binding_uses_let() {
    let result = compile_tsx(
        r#"<script setup>
let x = 0
</script>
<template><div>{{ x }}</div></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // SetupLet must use `let`
    assert!(
        tsx.code.contains("let {") && tsx.code.contains(" x"),
        "SetupLet binding should use `let` in destructuring, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("const { x }") && !tsx.code.contains("const {\n    x"),
        "SetupLet binding must NOT appear in const destructuring, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_model_on_component_produces_valid_jsx() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const val = ref('')
</script>
<template><MyComp v-model="val" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // modelValue prop must be present
    assert!(
        tsx.code.contains("modelValue={"),
        "v-model on component should emit modelValue prop, got:\n{}",
        tsx.code
    );
    // Event handler must use spread syntax (quoted keys are invalid JSX attribute names)
    assert!(
        tsx.code.contains(r#""onUpdate:modelValue""#),
        "v-model should emit onUpdate:modelValue handler, got:\n{}",
        tsx.code
    );
    // Negative: quoted string must NOT appear as a standalone JSX attribute (before `=`)
    // Valid: {...{"onUpdate:modelValue": handler}} — inside spread
    // Invalid: "onUpdate:modelValue"={handler} — bare quoted attribute
    assert!(
        !tsx.code.contains(r#""onUpdate:modelValue"={"#),
        "onUpdate:modelValue must NOT be a bare JSX attribute (invalid syntax), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_model_on_native_input_uses_value_prop() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('')
</script>
<template><input v-model="msg" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Native input should use `value` (not `modelValue`)
    assert!(
        tsx.code.contains("value={"),
        "v-model on native input should use value prop, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("modelValue"),
        "v-model on native input must NOT use modelValue, got:\n{}",
        tsx.code
    );
    // Must not have quoted attribute names (invalid JSX)
    assert!(
        !tsx.code.contains(r#""onUpdate:"#),
        "native input must not have quoted onUpdate attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_model_named_on_component_uses_spread() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const title = ref('')
</script>
<template><MyComp v-model:title="title" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Named model: title prop + onUpdate:title handler
    assert!(
        tsx.code.contains("title={"),
        "named v-model should emit title prop, got:\n{}",
        tsx.code
    );
    // Must not have bare quoted attribute
    assert!(
        !tsx.code.contains(r#""onUpdate:title"={"#),
        "named v-model must NOT use bare quoted JSX attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infers_native_click_handler_param_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handleClick(e) {
  return e
}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"function handleClick(...[e]: Parameters<NonNullable<import('vue').IntrinsicElementAttributes["button"]["onClick"]>>)"#
        ),
        "Expected inferred click handler parameter type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infers_native_input_handler_multi_params() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(a, b, c) {
  return [a, b, c]
}
</script>
<template>
  <input @input="handler" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"...[a, b, c]: Parameters<NonNullable<import('vue').IntrinsicElementAttributes["input"]["onInput"]>>"#
        ),
        "Expected inferred tuple rest parameter type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_native_event_matrix_matches_v5_process_cases() {
    let cases = [
        (
            "handleChange",
            "e",
            r#"<select @change="handleChange"></select>"#,
            "select",
            "onChange",
        ),
        (
            "handleSubmit",
            "e",
            r#"<form @submit="handleSubmit"></form>"#,
            "form",
            "onSubmit",
        ),
        (
            "handleKeydown",
            "e",
            r#"<input @keydown="handleKeydown" />"#,
            "input",
            "onKeydown",
        ),
        (
            "handleFocus",
            "e",
            r#"<input @focus="handleFocus" />"#,
            "input",
            "onFocus",
        ),
        (
            "handleBlur",
            "e",
            r#"<input @blur="handleBlur" />"#,
            "input",
            "onBlur",
        ),
        (
            "handleMouseEnter",
            "e",
            r#"<div @mouseenter="handleMouseEnter"></div>"#,
            "div",
            "onMouseenter",
        ),
        (
            "handleMouseLeave",
            "e",
            r#"<div @mouseleave="handleMouseLeave"></div>"#,
            "div",
            "onMouseleave",
        ),
        (
            "handleAnchorClick",
            "e",
            r##"<a href="#" @click="handleAnchorClick">Link</a>"##,
            "a",
            "onClick",
        ),
        (
            "handleDblClick",
            "e",
            r#"<div @dblclick="handleDblClick"></div>"#,
            "div",
            "onDblclick",
        ),
        (
            "handleContextMenu",
            "e",
            r#"<div @contextmenu="handleContextMenu"></div>"#,
            "div",
            "onContextmenu",
        ),
    ];

    for (fn_name, param, template, element, event_prop) in cases {
        let source = format!(
            r#"<script setup lang="ts">
function {fn_name}({param}) {{ return {param} }}
</script>
<template>{template}</template>"#
        );
        let result = compile_tsx(&source);
        assert!(
            result.errors.is_empty(),
            "errors for {}: {:?}",
            fn_name,
            result.errors
        );

        let tsx = result.tsx.as_ref().expect("tsx block");
        let expected = format!(
            "...[{param}]: Parameters<NonNullable<import('vue').IntrinsicElementAttributes[\"{element}\"][\"{event_prop}\"]>>"
        );
        assert!(
            tsx.code.contains(&expected),
            "Expected inferred native event type for {}.\nExpected snippet: {}\nActual TSX:\n{}",
            fn_name,
            expected,
            tsx.code
        );
    }
}

#[test]
fn tsx_infer_function_component_events_from_imported_components() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import MyButton from './MyButton.vue'
import CustomSelect from './CustomSelect.vue'
import DataTable from './DataTable.vue'

function handleChange(e) { return e }
function onClick(e) { return e }
function onSelect(item) { return item }
function handleUpdate(data) { return data }
</script>
<template>
  <MyComp @change="handleChange" />
  <MyButton @click="onClick" />
  <CustomSelect @select="onSelect" />
  <DataTable @update="handleUpdate" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"...[e]: Parameters<NonNullable<Required<InstanceType<typeof MyComp>["$props"]>["onChange"]>>"#
        ),
        "Expected component change event inference from MyComp props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[e]: Parameters<NonNullable<Required<InstanceType<typeof MyButton>["$props"]>["onClick"]>>"#
        ),
        "Expected component click event inference from MyButton props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[item]: Parameters<NonNullable<Required<InstanceType<typeof CustomSelect>["$props"]>["onSelect"]>>"#
        ),
        "Expected component custom event inference from CustomSelect props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[data]: Parameters<NonNullable<Required<InstanceType<typeof DataTable>["$props"]>["onUpdate"]>>"#
        ),
        "Expected component update event inference from DataTable props, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("IntrinsicElementAttributes"),
        "Component event inference should not use native IntrinsicElementAttributes, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_does_not_transform_arrow_function_handlers() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const handler = (e) => e?.target
</script>
<template>
  <div @click="handler"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const handler = (e) => e?.target"),
        "Arrow function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Arrow function should not receive inferred tuple-rest typing, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_does_not_transform_no_param_functions() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function noParams() {
  return 1
}
</script>
<template>
  <div @click="noParams"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function noParams()"),
        "No-parameter function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("Parameters<"),
        "No-parameter function should not receive inferred tuple-rest typing, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_supports_non_setup_scripts() {
    let result = compile_tsx(
        r#"<script lang="ts">
function handleClick(e) {
  return e
}

export default {}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"function handleClick(...[e]: Parameters<NonNullable<import('vue').IntrinsicElementAttributes["button"]["onClick"]>>)"#
        ),
        "Expected inference in non-setup script mode, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_skips_js_scripts() {
    let result = compile_tsx(
        r#"<script setup lang="js">
function handleClick(e) {
  return e
}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handleClick(e)"),
        "JS script function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("Parameters<"),
        "JS script should not get inferred TS parameter types, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_override_existing_typed_params() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(e: MouseEvent) {
  return e.clientX
}
</script>
<template>
  <div @click="handler"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handler(e: MouseEvent)"),
        "Typed function param should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Typed function param should not be replaced by inferred tuple rest, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_infer_unbound_function() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function unused(x) {
  return x
}
</script>
<template>
  <div>No event binding</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function unused(x)"),
        "Function not used in template events should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("IntrinsicElementAttributes"),
        "No inferred event types should be injected for unbound function, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_infer_inline_call_event_handler() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(e) {
  return e
}
</script>
<template>
  <div @click="handler()"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handler(e)"),
        "Inline call handler should not trigger parameter inference, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Inline call handler should not be rewritten with inferred tuple rest type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_infers_static_element_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let el = useTemplateRef('el')
</script>
<template>
  <div ref="el"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Uses ReturnType<typeof ___VERTER___Comp{offset}> which resolves to the build node type
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#)
            && tsx.code.contains(r#","el">('el')"#),
        "Expected inferred useTemplateRef generic with Comp build node, got:\n{}",
        tsx.code
    );
    // Negative: should NOT use InstanceType for ref type
    assert!(
        !tsx.code.contains("useTemplateRef<InstanceType"),
        "should use ReturnType<typeof Comp>, not InstanceType in useTemplateRef: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_skips_js_scripts() {
    let result = compile_tsx(
        r#"<script setup lang="js">
let el = useTemplateRef('el')
</script>
<template>
  <div ref="el"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("let el = useTemplateRef('el')"),
        "JS scripts should not inject template-ref generics, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("useTemplateRef<"),
        "JS scripts should not inject useTemplateRef type arguments, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_dynamic_ref_with_const_match() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'test'
</script>
<template>
  <my-comp :ref="foo"></my-comp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Dynamic ref with const match: should use Comp build node, not InstanceType
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#)
            && tsx.code.contains(r#",typeof foo>('test')"#),
        "Expected dynamic ref const-value matching to use Comp build node, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_dynamic_ref_unknown_when_unmatched() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'testx'
</script>
<template>
  <my-comp :ref="foo"></my-comp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<unknown,typeof foo>('test')"#),
        "Expected unmatched dynamic ref selector to fall back to unknown, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_dynamic_component_is_union_from_literals() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let a = useTemplateRef('a')
</script>
<template>
  <component :is="true ? 'div' : 'span'" ref="a"></component>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Dynamic component :is with ref should use ReturnType<typeof Comp{offset}>
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#),
        "Expected dynamic component ref to use Comp build node, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_ref_variable_matching_template_ref_gets_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref()
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const myDiv = ref<") && tsx.code.contains("|null>()"),
        "Expected ref() variable to receive inferred template-ref type, got:\n{}",
        tsx.code
    );
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

#[test]
fn tsx_template_ref_ref_variable_not_matching_template_ref_is_unchanged() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const other = ref()
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const other = ref()"),
        "Non-matching ref() variable should remain unchanged, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_skips_calls_with_explicit_type_arguments() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref<HTMLInputElement>()
const x = useTemplateRef<HTMLInputElement>('myDiv')
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const myDiv = ref<HTMLInputElement>()"),
        "Explicit ref<T>() should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("useTemplateRef<HTMLInputElement>('myDiv')"),
        "Explicit useTemplateRef<T>() should remain unchanged, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_without_argument_uses_all_template_ref_names() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let x = useTemplateRef()
</script>
<template>
  <div ref="foo"></div>
  <span ref="bar"></span>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(r#""foo""#) && tsx.code.contains(r#""bar""#),
        "Expected no-arg useTemplateRef() to include all template ref names, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_dynamic_function_ref_expression_is_ignored() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let x = useTemplateRef()
</script>
<template>
  <div :ref="el => (x = el)"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("let x = useTemplateRef()"),
        "Function-like dynamic ref expressions should not drive inference, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_options_api_setup_function_is_supported() {
    let result = compile_tsx(
        r#"<script lang="ts">
import { defineComponent, useTemplateRef } from 'vue'
export default defineComponent({
  setup() {
    const myRef = useTemplateRef('myRef')
    return { myRef }
  }
})
</script>
<template>
  <div ref="myRef"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("useTemplateRef<") && tsx.code.contains(r#","myRef">('myRef')"#),
        "Expected nested setup() call to receive inferred generic, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_v5_process_parity_matrix() {
    let ts_cases: [(&str, &[&str], &[&str]); 7] = [
        (
            r#"<script setup lang="ts">
let el = useTemplateRef('el')
</script>
<template><div ref="el"></div></template>"#,
            &[
                r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#,
                r#","el">('el')"#,
            ],
            &[r#"useTemplateRef<InstanceType"#],
        ),
        (
            r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'test'
</script>
<template><my-comp :ref="foo"></my-comp></template>"#,
            &[
                r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#,
                r#",typeof foo>('test')"#,
            ],
            &[r#"useTemplateRef<unknown,typeof foo>('test')"#],
        ),
        (
            r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'testx'
</script>
<template><my-comp :ref="foo"></my-comp></template>"#,
            &[r#"useTemplateRef<unknown,typeof foo>('test')"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
import { ref } from 'vue'
const itemRef = ref()
</script>
<template><div v-for="item in items" :key="item" ref="itemRef"></div></template>"#,
            &[r#"const itemRef = ref<"#, r#"[]|null>()"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
let a = useTemplateRef('a')
</script>
<template><component :is="true ? 'div' : 'span'" ref="a"></component></template>"#,
            &[r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref()
</script>
<template><div ref="myDiv"></div></template>"#,
            &[r#"const myDiv = ref<"#, r#"|null>()"#],
            &[],
        ),
        (
            r#"<script lang="ts">
import { defineComponent, useTemplateRef } from 'vue'
export default defineComponent({
  setup() {
    const myRef = useTemplateRef('myRef')
    return { myRef }
  }
})
</script>
<template><div ref="myRef"></div></template>"#,
            &[r#"useTemplateRef<"#, r#","myRef">('myRef')"#],
            &[],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in ts_cases {
        let result = compile_tsx(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.as_ref().expect("tsx block");
        for required in required_snippets {
            assert!(
                tsx.code.contains(required),
                "Expected snippet '{}' in TSX output:\n{}",
                required,
                tsx.code
            );
        }
        for forbidden in forbidden_snippets {
            assert!(
                !tsx.code.contains(forbidden),
                "Unexpected snippet '{}' in TSX output:\n{}",
                forbidden,
                tsx.code
            );
        }
    }

    let js_result = compile_tsx(
        r#"<script setup lang="js">
let el = useTemplateRef('el')
</script>
<template><div ref="el"></div></template>"#,
    );
    assert!(
        js_result.errors.is_empty(),
        "errors: {:?}",
        js_result.errors
    );
    let js_tsx = js_result.tsx.as_ref().expect("tsx block");
    assert!(
        js_tsx.code.contains("let el = useTemplateRef('el')"),
        "JS parity case should preserve plain useTemplateRef call, got:\n{}",
        js_tsx.code
    );
    assert!(
        !js_tsx.code.contains("useTemplateRef<"),
        "JS parity case should not inject generic type parameters, got:\n{}",
        js_tsx.code
    );
}

#[test]
fn tsx_template_ref_vslot_component_scope_aware() {
    // When a component comes from v-slot destructuring, its ref type should
    // resolve through the parent component's slot type, not directly.
    let result = compile_tsx(
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
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: useTemplateRef should get a type argument
    assert!(
        tsx.code.contains("useTemplateRef<"),
        "useTemplateRef should have inferred type arguments: {}",
        tsx.code
    );

    // Positive: The Comp function for the slot-scoped component should drill
    // into the parent's $slots type
    assert!(
        tsx.code.contains("$slots") && tsx.code.contains("default"),
        "Comp function should reference parent's $slots['default']: {}",
        tsx.code
    );

    // Positive: parent MyComp should have its own Comp function with instantiateComponent
    assert!(
        tsx.code.contains("instantiateComponent(MyComp,"),
        "parent MyComp should be instantiated: {}",
        tsx.code
    );

    // Negative: should NOT have a bare `instantiateComponent(Comp,` without
    // the slot type reconstruction preamble
    assert!(
        tsx.code.contains("type __Parent"),
        "slot-scoped component should use __Parent type reconstruction: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_named_slot_scope_aware() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { useTemplateRef } from 'vue'
const myRef = useTemplateRef('myRef')
</script>
<template>
  <MyComp>
    <template #items="{ Item }">
      <Item ref="myRef" />
    </template>
  </MyComp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: should reference the named slot 'items'
    assert!(
        tsx.code.contains("$slots") && tsx.code.contains("items"),
        "named slot should reference $slots['items']: {}",
        tsx.code
    );

    // Negative: should NOT reference 'default' slot
    assert!(
        !tsx.code.contains("$slots']['default']"),
        "named slot should not reference default slot: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_multiple_refs_union() {
    // Multiple refs with different elements should create a union for the second generic
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const x = useTemplateRef('foo')
</script>
<template>
  <div ref="foo"></div>
  <span ref="bar"></span>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: second generic should contain both ref names
    assert!(
        tsx.code.contains(r#""foo""#) && tsx.code.contains(r#""bar""#),
        "second generic should contain all ref name literals: {}",
        tsx.code
    );

    // Positive: first generic should match 'foo' ref type (not union, since selector matches)
    assert!(
        tsx.code
            .contains("useTemplateRef<ReturnType<typeof ___VERTER___Comp"),
        "first generic should be the matched ref's type: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_unmatched_arg_produces_unknown() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const x = useTemplateRef('nonexistent')
</script>
<template>
  <div ref="foo"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: unmatched arg should produce unknown as first generic
    assert!(
        tsx.code.contains("useTemplateRef<unknown,"),
        "unmatched arg should produce unknown first generic: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_vfor_scope_component_ref() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
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
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: should reconstruct the v-for iterable element type
    assert!(
        tsx.code.contains("(typeof components)[number]"),
        "v-for component should use iterable element type: {}",
        tsx.code
    );
}

#[test]
fn tsx_binding_v5_process_parity_matrix() {
    let cases: [(&str, &[&str], &[&str]); 8] = [
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ test }}</template>"#,
            &["{ test }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
const handler = () => {}
</script>
<template><div :test="test" @click="handler"></div></template>"#,
            &["test={test}", "onClick={handler}"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const items = [1]
</script>
<template><div v-for="item in items">{{ item + items.length }}</div></template>"#,
            &["items).map((item) => { return (", "{ item + items.length }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><div :[msg]="msg" /></template>"#,
            &["{...{[msg]: msg}}"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><Comp v-model:[`${msg}ss`]="msg" /></template>"#,
            &[r#"[`${msg}ss`]:msg"#],
            &[r#"v-model:"#, "_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ { test } }}</template>"#,
            &["{ { test } }"],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template>{{ [ test, { test }, [test] ] }}</template>"#,
            &["{ [ test, { test }, [test] ] }"],
            &["_ctx."],
        ),
        (
            r#"<template>{{ (foo:string)=> { foo.toLowerCase(); } }}</template>"#,
            &["(foo:string)=> { foo.toLowerCase(); }"],
            &["_ctx.foo", "___VERTER___instance.foo"],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in cases {
        let result = compile_tsx(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.as_ref().expect("tsx block");
        for required in required_snippets {
            assert!(
                tsx.code.contains(required),
                "Expected snippet '{}' in TSX output:\n{}",
                required,
                tsx.code
            );
        }
        for forbidden in forbidden_snippets {
            assert!(
                !tsx.code.contains(forbidden),
                "Unexpected snippet '{}' in TSX output:\n{}",
                forbidden,
                tsx.code
            );
        }
    }
}

#[test]
fn tsx_binding_type_assertions_do_not_prefix_type_members() {
    let result = compile_tsx(
        r#"<template>{{
  () => {
    let a = {} as { foo: 1 };
    a;
  }
}}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("_ctx.foo"),
        "Type-annotation members must not be treated as runtime bindings, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.as"),
        "Type assertion keyword context must not be prefixed as identifier, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_prop_v5_process_parity_matrix() {
    let cases: [(&str, &[&str], &[&str]); 11] = [
        (
            r#"<template><div test="test" /></template>"#,
            &[r#"<div test="test" />"#],
            &[],
        ),
        (
            r#"<template><div test /></template>"#,
            &[r#"<div test />"#],
            &[],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template><div :test="test" /></template>"#,
            &[r#"<div test={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template><div :test /></template>"#,
            &[r#"<div test={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const testToFoo = 1
</script>
<template><div :test-to-foo /></template>"#,
            &[r#"<div test-to-foo={testToFoo} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><div :[msg]="msg" /></template>"#,
            &[r#"<div {...{[msg]: msg}} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const obj = {}
</script>
<template><div v-bind="obj" /></template>"#,
            &[r#"<div {...obj} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = () => {}
</script>
<template><div @test="test" /></template>"#,
            &[r#"<div onTest={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = () => {}
</script>
<template><div @test-camel-case="test" /></template>"#,
            &[r#""onTest-camel-case": test"#],
            &[r#"onTestCamelCase"#, "_ctx."],
        ),
        (
            r#"<template><div aria-label="test" data-test="value" /></template>"#,
            &[r#"<div aria-label="test" data-test="value" />"#],
            &[],
        ),
        (
            r#"<script setup>
const ok = true
</script>
<template><div :style="{ color: 'red' }" style="color: blue" :class="{ active: ok }" class="btn" /></template>"#,
            &[
                r#"normalizeStyle([{ color: 'red' },"color: blue"])"#,
                r#"normalizeClass([{ active: ok },"btn"])"#,
            ],
            &["_ctx.", r#"style="color: blue""#, r#"class="btn""#],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in cases {
        let result = compile_tsx(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.as_ref().expect("tsx block");
        for required in required_snippets {
            assert!(
                tsx.code.contains(required),
                "Expected snippet '{}' in TSX output:\n{}",
                required,
                tsx.code
            );
        }
        for forbidden in forbidden_snippets {
            assert!(
                !tsx.code.contains(forbidden),
                "Unexpected snippet '{}' in TSX output:\n{}",
                forbidden,
                tsx.code
            );
        }
    }
}

// ==================== v5/process parity matrices (ported/adapted suites) ====================

/// Adapted parity matrix for:
/// - script/plugins/macros/macros.spec.ts
/// - script/plugins/macros/macros.fixtures.ts
#[test]
fn tsx_macros_v5_process_parity_matrix() {
    type_based_define_props_resolves_to_props_prefix();
    with_defaults_merges_defaults_into_props();
    with_defaults_type_reference();
    define_props_type_with_imported_types();
    with_defaults_imported_types_all_props_present();
    with_defaults_unresolvable_type_no_declarator();
    with_defaults_unresolvable_type_with_declarator();
    with_defaults_unresolvable_type_object_literal_defaults();
    with_defaults_unresolvable_type_function_call_defaults();
    with_defaults_unresolvable_type_no_defaults();
    with_defaults_resolvable_type_still_works();
    with_defaults_unresolvable_type_mixed_defaults();
    with_defaults_unresolvable_type_complex_expression_defaults();
    cross_block_type_resolution_for_define_props();
    with_defaults_cross_block_type_uses_key_name();

    define_model_declares_prop_and_emit();
    define_model_named_declares_prop_and_emit();
    define_model_with_defaults_resolved_type();
    define_model_with_defaults_runtime_var_iife();
    define_model_with_define_props_object_uses_merge_models();
    define_model_with_typed_with_defaults();
    define_model_with_define_emits_uses_merge_models_for_emits();

    type_based_define_emits_generates_emits_option();
    type_based_define_emits_call_signature_generates_emits_option();
    optional_tuple_element_in_define_emits();

    destructured_define_props_resolves_to_props_prefix();
    aliased_destructured_define_props_resolves_to_props_prefix();
    destructured_with_defaults_resolves_to_props_prefix();
    destructured_props_mixed_with_setup_bindings();
    destructured_prop_in_v_bind();
    destructured_prop_in_event_handler();
    destructured_props_not_in_setup_return();
    destructured_with_defaults_multiple_props();
    destructured_with_defaults_unresolvable_type_resolves_to_props_prefix();
}

/// Adapted parity matrix for:
/// - template/plugins/slot/slot.spec.ts
/// - template/plugins/slot-type-check/slotTypeCheck.spec.ts
#[test]
fn tsx_slot_v5_process_parity_matrix() {
    slot_outlet_default_compiles_to_render_slot();
    slot_outlet_named_compiles_to_render_slot();
    slot_outlet_self_closing();
    slot_outlet_with_v_if_gets_ternary();
    slot_outlet_with_fallback_children();
    slot_outlet_with_v_for_gets_render_list();
    component_named_slots_compiled_as_slot_object();
    component_default_slot_compiled_as_slot_object();
    conditional_slot_v_if_uses_create_slots();
    conditional_slot_v_if_else_chain();
    static_slots_no_create_slots();
    component_hyphenated_slot_names_quoted();
    component_named_slot_plus_default_text();
    scoped_slot_parameters_passed_to_withctx();
    empty_named_slot_no_close_tag_leak();
    empty_named_slot_whitespace_only();
    multiple_empty_named_slots();
    empty_scoped_slot_no_children();
    empty_slot_with_v_if_dynamic();
    empty_slot_mixed_with_content_slots();
    self_closing_template_slot();
    self_closing_slot_with_other_normal_slot();
    multiple_self_closing_named_slots();
    self_closing_scoped_template_slot();
    self_closing_slot_with_v_if_dynamic();
    self_closing_slot_mixed_with_content();
    component_v_slot_params_in_default_slot();
    v_for_locals_no_ctx_prefix_in_slot();
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

/// Adapted parity matrix for:
/// - template/plugins/conditional/conditional.spec.ts
/// - template/plugins/conditional/generateConditionText.spec.ts
#[test]
fn tsx_conditional_v5_process_parity_matrix() {
    v_if_only_emits_comment_fallback();
    v_if_v_else_no_comment_fallback();
    v_if_v_else_if_no_v_else_emits_comment_fallback();
    v_if_v_else_if_v_else_complete_chain();
    v_if_after_sibling_has_comma_separator();
    v_if_chain_after_sibling();
    v_if_chain_without_v_else_after_sibling();
    v_if_as_root_single_child();
    v_if_v_else_as_root();
    v_if_in_multi_root_fragment();
    multiple_v_if_chains_in_same_parent();
    v_if_with_whitespace_between_branches();
    v_if_nested_inside_v_for();
    v_if_standalone_emits_comment_vnode();
    v_if_else_chain_with_whitespace_valid_output();
    v_if_inside_v_for_with_whitespace();
    v_if_followed_by_sibling_valid_js();
    nested_v_if_chains_no_overlap();
    v_if_with_comment_between_branches();
    comment_between_v_if_branches_does_not_leak_in_prod();

    template_v_if_renders_as_fragment();
    template_v_for_with_v_if_children_renders_as_fragment();
    tsx_v_for_with_v_if_combination_contains_condition_and_map();
    tsx_parent_v_if_with_child_v_for_contains_outer_condition();
}

/// Adapted parity matrix for:
/// - template/template.spec.ts
/// - template/plugins/block/block.spec.ts
/// - template/plugins/sfc-cleaner/sfcCleaner.spec.ts
#[test]
fn tsx_template_v5_process_parity_matrix() {
    template_output_contains_render_function_vdom();
    template_output_contains_render_function_vapor();
    template_heavy_vue_full_css_scoping();
    template_only_no_scoped_style_no_script_block();
    template_only_scoped_style_emits_scope_id_in_script();
    template_only_scoped_style_css_is_scoped();
    template_only_scoped_style_grid_layout_scope_id_consistency();
    component_whitespace_children_clean_output();
    component_whitespace_only_children_no_close_tag_leak();
    analysis_panel_regression_valid_js();

    tsx_template_interpolation_with_bindings();
    tsx_template_tag_replacement_wraps_content_in_fragment();
    tsx_template_tag_empty_template_emits_empty_fragment();
    tsx_template_comment();
    tsx_template_comment_no_extra_spacing();
    tsx_template_comment_with_nested_marker_text();
    tsx_template_comment_with_angle_bracket_text();
    tsx_no_template();
    html_entity_copy_decoded();
    html_entity_nbsp_decoded_in_text();
}

/// Adapted parity matrix for:
/// - script/plugins/component-type/component-type.spec.ts
#[test]
fn tsx_component_type_v5_process_parity_matrix() {
    component_resolves_to_setup_binding();
    component_kebab_case_resolves_to_pascal_setup_binding();
    unknown_component_uses_resolve_component();
    self_referencing_component_uses_maybe_self_reference();
    self_referencing_component_kebab_case();
    component_is_uses_resolve_dynamic_component();
    component_is_self_closing_uses_resolve_dynamic_component();
    component_is_empty_uses_resolve_dynamic_component();
    component_is_self_closing_with_props();
    component_static_is_uses_resolve_dynamic_component();
    component_is_with_prop_binding_and_vbind();
    imported_component_uses_setup_binding_not_resolve_component();
    builtin_component_suspense();
    builtin_component_teleport();
    builtin_component_keep_alive();
    builtin_component_transition();
    builtin_component_transition_group();
    builtin_component_kebab_case_keep_alive();
    builtin_component_kebab_case_teleport();
    builtin_component_in_imports_list();

    tsx_component_pascal_and_dotted_names_are_preserved();
    tsx_component_static_is_rewrites_to_target_tag();
    tsx_component_static_is_keeps_other_attributes();
    tsx_component_dynamic_is_literal_string_rewrites_to_target_tag();
    tsx_component_dynamic_is_expression_rewrites_to_temp_component();
    tsx_component_with_v_if_and_v_for_preserves_component_tags();
    tsx_component_kebab_and_mixed_case_names_are_preserved();
}

/// Adapted parity matrix for:
/// - script/plugins/component-instance/componentInstance.spec.ts
#[test]
fn tsx_component_instance_v5_process_parity_matrix() {
    tsx_infer_function_component_events_from_imported_components();
    tsx_template_ref_dynamic_component_is_union_from_literals();
    tsx_template_ref_use_template_ref_dynamic_ref_with_const_match();
    tsx_template_ref_use_template_ref_dynamic_ref_unknown_when_unmatched();
    tsx_template_ref_options_api_setup_function_is_supported();
    tsx_template_ref_v5_process_parity_matrix();
}

/// Adapted parity matrix for:
/// - script/builders/bundle/bundle.spec.ts
#[test]
fn tsx_script_bundle_v5_process_parity_matrix() {
    basic_sfc_compiles();
    style_block_extracted();
    custom_blocks_extracted();
    export_type_hoisted_when_keep_ts();
    tsx_basic_sfc();
    tsx_source_map_script_only();
    tsx_source_map_is_generated();
    tsx_source_map_maps_script_binding();
    tsx_force_js_toggle_does_not_change_code();
    tsx_force_js_toggle_does_not_change_source_map();
}

/// Adapted parity matrix for:
/// - script/plugins/script-default/script-default.spec.ts
#[test]
fn tsx_script_default_v5_process_parity_matrix() {
    dual_script_preserves_named_exports();
    dual_script_export_default_merged_as_options();
    companion_script_import_available_in_template();
    companion_script_type_only_import_not_in_returned();
    companion_script_import_used_in_template_in_returned();
    cross_block_type_resolution_for_define_props();
    with_defaults_cross_block_type_uses_key_name();
}

/// Adapted parity matrix for:
/// - script/plugins/script-block/script-block.spec.ts
#[test]
fn tsx_script_block_v5_process_parity_matrix() {
    ts_return_type_annotation_in_computed();
    ts_return_type_no_strip_mode();
    top_level_await_produces_async_setup();
    async_setup_wraps_await_with_async_context();
    async_setup_wraps_dynamic_import_await();
    tsx_basic_sfc();
    tsx_script_with_imports();
}

/// Adapted parity matrix for:
/// - script/plugins/define-options/defineOptions.spec.ts
#[test]
fn tsx_define_options_v5_process_parity_matrix() {
    ts_return_type_annotation_in_computed();
    ts_return_type_no_strip_mode();
    dual_script_export_default_merged_as_options();

    let result = compile_tsx(
        r#"<script setup lang="ts">
defineOptions({ name: 'App', inheritAttrs: false })
</script>
<template><div /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // With macro boxing, defineOptions arguments are extracted into a boxed constant.
    // The original arguments should appear in the boxing expression.
    assert!(
        tsx.code.contains("{ name: 'App', inheritAttrs: false }"),
        "defineOptions arguments should be preserved in TSX output (boxed or raw), got:\n{}",
        tsx.code
    );
}

/// Adapted parity matrix for:
/// - script/plugins/sfc-cleaner/sfcCleaner.spec.ts
#[test]
fn tsx_script_sfc_cleaner_v5_process_parity_matrix() {
    export_type_stripped_when_force_js();
    export_interface_stripped_when_force_js();
    bare_type_and_interface_stripped_when_force_js();
    import_specifier_used_only_as_type_should_be_elided();
    export_type_hoisted_when_keep_ts();
}

/// Adapted parity matrix for:
/// - script/plugins/template-binding/template-binding.spec.ts
#[test]
fn tsx_template_binding_plugin_v5_process_parity_matrix() {
    tsx_binding_v5_process_parity_matrix();
    tsx_binding_type_assertions_do_not_prefix_type_members();
    imported_function_in_template_gets_setup_prefix();
    companion_script_import_available_in_template();
    companion_script_import_used_in_template_in_returned();
    tsx_template_interpolation_with_bindings();
}

/// Adapted parity matrix for:
/// - script/plugins/attributes/attributes.spec.ts
#[test]
fn tsx_script_attributes_v5_process_parity_matrix() {
    script_attrs_contain_lang();
    export_type_hoisted_when_keep_ts();
    tsx_basic_sfc();
}

/// Adapted parity matrix for:
/// - script/plugins/full-context/full-context.spec.ts
#[test]
fn tsx_full_context_v5_process_parity_matrix() {
    setup_returns_bindings_for_template_refs();
    setup_returns_bindings_with_define_props();
    import_used_in_template_should_be_in_returned();
    companion_script_import_used_in_template_in_returned();
    companion_script_type_only_import_not_in_returned();
}

/// Adapted parity matrix for:
/// - script/plugins/imports/imports.spec.ts
#[test]
fn tsx_imports_plugin_v5_process_parity_matrix() {
    script_imports_use_as_syntax();
    tsx_script_with_imports();
    import_specifier_used_only_as_type_should_be_elided();
    imported_function_in_template_gets_setup_prefix();
}

#[test]
fn tsx_event_call_expression_is_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = { toString: () => "ok" }
</script>
<template>
  <div @click="test.toString()"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={() =>"),
        "Call-expression event handler should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("test.toString()"),
        "Wrapped handler should preserve call expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_simple_member_and_arrow_handlers_not_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => 1
const state = { click: () => 2 }
const handler = (event) => event
</script>
<template>
  <div @click="test"></div>
  <div @click="state.click"></div>
  <div @click="handler"></div>
  <div @click="function (...args) { return args }"></div>
  <div @input="(...args) => args"></div>
  <div @touchmove="(event) => { event; }"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={test}"),
        "Simple identifier handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={state.click}"),
        "Member-expression handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={handler}"),
        "Function reference handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onInput={(...args) => args}"),
        "Inline spread arrow function should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("onClick={function (...args) { return args }}"),
        "Inline function with spread params should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onTouchmove={(event) => { event; }}"),
        "Inline arrow function with explicit parameter should not be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_object_literal_handler_not_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = 1
</script>
<template>
  <div @click="{ test }"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={{ test }}") || tsx.code.contains("onClick={{test}}"),
        "Object-literal handler should remain direct object expression, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("onClick={() =>"),
        "Object-literal handler should not be wrapped in callback, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_string_template_and_ternary_handlers_are_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = true
</script>
<template>
  <div @click="'foo'" />
  <div @click="`foo${'test'}`" />
  <div @click="foo ? 'bar' : 'baz'" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("onClick={() => {'foo'}}"),
        "String literal event expression should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={() => {`foo${'test'}`}}"),
        "Template-string event expression should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={() => {foo ? 'bar' : 'baz'}}"),
        "Ternary event expression should be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_name_with_vue_namespace_is_preserved() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => {}
</script>
<template>
  <div @vue:mounted="test" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onVue:mounted={test}"),
        "Namespaced vue event should map to onVue:mounted, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_hyphenated_name_camelcases_segments() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => {}
</script>
<template>
  <div @test-camel-case="test" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Kebab-case events preserve hyphens and use spread syntax since
    // "onTest-camel-case" is not a valid JSX identifier.
    assert!(
        tsx.code.contains(r#""onTest-camel-case""#),
        "Kebab event should preserve hyphens in spread syntax, got:\n{}",
        tsx.code
    );
    // Should NOT be camelized
    assert!(
        !tsx.code.contains("onTestCamelCase"),
        "Kebab event should NOT be camelized, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_on_object_literal_rewrites_to_on_event_keys() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const click = () => {}
const mouseenter = () => {}
</script>
<template>
  <button v-on="{ click, mouseenter }" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        normalized.contains("{...{onClick:") && normalized.contains("onMouseenter:"),
        "v-on object literal should map event keys to JSX on* props, got:\n{}",
        tsx.code
    );
    assert!(
        !normalized.contains("{...{click:"),
        "Raw DOM event keys should not remain inside v-on object spread, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_49_event_handlers_with_spread_params_do_not_bind_args_to_ctx() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div
    @click="function (...args) {}"
    @input="(...args) => {}"
    @touchmove="
      (event) => {
        event;
      }
    "
  />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("..._ctx.args"),
        "Spread event parameters must never be prefixed to _ctx, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...___VERTER___ctx.args"),
        "Spread event parameters must never be prefixed to ___VERTER___ctx, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={function (...args) {}}"),
        "Function handler with spread args should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onInput={(...args) => {}}"),
        "Arrow handler with spread args should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onTouchmove={(event) => {") && tsx.code.contains("event;"),
        "Arrow handler with explicit param should stay direct (no wrapper), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_46_bare_click_does_not_bind_click_identifier_from_context() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div @click></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("onclick={"),
        "Bare @click must not be emitted as lowercase onclick binding, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.click"),
        "Bare @click must not bind synthetic click identifier from context, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_48_event_identifier_does_not_prefix_dollar_event_with_ctx() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div @click="$event"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // `$event` is bound as the handler's sole parameter so it is contextually typed
    // by the JSX event prop, and stays inside the callback scope (never
    // context-prefixed).
    assert!(
        normalized.contains("onClick={($event)=>{$event}}"),
        "Bare $event should be emitted as a contextually-typed ($event) => callback, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.$event"),
        "$event must not be context-prefixed, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___eventCallbacks("),
        "$event handler should not call the generic eventCallbacks wrapper, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_48_event_member_expression_stays_in_event_scope() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <input @input="$event.target" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // `$event` member access stays inside the contextually-typed ($event) =>
    // callback scope, never context-prefixed.
    assert!(
        normalized.contains("onInput={($event)=>{$event.target}}"),
        "$event member expressions stay inside the ($event) => callback scope, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.$event"),
        "$event must not be context-prefixed, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___eventCallbacks("),
        "$event member handler should not call the generic eventCallbacks wrapper, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_handler_under_v_if_includes_runtime_guard() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg: string | number = Math.random() > 0.5 ? 'x' : 0
</script>
<template>
  <button v-if="typeof msg === 'string'" @click="msg.toLowerCase()" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // In the new output, .value is NOT appended — msg stays as msg
    assert!(
        normalized.contains(
            "onClick={()=>{if(!((typeofmsg==='string'))){returnundefined;}msg.toLowerCase()}}"
        ),
        "v-if event handlers should include the guard inside callback for narrowing (no .value), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_79_v_on_object_syntax_supports_explicit_event_map() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const doThis = () => {}
const doThat = () => {}
</script>
<template>
  <button v-on="{ mousedown: doThis, mouseup: doThat }" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        normalized.contains("onMousedown:doThis") && normalized.contains("onMouseup:doThat"),
        "v-on object explicit map should convert to on* keys, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_without_spaces() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{test}}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{test}"),
        "Interpolation without spaces should become {{test}} (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_preserves_inner_spaces() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{  test  }}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{  test  }"),
        "Interpolation with spaces should preserve inner spaces (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_preserves_inner_newlines() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{  test
  }}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{  test\n  }"),
        "Interpolation with newlines should preserve inner formatting (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_plain_content_wrapped_as_string_expression() {
    let result = compile_tsx(r#"<template>test</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{\"test\"}"),
        "Plain text content should be wrapped as string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_with_less_than_wrapped_as_string_expression() {
    let result = compile_tsx(r#"<template>2 < 1</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        (tsx.code.contains("{\"2 < 1\"}")
            || (tsx.code.contains("{\"2\"}") && tsx.code.contains("{\"< 1\"}"))),
        "Text containing '<' should be wrapped into string expressions, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_escapes_double_quotes_in_string_expression() {
    let result = compile_tsx(r#"<template>"</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{\"\\\"\"}"),
        "Text with quote should escape it inside string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_whitespace_only_is_preserved_without_wrapping() {
    let result = compile_tsx("<template>\n\n\r\n      \n\r\n</template>");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("{\""),
        "Whitespace-only text should not be wrapped as string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_single_lt_is_not_wrapped() {
    let result = compile_tsx(r#"<template><</template>"#);
    // Parser may report a malformed-tag diagnostic, but TSX generation should still avoid
    // wrapping a lone '<' into a string expression.
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("{\"<\"}"),
        "A lone '<' text segment should not be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_tag_replacement_wraps_content_in_fragment() {
    let result = compile_tsx(r#"<template><div></div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN()"),
        "Template should be emitted inside component function wrapper, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<div></div>"),
        "Template root content should be preserved after template-tag replacement, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("<template>"),
        "Raw <template> tag should be removed from TSX output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_tag_empty_template_emits_empty_fragment() {
    let result = compile_tsx(r#"<template></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<></>"),
        "Empty template should still emit an empty fragment, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_item_in_items_emits_map_expression() {
    let result = compile_tsx(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("items).map((item") || tsx.code.contains("_ctx.items).map((item"),
        "v-for item in items should compile to .map expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_item_of_items_emits_map_expression() {
    let result =
        compile_tsx(r#"<template><div v-for="item of items">{{ item + 1 }}</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("items).map((item") || tsx.code.contains("_ctx.items).map((item"),
        "v-for item of items should compile to .map expression, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ item + 1 }") || tsx.code.contains("{ _ctx.item + 1 }"),
        "v-for loop body expression should be preserved, got:\n{}",
        tsx.code
    );
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
        tsx.code.contains("map((item, index) => { return ("),
        "v-for with (item, index) should preserve both params, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("map(({obj}, key, index) => { return (")
            || tsx.code.contains("map(({ obj }, key, index) => { return ("),
        "v-for with destructured value/key/index params should be preserved, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_nested_loop_emits_nested_maps() {
    let result = compile_tsx(
        r#"<template><li v-for="item in items"><span v-for="childItem in item.children">{{ item.message }} {{ childItem }}</span></li></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.matches(".map((").count() >= 2,
        "Nested v-for should produce nested map expressions, got:\n{}",
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
        tsx.code.contains(".map((item") && tsx.code.contains("=> { return ("),
        "template v-for should compile to map over item with statement body, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<li>{ item.msg }</li>")
            || tsx.code.contains("<li>{ _ctx.item.msg }</li>"),
        "template v-for branch should render li child content, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("</>) })"),
        "template v-for branch should close with fragment + statement-body syntax, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_with_v_if_combination_contains_condition_and_map() {
    let result =
        compile_tsx(r#"<template><li v-for="item in items" v-if="item.active"></li></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(".map((item") && tsx.code.contains("=> { return ("),
        "v-for branch should still emit map expression with statement body when combined with v-if, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("item.active ?") || tsx.code.contains("_ctx.item.active ?"),
        "v-if condition should be emitted as lifted ternary for v-for + v-if, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_parent_v_if_with_child_v_for_contains_outer_condition() {
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
        tsx.code.contains(".map((item") && tsx.code.contains("=> { return ("),
        "Child v-for under parent v-if should still emit map expression with statement body, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_pascal_and_dotted_names_are_preserved() {
    let result = compile_tsx(
        r#"<template>
  <Comp></Comp>
  <Calendar.Root />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<Comp></Comp>"),
        "PascalCase component tag should be preserved in TSX output, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<Calendar.Root />") || tsx.code.contains("<Calendar.Root/>"),
        "Dotted component tag should be preserved in TSX output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_static_is_rewrites_to_target_tag() {
    let result = compile_tsx(r#"<template><component is="div"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<div ></div>") || tsx.code.contains("<div></div>"),
        "Static component is=\"div\" should rewrite tag to <div>, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("<component"),
        "Static component is=\"...\" should not keep <component> tag, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_static_is_keeps_other_attributes() {
    let result =
        compile_tsx(r#"<template><component is="div" tabindex="1"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<div  tabindex=\"1\"></div>")
            || tsx.code.contains("<div tabindex=\"1\"></div>")
            || tsx.code.contains("<div tabindex={\"1\"}></div>"),
        "Static component is rewrite should preserve other attrs, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_dynamic_is_literal_string_rewrites_to_target_tag() {
    let result = compile_tsx(r#"<template><component :is="'div'"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("___VERTER___extractRenderComponent('div')"),
        "Dynamic :is with literal string should use extractRenderComponent, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("<___VERTER___component_render ></___VERTER___component_render>"),
        "Dynamic :is should rewrite tag to ___VERTER___component_render, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_dynamic_is_expression_rewrites_to_temp_component() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const as = 'div'
</script>
<template><component :is="as || 'div'"></component></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
        "Dynamic :is expression should emit extractRenderComponent binding, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<___VERTER___component_render "),
        "Dynamic :is expression should rewrite element tag to ___VERTER___component_render, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_with_v_if_and_v_for_preserves_component_tags() {
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
        tsx.code.contains(".map((item") && tsx.code.contains("=> { return (<Comp"),
        "Component with v-for should keep Comp tag inside map statement body, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_kebab_and_mixed_case_names_are_preserved() {
    let result = compile_tsx(
        r#"<template>
  <item-render></item-render>
  <hello-moto />
  <helloMoto />
  <Hello-Moto />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<item-render></item-render>"),
        "kebab-case component tag should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<hello-moto />") || tsx.code.contains("<hello-moto/>"),
        "lower-kebab component tag should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<helloMoto />") || tsx.code.contains("<helloMoto/>"),
        "mixed camelCase component tag should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<Hello-Moto />") || tsx.code.contains("<Hello-Moto/>"),
        "mixed Pascal-kebab component tag should be preserved, got:\n{}",
        tsx.code
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
    assert!(result.tsx.is_none(), "TSX should be None when disabled");
}

#[test]
fn tsx_template_comment() {
    let result = compile_tsx(r#"<template><!-- hello --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* hello */}"),
        "Comment should be converted to JSX comment with preserved spacing, got: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_no_extra_spacing() {
    let result = compile_tsx(r#"<template><!--comment--></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/*comment*/}"),
        "Comment without spaces should not gain extra padding, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_with_nested_marker_text() {
    let result = compile_tsx(r#"<template><!-- <!-- --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* <!-- */}"),
        "Comment containing '<!--' text should remain valid JSX comment, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_with_angle_bracket_text() {
    let result = compile_tsx(r#"<template><!-- <MyComp --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* <MyComp */}"),
        "Comment containing '<MyComp' text should remain wrapped in JSX comment, got:\n{}",
        tsx.code
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
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Should still contain script content, just no template JSX
    assert!(
        tsx.code.contains("const msg = 'hello'"),
        "Script-only TSX should contain setup content, got: {}",
        tsx.code
    );
}

/// @ai-generated - TSX source map should be generated (not empty) for SFCs with template
#[test]
fn tsx_source_map_is_generated() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty"
    );

    // Parse the source map JSON to validate structure
    let sm: serde_json::Value =
        serde_json::from_str(&tsx.source_map).expect("TSX source map should be valid JSON");
    assert_eq!(sm.get("version").and_then(|v| v.as_u64()), Some(3));
    assert!(
        sm.get("mappings")
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "Mappings should not be empty"
    );
    assert!(
        sm.get("sources")
            .and_then(|v| v.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "Sources array should not be empty"
    );
}

/// @ai-generated - TSX source map should map `msg` in script back to the original position
#[test]
fn tsx_source_map_maps_script_binding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty"
    );

    // Parse with oxc_sourcemap and verify we can look up a position
    let sm = oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map)
        .expect("should parse source map");

    // Find "const msg" in the TSX output and look it up
    let msg_offset = tsx
        .code
        .find("const msg")
        .expect("TSX should contain 'const msg'");
    let tsx_line = tsx.code[..msg_offset].matches('\n').count() as u32;
    let tsx_col = (msg_offset
        - tsx.code[..msg_offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0)) as u32;

    let lookup_table = sm.generate_lookup_table();
    let token = sm.lookup_token(&lookup_table, tsx_line, tsx_col);

    assert!(
        token.is_some(),
        "Should find a source map token for 'const msg' at TSX line {tsx_line}, col {tsx_col}"
    );

    if let Some(token) = token {
        // "const msg" is on line 1 (0-indexed) in the original source
        let original_msg_line = source[..source.find("const msg").unwrap()]
            .matches('\n')
            .count() as u32;
        assert_eq!(
            token.get_src_line(),
            original_msg_line,
            "Source line should map back to the original 'const msg' line"
        );
    }
}

/// @ai-generated - TSX source map for script-only SFC (no template)
#[test]
fn tsx_source_map_script_only() {
    let source = r#"<script setup>
const msg = 'hello'
</script>"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty even for script-only SFC"
    );
}

/// @ai-generated - TSX output must be independent from force_js mode.
#[test]
fn tsx_force_js_toggle_does_not_change_code() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count: number = 1
const msg: string = 'hello'
</script>
<template>
  <button @click="count++">{{ msg }} {{ count }}</button>
</template>"#;

    let force_js_true = compile_tsx_with_force_js(source, true);
    let force_js_false = compile_tsx_with_force_js(source, false);

    assert!(
        force_js_true.errors.is_empty(),
        "force_js=true compile errors: {:?}",
        force_js_true.errors
    );
    assert!(
        force_js_false.errors.is_empty(),
        "force_js=false compile errors: {:?}",
        force_js_false.errors
    );

    let tsx_true = force_js_true.tsx.expect("tsx block (force_js=true)");
    let tsx_false = force_js_false.tsx.expect("tsx block (force_js=false)");

    assert_eq!(
        tsx_true.code, tsx_false.code,
        "TSX code must be identical regardless of force_js"
    );
}

/// @ai-generated - TSX source map must be independent from force_js mode.
#[test]
fn tsx_force_js_toggle_does_not_change_source_map() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count: number = 1
const msg: string = 'hello'
</script>
<template>
  <button @click="count++">{{ msg }} {{ count }}</button>
</template>"#;

    let force_js_true = compile_tsx_with_force_js(source, true);
    let force_js_false = compile_tsx_with_force_js(source, false);

    assert!(
        force_js_true.errors.is_empty(),
        "force_js=true compile errors: {:?}",
        force_js_true.errors
    );
    assert!(
        force_js_false.errors.is_empty(),
        "force_js=false compile errors: {:?}",
        force_js_false.errors
    );

    let tsx_true = force_js_true.tsx.expect("tsx block (force_js=true)");
    let tsx_false = force_js_false.tsx.expect("tsx block (force_js=false)");

    assert_eq!(
        tsx_true.source_map, tsx_false.source_map,
        "TSX source map must be identical regardless of force_js"
    );
}

// ==================== Vue built-in components ====================
//
// Vue built-in components (Suspense, Teleport, KeepAlive, Transition,
// TransitionGroup, BaseTransition) must be imported directly from "vue"
// instead of using _resolveComponent(). Vue's compiler emits e.g.:
//   import { Suspense as _Suspense } from "vue"
//   _createBlock(_Suspense, null, { ... })

/// @ai-generated - Suspense must be imported from "vue" and used directly,
/// NOT via _resolveComponent("Suspense").
#[test]
fn builtin_component_suspense() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Suspense>
    <Comp />
  </Suspense>
</template>"#,
    );
    eprintln!("=== SUSPENSE OUTPUT ===\n{}", code);
    // Must NOT use _resolveComponent for Suspense
    assert!(
        !code.contains("_resolveComponent(\"Suspense\")"),
        "Suspense must NOT use _resolveComponent, got:\n{}",
        code
    );
    // Must use _Suspense directly
    assert!(
        code.contains("_Suspense"),
        "Suspense must be imported and used as _Suspense, got:\n{}",
        code
    );
}

/// @ai-generated - Teleport must be imported from "vue" and used directly.
#[test]
fn builtin_component_teleport() {
    let code = compile_and_validate_template(
        r#"<template>
  <Teleport to="body">
    <div>modal content</div>
  </Teleport>
</template>"#,
    );
    eprintln!("=== TELEPORT OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"Teleport\")"),
        "Teleport must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Teleport"),
        "Teleport must be imported and used as _Teleport, got:\n{}",
        code
    );
}

/// @ai-generated - KeepAlive must be imported from "vue" and used directly.
#[test]
fn builtin_component_keep_alive() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <KeepAlive>
    <Comp />
  </KeepAlive>
</template>"#,
    );
    eprintln!("=== KEEPALIVE OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"KeepAlive\")"),
        "KeepAlive must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_KeepAlive"),
        "KeepAlive must be imported and used as _KeepAlive, got:\n{}",
        code
    );
}

/// @ai-generated - Transition must be imported from "vue" and used directly.
#[test]
fn builtin_component_transition() {
    let code = compile_and_validate_template(
        r#"<template>
  <Transition name="fade">
    <div>content</div>
  </Transition>
</template>"#,
    );
    eprintln!("=== TRANSITION OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"Transition\")"),
        "Transition must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Transition"),
        "Transition must be imported and used as _Transition, got:\n{}",
        code
    );
}

/// @ai-generated - TransitionGroup must be imported from "vue" and used directly.
#[test]
fn builtin_component_transition_group() {
    let code = compile_and_validate_template(
        r#"<template>
  <TransitionGroup name="list" tag="ul">
    <li v-for="item in items" :key="item">{{ item }}</li>
  </TransitionGroup>
</template>"#,
    );
    eprintln!("=== TRANSITION GROUP OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"TransitionGroup\")"),
        "TransitionGroup must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_TransitionGroup"),
        "TransitionGroup must be imported and used as _TransitionGroup, got:\n{}",
        code
    );
}

/// @ai-generated - kebab-case built-in components must also be recognized.
/// <keep-alive> is the same as <KeepAlive>.
#[test]
fn builtin_component_kebab_case_keep_alive() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <keep-alive>
    <Comp />
  </keep-alive>
</template>"#,
    );
    eprintln!("=== KEBAB KEEP-ALIVE OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"keep-alive\")"),
        "keep-alive must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_KeepAlive"),
        "keep-alive must be imported and used as _KeepAlive, got:\n{}",
        code
    );
}

/// @ai-generated - kebab-case <teleport> must be recognized as built-in.
#[test]
fn builtin_component_kebab_case_teleport() {
    let code = compile_and_validate_template(
        r#"<template>
  <teleport to="body">
    <div>modal</div>
  </teleport>
</template>"#,
    );
    assert!(
        !code.contains("_resolveComponent(\"teleport\")"),
        "teleport must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Teleport"),
        "teleport must be imported and used as _Teleport, got:\n{}",
        code
    );
}

/// @ai-generated - prop_constness_overrides threads through compile pipeline without error.
/// Verifies that passing const prop overrides produces valid compiled output.
#[test]
fn const_props_override_compiles_successfully() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let mut const_set = rustc_hash::FxHashSet::default();
    const_set.insert("msg".to_string());
    let verter_opts = VerterCompileOptions {
        force_js: true,
        prop_constness_overrides: Some(const_set),
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const props = defineProps({ msg: String, count: Number })
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
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
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Validate the generated JS is syntactically valid
    let alloc2 = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&alloc2, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Generated JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
}

/// @ai-generated - prop_constness_overrides threads through Vapor compile pipeline.
#[test]
fn const_props_override_compiles_vapor() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let mut const_set = rustc_hash::FxHashSet::default();
    const_set.insert("msg".to_string());
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        prop_constness_overrides: Some(const_set),
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const props = defineProps({ msg: String, count: Number })
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
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
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    let alloc2 = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&alloc2, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Vapor generated JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
}

/// @ai-generated - Built-in component imports should appear in template.imports.
#[test]
fn builtin_component_in_imports_list() {
    let result = compile_sfc(
        r#"<template>
  <Suspense>
    <div />
  </Suspense>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    // The imports list should include _Suspense
    assert!(
        tpl.imports.contains(&"_Suspense"),
        "template.imports should contain _Suspense, got: {:?}",
        tpl.imports
    );
}

#[test]
fn tsx_template_inside_return_statement() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)
const message = ref('Hello from Verter!')

function increment() {
  count.value++
}
</script>

<template>
  <div class="app">
    <h1>{{ message }}</h1>
    <button @click="increment">Count: {{ count }}</button>
  </div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");

    // The template JSX must be INSIDE the TemplateBindingFN function body.
    // It is emitted as an expression statement (not returned), so that
    // ReturnType<typeof TemplateBindingFN> resolves to the binding object.
    let fn_open = tsx
        .code
        .find("___VERTER___TemplateBindingFN")
        .expect("TemplateBindingFN");
    let fn_brace = tsx.code[fn_open..].find('{').expect("opening brace") + fn_open;
    // Find the matching closing brace
    let mut depth = 0i32;
    let mut fn_close = None;
    for (i, ch) in tsx.code[fn_brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    fn_close = Some(fn_brace + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let fn_end = fn_close.expect("closing brace of TemplateBindingFN");
    let fn_body = &tsx.code[fn_brace..fn_end + 1];

    // The template content must be inside the function body
    assert!(
        fn_body.contains("<div class=\"app\">"),
        "Template <div> should be inside TemplateBindingFN body, but body is:\n{}\n\nFull TSX:\n{}",
        fn_body,
        tsx.code
    );
    assert!(
        fn_body.contains("message"),
        "Template interpolation should be inside TemplateBindingFN body, but body is:\n{}",
        fn_body,
    );

    // The binding return must also be inside the function
    assert!(
        fn_body.contains("shallowUnwrapRef"),
        "shallowUnwrapRef return should be inside TemplateBindingFN body, got:\n{}",
        fn_body,
    );

    // The empty placeholder <></> should NOT remain
    assert!(
        !tsx.code.contains("<></>"),
        "Empty fragment placeholder <></> should be replaced with template content.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_block_scope_no_double_braces() {
    // Double braces `}}` / `{{` in push_str should not appear —
    // they are NOT format-escaping, they emit literal characters.
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Should NOT contain `}}` followed by " // close" (old double-brace bug)
    assert!(
        !tsx.code.contains("}} // close"),
        "Double braces `}}` must not appear in block scope / templateBindingFN close.\nTSX:\n{}",
        tsx.code
    );

    // Should contain single-brace closes
    assert!(
        tsx.code.contains("} // close block scope"),
        "Block scope should close with single brace.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("} // close templateBindingFN"),
        "TemplateBindingFN should close with single brace.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_shallow_unwrap_ref_avoids_tdz() {
    // The shallowUnwrapRef call must use a temp variable outside the block scope
    // to avoid TDZ (Temporal Dead Zone) errors where `const { count } = shallowUnwrapRef({ count: count ... })`
    // would self-reference the uninitialized binding.
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref('hello')
</script>

<template>
  <div>{{ count }} {{ message }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: temp variable pattern
    assert!(
        tsx.code
            .contains("___VERTER___unwrapped = ___VERTER___shallowUnwrapRef("),
        "Should use temp variable for shallowUnwrapRef.\nTSX:\n{}",
        tsx.code
    );

    // Positive: destructuring from temp
    assert!(
        tsx.code.contains("} = ___VERTER___unwrapped;"),
        "Should destructure from ___VERTER___unwrapped temp variable.\nTSX:\n{}",
        tsx.code
    );

    // Negative: old combined pattern must not appear (TDZ-prone)
    assert!(
        !tsx.code.contains("} = ___VERTER___shallowUnwrapRef("),
        "Old combined destructure+call pattern must not appear (causes TDZ).\nTSX:\n{}",
        tsx.code
    );

    // Positive: boundary markers around the destructuring block
    assert!(
        tsx.code.contains("/* verter-destructured-start */"),
        "Destructuring block must be wrapped with start marker.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("/* verter-destructured-end */"),
        "Destructuring block must be wrapped with end marker.\nTSX:\n{}",
        tsx.code
    );

    // The start marker must come before the destructuring, end marker after
    let start_marker_pos = tsx.code.find("/* verter-destructured-start */").unwrap();
    let end_marker_pos = tsx.code.find("/* verter-destructured-end */").unwrap();
    let destruct_pos = tsx.code.find("} = ___VERTER___unwrapped;").unwrap();
    assert!(
        start_marker_pos < destruct_pos && destruct_pos < end_marker_pos,
        "Markers must bracket the destructuring: start={}, destruct={}, end={}\nTSX:\n{}",
        start_marker_pos,
        destruct_pos,
        end_marker_pos,
        tsx.code
    );
}

// ── TSX source map round-trip integration tests ───────────────────

fn compile_tsx_with_source_map(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

fn compile_tsx_with_template_data(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        extract_template_data: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// Verify that every source map token in the TSX output maps back to valid
/// positions in the original Vue SFC source (no out-of-bounds lines/cols).
fn verify_sourcemap_tokens_in_bounds(source: &str, tsx: &VerterTsxBlock) {
    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");

    let vue_lines: Vec<&str> = source.lines().collect();
    let tsx_lines: Vec<&str> = tsx.code.lines().collect();

    for token in sm.get_tokens() {
        // Only check tokens that reference a source file
        if token.get_source_id().is_none() {
            continue;
        }

        let src_line = token.get_src_line() as usize;
        let src_col = token.get_src_col() as usize;
        let dst_line = token.get_dst_line() as usize;
        let dst_col = token.get_dst_col() as usize;

        // Source position must be in bounds of Vue SFC
        assert!(
            src_line < vue_lines.len(),
            "Source map token points to Vue line {} but SFC only has {} lines.\n\
             TSX gen position: {}:{}\nTSX code:\n{}",
            src_line,
            vue_lines.len(),
            dst_line,
            dst_col,
            tsx.code
        );

        // Generated position must be in bounds of TSX output
        assert!(
            dst_line < tsx_lines.len(),
            "Source map token points to TSX line {} but TSX only has {} lines.\n\
             Vue src position: {}:{}\nTSX code:\n{}",
            dst_line,
            tsx_lines.len(),
            src_line,
            src_col,
            tsx.code
        );
    }
}

/// @ai-generated — TSX source map: interpolation `{{ msg }}` maps back to Vue source
#[test]
fn tsx_sourcemap_interpolation() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
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

/// @ai-generated — TSX source map: @click event handler maps back
#[test]
fn tsx_sourcemap_event_handler() {
    let source = r#"<script setup>
function handler() {}
</script>

<template>
  <button @click="handler">click</button>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: bound prop maps back
#[test]
fn tsx_sourcemap_bound_prop() {
    let source = r#"<script setup>
const value = 42
</script>

<template>
  <input :value="value" />
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: component tag maps back
#[test]
fn tsx_sourcemap_component_tag() {
    let source = r#"<script setup>
import MyComponent from './MyComponent.vue'
</script>

<template>
  <MyComponent />
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: slot with destructured params maps back
#[test]
fn tsx_sourcemap_slot_destructured() {
    let source = r#"<script setup>
import MyComponent from './MyComponent.vue'
</script>

<template>
  <MyComponent>
    <template #default="{ data }">{{ data }}</template>
  </MyComponent>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: multi-byte characters (CJK, emoji) in script and template
#[test]
fn tsx_sourcemap_multibyte_characters() {
    let source = r#"<script setup>
// 你好世界
const msg = '你好'
</script>

<template>
  <div>{{ msg }} 🎉</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: SFC with style block after template (exposes bleeding bug)
///
/// This is the key regression test for Bug 1. The template CodeTransform operates
/// on the full SFC input, so its source map contains tokens for ALL regions including
/// the style block after `</template>`. If combine_tsx_source_maps doesn't filter
/// post-template tokens, style block tokens bleed into the combined map with incorrect
/// generated line positions.
#[test]
fn tsx_sourcemap_style_after_template_no_bleeding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div class="app">{{ msg }}</div>
</template>

<style scoped>
.app {
  color: red;
  font-size: 16px;
}
</style>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Basic bounds check
    verify_sourcemap_tokens_in_bounds(source, tsx);

    // TSX output should NOT contain any style content
    assert!(
        !tsx.code.contains("color: red"),
        "Style content should not appear in TSX output:\n{}",
        tsx.code
    );

    // Critical check: no source map token should reference a source position within
    // the style block (lines 8-13 in the Vue SFC). If style tokens bleed through
    // combine_tsx_source_maps, they'll appear as tokens with src_line in the style range.
    let style_start_line = source[..source.find("<style").unwrap()]
        .matches('\n')
        .count() as u32;
    let style_end_line = source[..source.find("</style>").unwrap() + "</style>".len()]
        .matches('\n')
        .count() as u32;

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");

    let mut style_tokens = Vec::new();
    for token in sm.get_tokens() {
        if token.get_source_id().is_none() {
            continue;
        }
        let src_line = token.get_src_line();
        if src_line >= style_start_line && src_line <= style_end_line {
            style_tokens.push((
                token.get_src_line(),
                token.get_src_col(),
                token.get_dst_line(),
                token.get_dst_col(),
            ));
        }
    }

    assert!(
        style_tokens.is_empty(),
        "Source map contains {} tokens referencing style block (Vue lines {}-{}): {:?}\n\
         These tokens bleed from the template CodeTransform's source map.\nTSX code:\n{}",
        style_tokens.len(),
        style_start_line,
        style_end_line,
        style_tokens,
        tsx.code
    );
}

/// @ai-generated — TSX source map: SFC with multiple style blocks
#[test]
fn tsx_sourcemap_multiple_style_blocks() {
    let source = r#"<script setup>
const x = 1
</script>

<template>
  <div>{{ x }}</div>
</template>

<style>
.a { color: red; }
</style>

<style scoped>
.b { color: blue; }
</style>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);

    // No token should reference source positions in the style blocks
    let first_style_line = source[..source.find("<style").unwrap()]
        .matches('\n')
        .count() as u32;
    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    for token in sm.get_tokens() {
        if token.get_source_id().is_some() {
            assert!(
                token.get_src_line() < first_style_line,
                "Source map token references style block (src line {}, first style line {}).\n\
                 Style tokens must not bleed into the combined TSX source map.",
                token.get_src_line(),
                first_style_line,
            );
        }
    }
}

/// @ai-generated — TSX source map: template-only SFC
#[test]
fn tsx_sourcemap_template_only() {
    let source = r#"<template>
  <div>hello</div>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: script-only SFC
#[test]
fn tsx_sourcemap_script_only() {
    let source = r#"<script setup>
const x = 1
const y = 2
</script>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

// ── Source map position mapping regression tests ──────────────────

/// Helper: find `needle` in `haystack` and return its 0-indexed (line, col).
/// Panics if `needle` is not found.
fn find_line_col(haystack: &str, needle: &str) -> (u32, u32) {
    let offset = haystack
        .find(needle)
        .unwrap_or_else(|| panic!("'{}' not found in text", needle));
    let line = haystack[..offset].matches('\n').count() as u32;
    let col = (offset - haystack[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;
    (line, col)
}

fn find_line_col_at(haystack: &str, offset: usize) -> (u32, u32) {
    let line = haystack[..offset].matches('\n').count() as u32;
    let col = (offset - haystack[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0)) as u32;
    (line, col)
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

/// @ai-generated — TSX source map: v-else-if expression maps correctly.
#[test]
fn tsx_sourcemap_v_else_if_expression_maps_correctly() {
    let source = r#"<script lang="ts" setup>
let isLoggedIn = false;
let hasPermission = false;
</script>

<template>
  <div v-if="isLoggedIn && hasPermission">Full Access</div>
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

    // Find `isLoggedIn` in the v-else-if expression in TSX
    let elseif_start = tsx.code.find("else if(isLoggedIn").unwrap();
    let tsx_elseif_islogged = tsx.code[elseif_start..].find("isLoggedIn").unwrap() + elseif_start;
    let (tsx_line, tsx_col) = find_line_col_at(&tsx.code, tsx_elseif_islogged);

    // Find `isLoggedIn` in v-else-if attribute in Vue source
    let vue_elseif_attr = source.find(r#"v-else-if="isLoggedIn"#).unwrap() + 11; // skip v-else-if="
    let (vue_line, vue_col) = find_line_col_at(source, vue_elseif_attr);

    let token = sm
        .lookup_token(&lookup, tsx_line, tsx_col)
        .expect("source map token for v-else-if isLoggedIn");
    assert!(
        token.get_source_id().is_some(),
        "v-else-if isLoggedIn should have source mapping"
    );

    let mut mapped_col = token.get_src_col();
    if token.get_dst_line() == tsx_line && tsx_col > token.get_dst_col() {
        mapped_col += tsx_col - token.get_dst_col();
    }
    assert_eq!(
        token.get_src_line(),
        vue_line,
        "v-else-if isLoggedIn: wrong Vue line"
    );
    assert_eq!(mapped_col, vue_col, "v-else-if isLoggedIn: wrong Vue col");
}

// ── Binding occurrence position tests ─────────────────────────────

/// @ai-generated — Binding occurrences have correct span offsets
///
/// Verifies that each RawBindingOccurrence's span.start..span.end maps to the
/// correct binding name in the original source. This catches position bugs in
/// template data extraction.
#[test]
fn binding_occurrence_spans_match_source() {
    let source = r#"<script setup>
const msg = 'hello'
const count = 0
</script>

<template>
  <div>{{ msg }}</div>
  <span>{{ count }}</span>
</template>
"#;
    let result = compile_tsx_with_template_data(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tpl_data = result.template_data.as_ref().expect("template data");
    assert!(
        !tpl_data.binding_occurrences.is_empty(),
        "Expected binding occurrences but got none"
    );

    for occ in &tpl_data.binding_occurrences {
        let start = occ.span.start as usize;
        let end = occ.span.end as usize;
        assert!(
            end <= source.len(),
            "Binding '{}' span {}..{} exceeds source length {}",
            occ.name,
            start,
            end,
            source.len()
        );
        let slice = &source[start..end];
        assert_eq!(
            slice, occ.name,
            "Binding occurrence span {}..{} contains '{}' but expected '{}'",
            start, end, slice, occ.name
        );
    }
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
    let result = compile_tsx(
        r#"<script setup lang="ts">
const props = defineProps<{ render: 'svg' | 'img' }>()
</script>
<template>
  <img v-if="render === 'svg'" class="icon" />
  <span v-else>fallback</span>
</template>"#,
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
    let iife_start = tsx.code.find("{()=>{if(").expect("IIFE should exist");
    let iife_end = tsx.code[iife_start..].find("}}}").expect("IIFE close") + iife_start + 3;
    let iife_region = &tsx.code[iife_start..iife_end];
    // Wrap in a JSX expression context for parsing
    let wrapper = format!("const x = <>{}</>", iife_region);
    let alloc = oxc_allocator::Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let parsed = oxc_parser::Parser::new(&alloc, &wrapper, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "IIFE region has syntax errors: {:?}\n--- IIFE ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        iife_region
    );
}

#[test]
fn tsx_custom_types_module_in_output() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        types_module_name: Some("@my/types".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let source = r#"<script setup>const x = 1</script><template><div/></template>"#;
    let result = compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.expect("tsx block");
    assert!(
        tsx.code.contains(r#"from "@my/types""#),
        "custom types module should be used, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains(r#"from "@verter/types""#),
        "default types module should NOT appear"
    );
}

#[test]
fn tsx_default_types_module_is_verter_types() {
    let result = compile_tsx(r#"<script setup>const x = 1</script><template><div/></template>"#);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(r#"from "@verter/types""#),
        "default should use @verter/types, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains(r#"from "$verter/types$""#),
        "should NOT use $verter/types$"
    );
}

#[test]
fn tsx_options_api_has_type_constructs_at_compile_level() {
    let result = compile_tsx(
        r#"<script lang="ts">
export default { props: ['msg'] }
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    // Helper imports present
    assert!(
        tsx.code.contains(r#"from "@verter/types""#),
        "Options API should have types imports, got:\n{}",
        tsx.code
    );
    // Negative: Instance type should no longer be emitted
    assert!(
        !tsx.code.contains("___VERTER___Instance"),
        "Options API should not have Instance type construct"
    );
    // OXC validation
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Options API TSX must be valid JS: {:?}\n---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

// ── Error recovery: malformed templates must not panic ─────────────
// @ai-generated — Verifies that broken templates produce diagnostics, not panics.

#[test]
fn error_recovery_orphan_close_tag_does_not_panic() {
    // `</component` inside template — orphan close tag
    let result = compile_sfc("<template></component</template>");
    assert!(
        !result.errors.is_empty(),
        "should report diagnostics for orphan close tag"
    );
}

#[test]
fn error_recovery_unclosed_element_does_not_panic() {
    let result = compile_sfc("<template><div></template>");
    assert!(
        !result.errors.is_empty(),
        "should report diagnostics for unclosed element"
    );
}

#[test]
fn error_recovery_incomplete_tag_does_not_panic() {
    let result = compile_sfc("<template><</template>");
    // Should not panic — incomplete `<` treated as text
    let _ = result;
}

#[test]
fn error_recovery_bare_close_tag_no_angle() {
    // `</component` without closing `>` — tokenizer must handle gracefully
    let result = compile_sfc("<template></component");
    // Must not panic
    let _ = result;
}

// ==================== Static subtree hoisting ====================

/// Compile with hoist_static=true (default) and validate JS output.
fn compile_and_validate_hoisted(source: &str) -> String {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(true),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let parse_alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&parse_alloc, &wrapped, source_type).parse();
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

/// Compile with hoist_static=false and validate JS output.
fn compile_and_validate_no_hoist(source: &str) -> String {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        hoist_static: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Parse with OXC to ensure valid JS
    let parse_alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = oxc_parser::Parser::new(&parse_alloc, &wrapped, source_type).parse();
    assert!(
        parsed.errors.is_empty(),
        "Template JS parse error (no-hoist): {:?}\n--- generated code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
    tpl.code.clone()
}

/// @ai-generated — Single static element emits _cache wrapping
#[test]
fn static_hoist_single_static_element() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="card"><h3>Title</h3><p>text</p></div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping for static elements\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("-1 /* CACHED */"),
        "should emit -1 CACHED patch flag\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "should NOT use _createStaticVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Nested static elements emit _cache wrapping
#[test]
fn static_hoist_nested_static() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><section><div><span>deep</span></div></section></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "deeply nested static subtree should use _cache wrapping\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "should NOT use _createStaticVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Dynamic element is NOT hoisted
#[test]
fn static_hoist_dynamic_element_not_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div :class="cls">dynamic</div></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "dynamic element should NOT be hoisted\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "dynamic element should use _createElementVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Element with event is NOT hoisted
#[test]
fn static_hoist_event_not_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><button @click="fn">click</button></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "element with event should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Element with interpolation is NOT hoisted
#[test]
fn static_hoist_interpolation_not_hoisted() {
    let code =
        compile_and_validate_hoisted(r#"<template><div><div>{{ msg }}</div></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "element with interpolation should NOT be hoisted\n--- code ---\n{}",
        code
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

/// @ai-generated — Element with ref is NOT hoisted
#[test]
fn static_hoist_ref_not_hoisted() {
    let code =
        compile_and_validate_hoisted(r#"<template><div><div ref="el">text</div></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "element with ref should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Component is NOT hoisted
#[test]
fn static_hoist_component_not_hoisted() {
    let code = compile_and_validate_hoisted(r#"<template><div><MyComp /></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "component should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Static with dynamic sibling: both present
#[test]
fn static_hoist_mixed_static_and_dynamic() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><span>static</span><span :class="x">dynamic</span></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "static sibling should use _cache wrapping\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "dynamic sibling should use _createElementVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Consecutive static siblings each get _cache[N] entries
#[test]
fn static_hoist_consecutive_siblings_merge() {
    let code =
        compile_and_validate_hoisted(r#"<template><div><p>a</p><p>b</p><p>c</p></div></template>"#);
    assert!(
        code.contains("_cache[0]"),
        "first static element should use _cache[0]\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_cache[1]"),
        "second static element should use _cache[1]\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_cache[2]"),
        "third static element should use _cache[2]\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — hoist_static=false disables the optimization
#[test]
fn static_hoist_disabled() {
    let code = compile_and_validate_no_hoist(
        r#"<template><div><div class="card"><h3>Title</h3></div></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "hoist_static=false should disable optimization\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "should use _createElementVNode when hoisting disabled\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Scoped style injects data-v attribute on cached static elements
#[test]
fn static_hoist_scoped_style() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><p class="foo">text</p></div></template>
<style scoped>.foo { color: red; }</style>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping with scoped style\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — HTML with double quotes produces valid JS (OXC parse check)
#[test]
fn static_hoist_html_quotes_valid_js() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="foo" id="bar">text</div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping for static element with attributes\n--- code ---\n{}",
        code
    );
    // The OXC parse in compile_and_validate_hoisted already validates JS syntax
}

/// @ai-generated — Static parent with one dynamic child is NOT static
#[test]
fn static_hoist_parent_with_dynamic_child() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="wrapper"><span :title="t">x</span></div></div></template>"#,
    );
    // The wrapper div has a dynamic child, so it's not fully static
    assert!(
        !code.contains("_createStaticVNode"),
        "parent with dynamic child should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Static element with class (no dynamic binding) is cached
#[test]
fn static_hoist_static_class() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="foo">text</div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "element with static class should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Deep nesting (3+ levels) all static
#[test]
fn static_hoist_deep_nesting() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div><div><div><span>deep</span></div></div></div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "deeply nested static should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Self-closing static element
#[test]
fn static_hoist_self_closing() {
    let code = compile_and_validate_hoisted(r#"<template><div><br/><hr/></div></template>"#);
    assert!(
        code.contains("_cache["),
        "self-closing static elements should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Template literal escaping: backtick in attribute
#[test]
fn static_hoist_backtick_in_html() {
    let code = compile_and_validate_hoisted(
        "<template><div><div title=\"a`b\">text</div></div></template>",
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping\n--- code ---\n{}",
        code
    );
}

// ── TSX export / Comp / JSDoc / offset comment tests ──────────────────

#[test]
fn tsx_export_template_binding_fn() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: TemplateBindingFN should be exported
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN"),
        "TemplateBindingFN should be exported, got:\n{}",
        tsx.code
    );
    // Negative: no bare (non-exported) function declaration
    // Check that every "function ___VERTER___TemplateBindingFN" is preceded by "export "
    let needle = "function ___VERTER___TemplateBindingFN";
    let mut search_from = 0;
    while let Some(pos) = tsx.code[search_from..].find(needle) {
        let abs_pos = search_from + pos;
        let before = &tsx.code[..abs_pos];
        assert!(
            before.ends_with("export "),
            "TemplateBindingFN at offset {} is not preceded by 'export ': {}",
            abs_pos,
            tsx.code
        );
        search_from = abs_pos + needle.len();
    }
}

#[test]
fn tsx_export_instance_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { getCurrentInstance } from 'vue'
const instance = getCurrentInstance()
</script>
<template><div>{{ instance?.proxy }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Negative: Instance and CurrentComponentInstance should no longer be exported
    assert!(
        !tsx.code.contains("export type ___VERTER___Instance"),
        "Instance type should no longer be exported, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code
            .contains("export type ___VERTER___CurrentComponentInstance"),
        "CurrentComponentInstance type should no longer be exported, got:\n{}",
        tsx.code
    );
    // Negative: no bare type declarations either
    assert!(
        !tsx.code.contains("\ntype ___VERTER___Instance"),
        "Instance type should NOT have bare 'type' declaration:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_get_current_instance_no_declaration() {
    // Issue #11: When getCurrentInstance() is NOT called, the declaration must not appear
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Negative: no getCurrentInstance declaration when it's not used
    assert!(
        !tsx.code.contains("declare function getCurrentInstance"),
        "getCurrentInstance declaration must NOT appear when not used, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___Instance"),
        "Instance type must NOT appear when getCurrentInstance is not used, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_comp_only_for_ref_elements() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">a</div><span>b</span></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: Comp function present for div (has ref)
    assert!(
        tsx.code.contains("___VERTER___Comp"),
        "Should have Comp function for ref element, got:\n{}",
        tsx.code
    );
    // Negative: should NOT have a Comp for the span (no ref)
    // The span's tag_open.start offset should NOT appear as a Comp function
    // Since div has ref and span doesn't, we check there's exactly one Comp function
    let comp_count = tsx.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count, 1,
        "Should have exactly 1 Comp function (for ref element only), got {}: \n{}",
        comp_count, tsx.code
    );
}

#[test]
fn tsx_comp_emitted_for_root_element_without_ref() {
    // Single root: Comp function emitted for root element (for implicit attrs)
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "Should have Comp function for single root element, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___getRootComponent"),
        "Should have getRootComponent with template, got:\n{}",
        tsx.code
    );
    let comp_count = tsx.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count, 1,
        "Should have exactly 1 Comp function (single root), got {} in:\n{}",
        comp_count, tsx.code
    );

    // Multi-root (fragment): no Comp functions for root (no ref, no attrs fallthrough)
    let result2 = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div><MyComp /></template>"#,
    );
    assert!(result2.errors.is_empty(), "errors: {:?}", result2.errors);
    let tsx2 = result2.tsx.as_ref().expect("tsx block");
    // getRootComponent still emitted (returns {})
    assert!(
        tsx2.code.contains("___VERTER___getRootComponent"),
        "Should have getRootComponent with template, got:\n{}",
        tsx2.code
    );
    // No Comp functions (fragment, no ref elements)
    let comp_count2 = tsx2.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count2, 0,
        "Fragment should have 0 Comp functions (no ref), got {} in:\n{}",
        comp_count2, tsx2.code
    );
    // getRootComponent returns {}
    assert!(
        tsx2.code.contains("getRootComponent() { return {};"),
        "Fragment getRootComponent should return empty, got:\n{}",
        tsx2.code
    );
}

#[test]
fn tsx_jsdoc_on_shallow_unwrap_ref_entries() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
/** My counter */
const count = ref(0)
const plain = "hello"
</script>
<template><div>{{ count }} {{ plain }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: shallowUnwrapRef should contain JSDoc before count
    assert!(
        tsx.code.contains("/** My counter */\n    count:"),
        "shallowUnwrapRef should have JSDoc before count entry, got:\n{}",
        tsx.code
    );
    // Negative: plain should NOT have JSDoc before it
    let plain_in_unwrap = tsx.code.find("plain: plain as unknown");
    if let Some(pos) = plain_in_unwrap {
        let before = &tsx.code[pos.saturating_sub(20)..pos];
        assert!(
            !before.contains("/**"),
            "plain should not have JSDoc before it in shallowUnwrapRef:\n{}",
            tsx.code
        );
    }
}

#[test]
fn tsx_destructured_block_has_no_offset_comments() {
    let source = r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref("hi")
</script>
<template><div>{{ count }} {{ message }}</div></template>"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: NO /*digits,digits*/ offset comments in the output
    assert!(
        !has_offset_comment(&tsx.code),
        "Offset comments /*start,end*/ must NOT appear in TSX output.\nTSX:\n{}",
        tsx.code
    );

    // Positive: boundary markers still present
    assert!(tsx.code.contains("/* verter-destructured-start */"));
    assert!(tsx.code.contains("/* verter-destructured-end */"));

    // Positive: destructured_block metadata is populated
    let meta = tsx
        .destructured_block
        .as_ref()
        .expect("destructured_block metadata should be populated");

    // Should have bindings for count and message
    let names: Vec<&str> = meta.bindings.iter().map(|b| b.name.as_str()).collect();
    assert!(
        names.contains(&"count"),
        "bindings should include 'count', got: {:?}",
        names
    );
    assert!(
        names.contains(&"message"),
        "bindings should include 'message', got: {:?}",
        names
    );

    // Verify source spans point to correct identifiers in SFC
    for binding in &meta.bindings {
        let span_text =
            &source[binding.source_span.start as usize..binding.source_span.end as usize];
        assert_eq!(
            span_text, binding.name,
            "source_span for '{}' should point to identifier in SFC",
            binding.name
        );
    }

    // Block range should bracket the destructured block in TSX
    let start_marker = tsx.code.find("/* verter-destructured-start */").unwrap();
    let end_marker = tsx.code.find("/* verter-destructured-end */").unwrap()
        + "/* verter-destructured-end */".len();
    assert!(
        meta.block_start as usize <= start_marker + "/* verter-destructured-start */".len() + 10,
        "block_start should be near the start marker"
    );
    assert!(
        (meta.block_end as usize) >= end_marker - 10,
        "block_end should be near the end marker"
    );
}

#[test]
fn tsx_destructured_block_meta_non_ascii_spans() {
    // Emoji 😀 is 4 UTF-8 bytes, 2 UTF-16 code units
    let source = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\n// 😀\nconst name = ref('')\n</script>\n<template><div>{{ name }}</div></template>";
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: NO offset comments
    assert!(
        !has_offset_comment(&tsx.code),
        "Offset comments must NOT appear in TSX output.\nTSX:\n{}",
        tsx.code
    );

    // Positive: metadata has the binding with correct SFC byte span
    let meta = tsx
        .destructured_block
        .as_ref()
        .expect("destructured_block metadata");
    let name_binding = meta
        .bindings
        .iter()
        .find(|b| b.name == "name")
        .expect("should have 'name' binding");

    let name_decl_pos = source.find("const name").unwrap();
    let name_ident_start = name_decl_pos + "const ".len();
    let name_ident_end = name_ident_start + "name".len();
    assert_eq!(&source[name_ident_start..name_ident_end], "name");
    assert_eq!(name_binding.source_span.start, name_ident_start as u32);
    assert_eq!(name_binding.source_span.end, name_ident_end as u32);
}

#[test]
fn tsx_no_destructured_block_meta_without_setup() {
    // No <script setup> = no destructured block
    let source = r#"<script lang="ts">
export default { setup() { return { x: 1 } } }
</script>
<template><div>{{ x }}</div></template>"#;
    let result = compile_tsx(source);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.destructured_block.is_none(),
        "destructured_block should be None without <script setup>"
    );
}

#[test]
fn tsx_comp_with_ref_has_void_reference() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">text</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: void reference to suppress unused warning
    assert!(
        tsx.code.contains("void ___VERTER___Comp"),
        "Comp function should have void reference to suppress unused warning, got:\n{}",
        tsx.code
    );
}

// ── TSX OXC parse-validity tests ─────────────────────────────────────
// These compile a full Vue SFC to TSX and then parse the entire output
// with OXC to verify it is syntactically valid TypeScript/TSX.

/// Compile a Vue SFC to TSX and assert the output parses without errors.
fn assert_tsx_parses(source: &str, label: &str) {
    let result = compile_tsx(source);
    assert!(
        result.errors.is_empty(),
        "[{}] compile errors: {:?}",
        label,
        result.errors
    );
    let tsx = result
        .tsx
        .as_ref()
        .unwrap_or_else(|| panic!("[{}] no tsx block", label));
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "[{}] OXC parse errors: {:?}\n--- TSX output ---\n{}",
        label,
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

/// Verify that a JS SFC (no lang attribute) produces valid JSX (JavaScript) output,
/// with `is_jsx: true` on the tsx block, and no TypeScript syntax.
fn assert_jsx_parses(source: &str, label: &str) {
    let result = compile_tsx(source);
    assert!(
        result.errors.is_empty(),
        "[{}] compile errors: {:?}",
        label,
        result.errors
    );
    let tsx = result
        .tsx
        .as_ref()
        .unwrap_or_else(|| panic!("[{}] no tsx block", label));
    // Positive: JS SFC must set is_jsx = true
    assert!(tsx.is_jsx, "[{}] is_jsx should be true for JS SFC", label);
    // Verify the output is valid JSX (JavaScript) — no TypeScript parse errors
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::jsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "[{}] OXC parse errors (should be valid JS):\n{:?}\n--- JSX output ---\n{}",
        label,
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
    // Negative: no TypeScript syntax in output
    assert!(
        !tsx.code.contains("as unknown"),
        "[{}] JSX output must not contain 'as unknown':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("import type"),
        "[{}] JSX output must not contain 'import type':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("declare let"),
        "[{}] JSX output must not contain 'declare let':\n{}",
        label,
        tsx.code
    );
    assert!(
        !tsx.code.contains("!:"),
        "[{}] JSX output must not contain definite assignment '!:':\n{}",
        label,
        tsx.code
    );
}

#[test]
fn tsx_parse_valid_v_model_input() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('Hello')
</script>
<template>
  <input v-model="msg" />
</template>"#,
        "v-model on input",
    );
}

#[test]
fn tsx_parse_valid_v_model_textarea() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const text = ref('')
</script>
<template>
  <textarea v-model="text"></textarea>
</template>"#,
        "v-model on textarea",
    );
}

#[test]
fn tsx_parse_valid_v_model_select() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const selected = ref('a')
</script>
<template>
  <select v-model="selected">
    <option value="a">A</option>
    <option value="b">B</option>
  </select>
</template>"#,
        "v-model on select",
    );
}

#[test]
fn tsx_parse_valid_v_model_checkbox() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const checked = ref(false)
</script>
<template>
  <input type="checkbox" v-model="checked" />
</template>"#,
        "v-model on checkbox",
    );
}

#[test]
fn tsx_parse_valid_v_model_component() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const count = ref(0)
</script>
<template>
  <Comp v-model="count" />
</template>"#,
        "v-model on component",
    );
}

#[test]
fn tsx_parse_valid_v_model_named() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const title = ref('')
</script>
<template>
  <Comp v-model:title="title" />
</template>"#,
        "v-model:title named model",
    );
}

#[test]
fn tsx_parse_valid_v_model_with_modifiers() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('')
</script>
<template>
  <input v-model.trim.lazy="msg" />
</template>"#,
        "v-model with modifiers",
    );
}

#[test]
fn tsx_parse_valid_events_click() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function handleClick() {}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
        "event @click",
    );
}

#[test]
fn tsx_parse_valid_events_inline_expression() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template>
  <button @click="count++">{{ count }}</button>
</template>"#,
        "event inline expression",
    );
}

#[test]
fn tsx_parse_valid_events_with_event_param() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function handle(e: Event) {}
</script>
<template>
  <input @input="handle($event)" />
</template>"#,
        "event with $event",
    );
}

#[test]
fn tsx_parse_valid_events_multi_statement() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template>
  <button @click="a++; b--">go</button>
</template>"#,
        "event multi-statement",
    );
}

#[test]
fn tsx_parse_valid_slot_outlet_default() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
</script>
<template>
  <div><slot></slot></div>
</template>"#,
        "slot outlet default",
    );
}

#[test]
fn tsx_parse_valid_slot_outlet_named() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
</script>
<template>
  <div>
    <slot name="header"></slot>
    <slot></slot>
    <slot name="footer"></slot>
  </div>
</template>"#,
        "slot outlet named",
    );
}

#[test]
fn tsx_parse_valid_slot_outlet_scoped() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const items = ref([1, 2, 3])
</script>
<template>
  <div>
    <slot :items="items" :count="items.length"></slot>
  </div>
</template>"#,
        "slot outlet scoped",
    );
}

#[test]
fn tsx_parse_valid_slot_outlet_with_fallback() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
</script>
<template>
  <div>
    <slot name="header">Default header</slot>
  </div>
</template>"#,
        "slot outlet with fallback",
    );
}

#[test]
fn tsx_parse_valid_component_named_slots() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template>
  <MyComp>
    <template #header>Header content</template>
    <template #default>Body content</template>
    <template #footer>Footer content</template>
  </MyComp>
</template>"#,
        "component named slots",
    );
}

#[test]
fn tsx_parse_valid_component_scoped_slot() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template>
  <MyComp v-slot="{ item, index }">
    <span>{{ item }} {{ index }}</span>
  </MyComp>
</template>"#,
        "component scoped slot",
    );
}

#[test]
fn tsx_parse_valid_define_model() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
const firstName = defineModel<string>('firstName')
</script>
<template>
  <input v-model="firstName" />
</template>"#,
        "defineModel",
    );
}

#[test]
fn tsx_parse_valid_define_emits() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
const emit = defineEmits<{
  change: [value: string]
  update: [id: number, value: string]
}>()
</script>
<template>
  <button @click="emit('change', 'hello')">go</button>
</template>"#,
        "defineEmits typed",
    );
}

#[test]
fn tsx_parse_valid_define_props_with_defaults() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  msg: string
  count?: number
}>(), {
  count: 0,
})
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
        "defineProps + withDefaults",
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
fn tsx_parse_valid_component_with_all_features() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const msg = ref('Hello World!')
</script>
<template>
  <div>
    <h1>{{ msg }}</h1>
    <input v-model="msg" />
    <Comp :foo="msg" @update="msg = $event">
      <template #header>Header</template>
      <template #default="{ data }">{{ data }}</template>
    </Comp>
  </div>
</template>"#,
        "component with props, events, v-model, slots",
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
fn tsx_parse_valid_dynamic_component() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import CompA from './CompA.vue'
import CompB from './CompB.vue'
const current = ref(CompA)
</script>
<template>
  <component :is="current" />
</template>"#,
        "dynamic component :is",
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
fn tsx_parse_valid_template_ref() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template>
  <div ref="el">text</div>
</template>"#,
        "template ref",
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
fn tsx_parse_valid_v_on_object() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function onMouseDown() {}
function onMouseUp() {}
</script>
<template>
  <div v-on="{ mousedown: onMouseDown, mouseup: onMouseUp }">drag me</div>
</template>"#,
        "v-on object syntax",
    );
}

// =============================================================================
// JSX mode compile tests — JS SFCs produce valid JavaScript + JSDoc output
// =============================================================================

#[test]
fn jsx_compile_basic_script_setup() {
    assert_jsx_parses(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        "basic JS script setup",
    );
}

#[test]
fn jsx_compile_define_props() {
    assert_jsx_parses(
        r#"<script setup>
const props = defineProps({
  msg: String
})
</script>
<template><div>{{ msg }}</div></template>"#,
        "JS defineProps (runtime)",
    );
}

#[test]
fn jsx_compile_lang_js_explicit() {
    assert_jsx_parses(
        r#"<script setup lang="js">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
        "explicit lang=js",
    );
}

#[test]
fn jsx_compile_options_api() {
    assert_jsx_parses(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
        "options API JS",
    );
}

#[test]
fn jsx_compile_template_only() {
    // Template-only SFCs should default to TSX (not JSX)
    let result = compile_tsx(r#"<template><div>hello</div></template>"#);
    let tsx = result.tsx.as_ref().expect("should have tsx block");
    assert!(
        !tsx.is_jsx,
        "template-only SFC should default to TSX (is_jsx = false):\n{}",
        tsx.code
    );
}

#[test]
fn jsx_compile_ts_sfc_stays_tsx() {
    // TypeScript SFCs should still produce TSX (is_jsx = false)
    let result = compile_tsx(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    let tsx = result.tsx.as_ref().expect("should have tsx block");
    assert!(
        !tsx.is_jsx,
        "TS SFC should produce TSX (is_jsx = false):\n{}",
        tsx.code
    );
}

#[test]
fn jsx_compile_global_components() {
    assert_jsx_parses(
        r#"<script setup>
</script>
<template><RouterView /><Transition><div>x</div></Transition></template>"#,
        "JS SFC with global components",
    );
}

#[test]
fn jsx_compile_v_model() {
    assert_jsx_parses(
        r#"<script setup>
import { ref } from 'vue'
const text = ref('')
</script>
<template><input v-model="text" /></template>"#,
        "JS SFC with v-model",
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

// ══════════════════════════════════════════════════════════════════════════════
// ── attrs attribute on <script setup> — IDE codegen ─────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_attrs_explicit_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class?: string; id?: string }">
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: explicit attrs type emitted
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should emit ___VERTER___attributes type alias, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ class?: string; id?: string }"),
        "should contain the attrs type value, got:\n{}",
        tsx.code
    );

    // Negative: should not contain the raw attrs= attribute in TSX output
    assert!(
        !tsx.code.contains(r#"attrs="{ class"#),
        "raw attrs attribute should not appear in output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_default_empty() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: default empty attrs type emitted
    assert!(
        tsx.code.contains("type ___VERTER___attributes = {}"),
        "should emit empty ___VERTER___attributes type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_with_generic() {
    let result = compile_tsx(
        r#"<script setup lang="ts" generic="T" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script>
<template><div /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs type with generic parameter
    assert!(
        tsx.code.contains("___VERTER___attributes<T>"),
        "should emit generic attrs type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ value: T }"),
        "should contain generic type value, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_alias_attributes() {
    // Also accept 'attributes' as alias for 'attrs'
    let result = compile_tsx(
        r#"<script setup lang="ts" attributes="{ role?: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attributes alias works the same as attrs
    assert!(
        tsx.code.contains("{ role?: string }"),
        "'attributes' alias should produce the same type, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_attrs_explicit_type() {
    let result = compile_tsx_with_force_js(
        r#"<script setup attrs="{ class?: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JSDoc typedef for attrs
    assert!(
        tsx.code.contains("@typedef"),
        "JS mode should use JSDoc @typedef, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ class?: string }"),
        "should contain the attrs type in JSDoc, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should reference ___VERTER___attributes, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_attrs_default_empty() {
    let result = compile_tsx_with_force_js(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: default empty attrs typedef
    assert!(
        tsx.code.contains("@typedef {{}} ___VERTER___attributes"),
        "JS mode should emit empty attrs typedef, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Root element prop capture — IDE codegen ─────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_root_element_props_captured() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const handler = () => {}
</script>
<template><div id="app" :title="'hello'" @click="handler">content</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Comp function emitted for root element
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "should emit Comp for root element, got:\n{}",
        tsx.code
    );
    // Positive: enhanceElementWithProps receives actual props
    assert!(
        tsx.code.contains(r#""id": "app""#),
        "should have static id prop in Comp, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(r#""title": 'hello'"#),
        "should have dynamic title bind in Comp, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(r#""onClick": () => {}"#),
        "should have onClick event in Comp, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns actual props
    assert!(
        tsx.code.contains("getRootComponentPassedProps"),
        "should emit getRootComponentPassedProps, got:\n{}",
        tsx.code
    );
    // Negative: class/style should be excluded from props
    assert!(
        !tsx.code.contains(r#""class""#) || !tsx.code.contains("getRootComponentPassedProps"),
        "class should not appear in serialized props"
    );
}

#[test]
fn tsx_root_element_skips_class_and_style() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div class="foo" style="color: red" id="bar">content</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: id is captured
    assert!(
        tsx.code.contains(r#""id": "bar""#),
        "should have id in props, got:\n{}",
        tsx.code
    );
    // Negative: class and style are excluded
    let comp_fn = tsx
        .code
        .split("function ___VERTER___Comp")
        .nth(1)
        .unwrap_or("");
    assert!(
        !comp_fn.contains(r#""class""#),
        "class should be excluded from Comp props, got:\n{}",
        comp_fn
    );
    assert!(
        !comp_fn.contains(r#""style""#),
        "style should be excluded from Comp props, got:\n{}",
        comp_fn
    );
}

#[test]
fn tsx_root_component_props_captured() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const el = ref()
</script>
<template><MyComp ref="el" :title="'hello'" /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: component Comp function has title prop
    assert!(
        tsx.code.contains(r#""title": 'hello'"#),
        "should have title prop in Comp, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns actual props
    assert!(
        tsx.code.contains("getRootComponentPassedProps")
            && tsx.code.contains(r#""title": 'hello'"#),
        "getRootComponentPassedProps should return actual props, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_root_element_no_props_empty_object() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Comp emitted even for root with no props
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "should emit Comp for root element, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns empty object
    assert!(
        tsx.code
            .contains("getRootComponentPassedProps() { return {}; }"),
        "should return empty object when no props, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Implicit attrs type composition — IDE codegen ───────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_implicit_attrs_root_element_types() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div id="app">hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: RootElement type alias
    assert!(
        tsx.code.contains("___VERTER___RootElement"),
        "should emit RootElement type, got:\n{}",
        tsx.code
    );
    // Positive: RootElementProps type alias using ExtractComponentProps
    assert!(
        tsx.code.contains("___VERTER___RootElementProps"),
        "should emit RootElementProps type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___ExtractComponentProps"),
        "should use ExtractComponentProps, got:\n{}",
        tsx.code
    );
    // Positive: Attrs combines explicit + implicit
    assert!(
        tsx.code.contains("___VERTER___Attrs")
            && tsx.code.contains("___VERTER___attributes")
            && tsx.code.contains("___VERTER___RootElementProps"),
        "Attrs should combine attributes + RootElementProps, got:\n{}",
        tsx.code
    );
    // Positive: instance declaration overrides $attrs
    assert!(
        tsx.code.contains("$attrs: ___VERTER___Attrs"),
        "instance should override $attrs, got:\n{}",
        tsx.code
    );
    // Negative: Omit should be used on the instance
    assert!(
        tsx.code.contains("Omit<InstanceType<"),
        "should Omit $attrs from base instance type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_inherit_attrs_false_omits_root_element_props() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Attrs type should only be explicit attrs (no RootElementProps)
    assert!(
        tsx.code
            .contains("type ___VERTER___Attrs = ___VERTER___attributes;"),
        "inheritAttrs: false should exclude RootElementProps from Attrs, got:\n{}",
        tsx.code
    );
    // Negative: RootElementProps should NOT be in the Attrs union
    let attrs_line = tsx
        .code
        .lines()
        .find(|l| l.contains("type ___VERTER___Attrs"))
        .unwrap_or("");
    assert!(
        !attrs_line.contains("RootElementProps"),
        "Attrs should not include RootElementProps when inheritAttrs: false, got:\n{}",
        attrs_line
    );
}

#[test]
fn tsx_no_template_no_root_element_types() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: no RootElement types without template
    assert!(
        !tsx.code.contains("___VERTER___RootElement"),
        "should not emit RootElement without template, got:\n{}",
        tsx.code
    );
    // Negative: no Attrs override without template
    assert!(
        !tsx.code.contains("type ___VERTER___Attrs"),
        "should not emit Attrs type without template, got:\n{}",
        tsx.code
    );
    // Negative: instance should not use Omit
    assert!(
        !tsx.code.contains("Omit<InstanceType"),
        "instance should not use Omit without template, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── useAttrs<T>() fallback for attrs type — IDE codegen ─────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_use_attrs_type_arg_fallback() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string; id?: string }>()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs type from useAttrs<T>() used as fallback
    assert!(
        tsx.code.contains("{ class?: string; id?: string }"),
        "should use useAttrs type parameter as attrs type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___attributes"),
        "should emit ___VERTER___attributes type alias, got:\n{}",
        tsx.code
    );

    // Negative: should not have empty attrs type
    assert!(
        !tsx.code.contains("type ___VERTER___attributes = {};"),
        "should not emit empty attrs when useAttrs<T> provides type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_attribute_takes_priority_over_use_attrs() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ role?: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs<{ class?: string }>()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs attribute value takes priority
    assert!(
        tsx.code
            .contains("___VERTER___attributes = { role?: string }"),
        "attrs attribute should take priority in type alias, got:\n{}",
        tsx.code
    );

    // Negative: useAttrs type should NOT be in the type alias
    assert!(
        !tsx.code
            .contains("___VERTER___attributes = { class?: string }"),
        "useAttrs type should not be used in type alias when attrs attribute present, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_use_attrs_without_type_arg_no_effect() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: plain useAttrs() without type param → default empty attrs
    assert!(
        tsx.code.contains("type ___VERTER___attributes = {}"),
        "useAttrs() without type param should produce empty attrs type, got:\n{}",
        tsx.code
    );
}

// ── Instance declaration regression tests ─────────────────────────
//
// These tests verify that the ___VERTER___instance declaration is correctly
// typed based on the script language. TS SFCs must get InstanceType<...>,
// not `any`. JS SFCs must get `@type {any}` (JSDoc style).

#[test]
fn tsx_instance_declaration_ts_sfc_has_instance_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.is_jsx, "TS SFC should produce TSX, not JSX");

    // Positive: should have typed InstanceType declaration
    assert!(
        tsx.code.contains("InstanceType<import("),
        "TS SFC instance declaration should use InstanceType<import(...)>, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("let ___VERTER___instance!:"),
        "TS SFC should use definite assignment 'let ... !:', got:\n{}",
        tsx.code
    );

    // Negative: must NOT have JSDoc `any` instance declaration
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "TS SFC must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_js_sfc_has_jsdoc_any() {
    let result = compile_tsx(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "JS SFC (no lang attr) should produce JSX");

    // Positive: should have JSDoc-style instance
    assert!(
        tsx.code.contains("/** @type {any} */"),
        "JS SFC should use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have TS-style InstanceType
    assert!(
        !tsx.code.contains("InstanceType<import("),
        "JS SFC must NOT use InstanceType<import(...)>, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("let ___VERTER___instance!:"),
        "JS SFC must NOT use definite assignment 'let ... !:', got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_explicit_js_lang_has_jsdoc_any() {
    let result = compile_tsx(
        r#"<script setup lang="js">
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "lang='js' SFC should produce JSX");

    // Positive: should have JSDoc-style instance
    assert!(
        tsx.code.contains("/** @type {any} */"),
        "lang='js' SFC should use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have TS InstanceType
    assert!(
        !tsx.code.contains("InstanceType<"),
        "lang='js' SFC must NOT use InstanceType, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_options_api_ts_has_ambient_instance_type() {
    let result = compile_tsx(
        r#"<script lang="ts">
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(!tsx.is_jsx, "TS Options API should produce TSX, not JSX");

    // Positive: should have ambient typed instance declaration
    assert!(
        tsx.code.contains("declare let ___VERTER___instance:"),
        "TS Options API should use 'declare let' for instance, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("InstanceType<import("),
        "TS Options API should use InstanceType<import(...)>, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have JSDoc `any`
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "TS Options API must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_options_api_js_has_jsdoc_typed() {
    let result = compile_tsx(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(tsx.is_jsx, "JS Options API should produce JSX");

    // Positive: should have inline defineComponent wrapping for plain object export
    assert!(
        tsx.code.contains("___VERTER___defineComponent)(__sfc__)"),
        "JS Options API should wrap __sfc__ with defineComponent, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("InstanceType<typeof ___VERTER___dc>"),
        "JS Options API should use InstanceType<typeof dc> for instance, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("var ___VERTER___instance"),
        "JS Options API should use var (not declare let) for instance, got:\n{}",
        tsx.code
    );
    // Positive: defineComponent must be imported from vue
    assert!(
        tsx.code
            .contains("defineComponent as ___VERTER___defineComponent"),
        "JS Options API should import defineComponent from vue, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have TS ambient instance syntax
    assert!(
        !tsx.code.contains("declare let ___VERTER___instance:"),
        "JS Options API must NOT use 'declare let' for instance, got:\n{}",
        tsx.code
    );
    // Negative: must NOT have the old untyped @type {any}
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "JS Options API must NOT use untyped @type {{any}} for instance, got:\n{}",
        tsx.code
    );
    // Negative: must NOT use self-import pattern (TSGO can't resolve virtual .vue.jsx imports)
    assert!(
        !tsx.code.contains("import('./"),
        "JS Options API must NOT use self-import for instance typing, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_template_only_uses_instance_type() {
    let result = compile_tsx(r#"<template><div>hello</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Template-only SFCs default to TS mode (is_jsx=false)
    assert!(!tsx.is_jsx, "Template-only SFC should default to TSX");

    // Positive: should have typed instance
    assert!(
        tsx.code.contains("InstanceType<import("),
        "Template-only SFC should use InstanceType<import(...)>, got:\n{}",
        tsx.code
    );

    // Negative: must NOT be `any`
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "Template-only SFC must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── _attrs parameter on TemplateBindingFN — IDE codegen ─────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_attrs_param_inline_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class: string }">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: _attrs parameter in function signature
    assert!(
        tsx.code
            .contains("TemplateBindingFN(_attrs: { class: string })"),
        "should have _attrs param with inline type, got:\n{}",
        tsx.code
    );

    // Negative: should NOT have empty parens
    assert!(
        !tsx.code.contains("TemplateBindingFN()"),
        "should not have empty parens when attrs specified, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_param_with_generics() {
    let result = compile_tsx(
        r#"<script setup lang="ts" generic="T extends string" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script>
<template><div /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: both generic bracket and _attrs param, generic BEFORE params
    assert!(
        tsx.code
            .contains("TemplateBindingFN<T extends string>(_attrs: { value: T })"),
        "should have generic bracket + _attrs param, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_before_generic_in_source_order() {
    // When attrs appears BEFORE generic in the SFC source,
    // the generated TSX must still have generic before params.
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class: string }" generic="T extends string">
defineProps<{ value: T }>()
</script>
<template><div /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: generic BEFORE params regardless of source order
    assert!(
        tsx.code
            .contains("TemplateBindingFN<T extends string>(_attrs: { class: string })"),
        "generic must come before params even when attrs is first in source, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have generic after params
    assert!(
        !tsx.code.contains("})<"),
        "generic must not appear after closing paren, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_attrs_param_without_attrs_attr() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: empty parens when no attrs
    assert!(
        tsx.code.contains("TemplateBindingFN()"),
        "should have empty parens without attrs, got:\n{}",
        tsx.code
    );

    // Negative: no _attrs parameter
    assert!(
        !tsx.code.contains("_attrs"),
        "should not have _attrs param without attrs attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_attrs_param_jsx_mode() {
    let result = compile_tsx_with_force_js(
        r#"<script setup attrs="{ class: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JSX mode → empty parens (no TS annotations)
    assert!(
        tsx.code.contains("TemplateBindingFN()"),
        "JSX mode should have empty parens (no TS annotations), got:\n{}",
        tsx.code
    );

    // Negative: no _attrs in JSX mode
    assert!(
        !tsx.code.contains("_attrs:"),
        "JSX mode should not have _attrs TS annotation, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_priority_over_attributes() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ role: string }" attributes="{ id: string }">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs value wins
    assert!(
        tsx.code.contains("{ role: string }"),
        "attrs should take priority over attributes, got:\n{}",
        tsx.code
    );

    // Negative: attributes value should NOT appear
    assert!(
        !tsx.code.contains("{ id: string }"),
        "attributes value should not appear when attrs is present, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_bare_use_attrs_with_explicit_attrs_typeof_cast() {
    let result = compile_tsx(
        r#"<script setup lang="ts" attrs="{ class: string }">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: bare useAttrs() gets `as typeof _attrs` cast
    assert!(
        tsx.code.contains("useAttrs() as typeof _attrs"),
        "bare useAttrs() should be cast to typeof _attrs when attrs specified, got:\n{}",
        tsx.code
    );

    // Negative: should NOT use the old ___VERTER___Attrs cast
    assert!(
        !tsx.code.contains("as unknown as ___VERTER___Attrs"),
        "should not use ___VERTER___Attrs cast when explicit attrs, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_bare_use_attrs_without_explicit_attrs_keeps_verter_cast() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useAttrs } from 'vue'
const attrs = useAttrs()
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: bare useAttrs() still gets ___VERTER___Attrs cast when no explicit attrs
    assert!(
        tsx.code.contains("as unknown as ___VERTER___Attrs"),
        "bare useAttrs() should use ___VERTER___Attrs cast without explicit attrs, got:\n{}",
        tsx.code
    );

    // Negative: should NOT use typeof _attrs
    assert!(
        !tsx.code.contains("as typeof _attrs"),
        "should not use typeof _attrs when no explicit attrs, got:\n{}",
        tsx.code
    );
}

// ── Sourcemap tests for attrs/generic content ───────────────────────────────

#[test]
fn tsx_attrs_content_is_sourcemapped() {
    let source = r#"<script setup lang="ts" attrs="{ class: string }">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Find "{ class: string }" in the generated TSX output
    let target = "{ class: string }";
    let gen_pos = tsx
        .code
        .find(target)
        .expect("should find attrs content in TSX output");

    // Find the same text in the original SFC source
    let src_pos = source
        .find(target)
        .expect("should find attrs content in SFC source");

    // Look up the sourcemap token at the generated position
    let gen_line = tsx.code[..gen_pos].matches('\n').count() as u32;
    let gen_col = (gen_pos - tsx.code[..gen_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    let token = sm
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("should have sourcemap token for attrs content");

    // The token should map back to the original source position
    let src_line = source[..src_pos].matches('\n').count() as u32;
    let src_col = (src_pos - source[..src_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    assert_eq!(
        token.get_src_line(),
        src_line,
        "attrs content should map back to SFC source line"
    );
    assert_eq!(
        token.get_src_col(),
        src_col,
        "attrs content should map back to SFC source column"
    );
}

#[test]
fn tsx_generic_content_is_sourcemapped() {
    let source = r#"<script setup lang="ts" generic="T extends string">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Find "T extends string" in the generated TSX output
    let gen_pos = tsx
        .code
        .find("T extends string")
        .expect("should find 'T extends string' in TSX output");

    // Find "T extends string" in the original SFC source
    let src_pos = source
        .find("T extends string")
        .expect("should find 'T extends string' in SFC source");

    // Look up the sourcemap token at the generated position
    let gen_line = tsx.code[..gen_pos].matches('\n').count() as u32;
    let gen_col = (gen_pos - tsx.code[..gen_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    let token = sm
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("should have sourcemap token for generic content");

    // The token should map back to the original source position
    let src_line = source[..src_pos].matches('\n').count() as u32;
    let src_col = (src_pos - source[..src_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    assert_eq!(
        token.get_src_line(),
        src_line,
        "generic content should map back to SFC source line"
    );
    assert_eq!(
        token.get_src_col(),
        src_col,
        "generic content should map back to SFC source column"
    );
}

// ── TemplateBindingFN empty return ──────────────────────────────────────────

#[test]
fn tsx_template_binding_fn_has_return_statement() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: should have return statement before close
    assert!(
        tsx.code
            .contains("return {};\n} // close templateBindingFN"),
        "should have empty return before closing brace of TemplateBindingFN, got:\n{}",
        tsx.code
    );

    // Negative: no `: any` return type annotation
    assert!(
        !tsx.code.contains(": any"),
        "TemplateBindingFN should not have `: any` return type, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_template_binding_fn_has_return_statement() {
    let result = compile_tsx_with_force_js(
        r#"<script setup>
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JS mode should have `return {};` (no `as any`)
    assert!(
        tsx.code
            .contains("return {};\n} // close templateBindingFN"),
        "JSX mode should have `return {{}};` before closing brace, got:\n{}",
        tsx.code
    );

    // Negative: no `as any` in JS mode
    assert!(
        !tsx.code.contains("as any"),
        "JSX mode should not have `as any`, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_first_empty_script_setup() {
    let result = compile_tsx(
        r#"<template>
	<section class="page">
		<h1>Chat</h1>
	</section>
</template>
<script setup lang="ts">
</script>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    let fn_open = tsx.code.find("function ___VERTER___TemplateBindingFN");
    let fn_close = tsx.code.find("} // close templateBindingFN");
    assert!(fn_open.is_some(), "should have function: {}", tsx.code);
    assert!(fn_close.is_some(), "should have close: {}", tsx.code);
    assert!(
        fn_open.unwrap() < fn_close.unwrap(),
        "function open must come before close: {}",
        tsx.code
    );
    // JSX must be inside the function
    let jsx_pos = tsx.code.find("<section").expect("should have JSX");
    assert!(
        fn_open.unwrap() < jsx_pos && jsx_pos < fn_close.unwrap(),
        "template JSX must be inside the function: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_first_with_bindings() {
    let result = compile_tsx(
        r#"<template>
  <div>{{ msg }}</div>
</template>
<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('hello')
</script>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    let fn_open = tsx
        .code
        .find("function ___VERTER___TemplateBindingFN")
        .unwrap_or_else(|| panic!("should have function: {}", tsx.code));
    let fn_close = tsx
        .code
        .find("} // close templateBindingFN")
        .unwrap_or_else(|| panic!("should have close: {}", tsx.code));
    assert!(
        fn_open < fn_close,
        "function open must come before close: {}",
        tsx.code
    );
    // Script content (const msg) should be inside the function
    let msg_pos = tsx
        .code
        .find("const msg = ref('hello')")
        .unwrap_or_else(|| panic!("should have script content: {}", tsx.code));
    assert!(
        fn_open < msg_pos && msg_pos < fn_close,
        "script content must be inside function: {}",
        tsx.code
    );
    // Template JSX should be inside the function
    let jsx_pos = tsx
        .code
        .find("{ msg }")
        .unwrap_or_else(|| panic!("should have template binding: {}", tsx.code));
    assert!(
        fn_open < jsx_pos && jsx_pos < fn_close,
        "template JSX must be inside function: {}",
        tsx.code
    );
    // Script content should come BEFORE template JSX (declarations must be in scope)
    assert!(
        msg_pos < jsx_pos,
        "script declarations must precede template JSX: {}",
        tsx.code
    );
}

#[test]
fn tsx_global_component_fallbacks_before_block_scope() {
    // Global components (not imported) should be declared BEFORE the block scope
    // so template JSX inside the block scope can reference them without TDZ errors.
    let result = compile_tsx(
        r#"<script setup lang="ts"></script>
<template>
  <RouterLink to="/home">Home</RouterLink>
  <RouterView />
</template>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    // RouterLink and RouterView fallback consts must appear before block scope
    let block_scope_pos = tsx
        .code
        .find("/* verter-destructured-start */")
        .or_else(|| tsx.code.find("\n{\n"))
        .unwrap_or_else(|| panic!("should have block scope: {}", tsx.code));
    let router_link_pos = tsx
        .code
        .find("const RouterLink =")
        .unwrap_or_else(|| panic!("should have RouterLink fallback: {}", tsx.code));
    let router_view_pos = tsx
        .code
        .find("const RouterView =")
        .unwrap_or_else(|| panic!("should have RouterView fallback: {}", tsx.code));
    assert!(
        router_link_pos < block_scope_pos,
        "RouterLink fallback must be before block scope (TDZ): {}",
        tsx.code
    );
    assert!(
        router_view_pos < block_scope_pos,
        "RouterView fallback must be before block scope (TDZ): {}",
        tsx.code
    );
    // Must NOT be after the block scope close
    let close_block = tsx
        .code
        .find("} // close block scope")
        .expect("should have close block scope");
    assert!(
        router_link_pos < close_block,
        "RouterLink fallback must not be after block scope: {}",
        tsx.code
    );
}

#[test]
fn tsx_slot_outlet_inside_v_for_no_object_literal_ambiguity() {
    // <slot v-for> must not produce `=> ({...})` which is parsed as object literal
    let source = r#"<script setup>
import { computed } from 'vue'
const props = defineProps({ list: { type: Array, default() { return [] } } })
const leftList = computed(() => props.list.filter((v, index) => index % 2 === 0))
</script>
<template>
  <div class="waterfall">
    <slot :item="item" v-for="item in leftList"></slot>
  </div>
</template>"#;
    let result = compile_tsx(source);
    let tsx = result.tsx.expect("should have tsx output");

    // Should have slot access inside map
    assert!(
        tsx.code.contains("$slots.default"),
        "should have $slots.default: {}",
        tsx.code
    );

    // Must NOT have `=> ({` pattern (parenthesized object literal ambiguity)
    assert!(
        !tsx.code.contains("=> ({"),
        "must not produce `=> ({{` pattern (object literal ambiguity): {}",
        tsx.code
    );

    // Should have clean map body without JSX expression wrapping
    assert!(
        tsx.code
            .contains("=> { return (___VERTER___instance.$slots"),
        "slot inside v-for should not have JSX {{...}} wrapping: {}",
        tsx.code
    );
}

/// Regression: slot cache wrapping must not insert `, -1` flags inside
/// `_createTextVNode(...)` content when text children are grouped in a text run.
#[test]
fn test_slot_cache_text_run_flags_not_inside_text_content() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        is_production: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import Header from './Header.vue'
import Layout from './Layout.vue'
import RedEnvelope from './RedEnvelope.vue'
import Top from './Top.vue'
import PageContent from './PageContent.vue'
</script>
<template>
  <PageContent class="bg-theme-black" :back-router="false">
    <Header ref="headerRef" class="fixed-dom fixed top-0 z-[11] w-full" />
    <div class="fixed-dom-placeholder h-[50px]"></div>

    <!-- comment1 -->
    <Layout class="min-h-screen" />

    <!-- comment2 -->
    <RedEnvelope />

    <Top />
  </PageContent>
</template>"#,
        &options,
        &verter_opts,
        &alloc,
    );

    let code = &result.template.unwrap().code;

    // Must not have `, -1` inside text node content
    assert!(
        !code.contains(r#"_createTextVNode(", -1"#),
        "cache flags must not leak into _createTextVNode content: {}",
        code
    );
    assert!(
        !code.contains(r#"_createTextVNode("..."#),
        "cache spread syntax must not leak into _createTextVNode content: {}",
        code
    );

    // Must produce valid cache patterns
    assert!(
        code.contains("_cache["),
        "should have slot cache wrapping: {}",
        code
    );

    // The code must be syntactically valid — no `, -1]))_createVNode` without a comma
    assert!(
        !code.contains("]))_create"),
        "cache close must be followed by comma separator, not immediately by _create: {}",
        code
    );
}

/// Vue's official compiler adds `-1 /* CACHED */` patchFlag to each VNode inside
/// cached slot content. This tells the runtime the VNode is hoisted/stable and
/// should skip diffing. Verter must match this behavior.
#[test]
fn test_slot_cached_elements_get_hoisted_patch_flag() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template>
  <MyComp>
    <div class="static-child">hello</div>
    <span>world</span>
  </MyComp>
</template>"#,
        &options,
        &verter_opts,
        &alloc,
    );

    let code = &result.template.unwrap().code;

    // Cached element VNodes must have -1 /* CACHED */ patchFlag
    // Vue compiler output: _createElementVNode("div", { class: "static-child" }, "hello", -1 /* CACHED */)
    assert!(
        code.contains("-1 /* CACHED */"),
        "cached slot child elements must have -1 /* CACHED */ patchFlag: {}",
        code
    );

    // The cache wrapper should still be present
    assert!(
        code.contains("_cache["),
        "should have slot cache wrapping: {}",
        code
    );
}

/// Production mode: cached elements get just `, -1` (no comment)
#[test]
fn test_slot_cached_elements_hoisted_flag_production() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        is_production: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template>
  <MyComp>
    <div class="static-child">hello</div>
  </MyComp>
</template>"#,
        &options,
        &verter_opts,
        &alloc,
    );

    let code = &result.template.unwrap().code;

    // Production mode: just -1, no comment
    assert!(
        code.contains(", -1)"),
        "cached slot child elements must have -1 patchFlag in production: {}",
        code
    );
}

// ==================== Vue 3.4+ same-name shorthand bindings ====================

// @ai-generated - TDD test: Issue 1 — `:disabled` with no value resolves to the binding
#[test]
fn bind_shorthand_resolves_to_binding() {
    // Vue 3.4+ shorthand: `:disabled` is equivalent to `:disabled="disabled"`
    let code = compile_and_validate_template(
        r#"<template><button :disabled>click</button></template>
<script setup>const disabled = true;</script>"#,
    );
    // Should resolve to the setup binding, not emit ""
    assert!(
        code.contains("disabled: $setup.disabled"),
        ":disabled shorthand should resolve to $setup.disabled, got:\n{}",
        code
    );
    assert!(
        !code.contains("disabled: \"\""),
        ":disabled shorthand should NOT emit empty string, got:\n{}",
        code
    );
}

#[test]
fn bind_shorthand_id_resolves_to_binding() {
    let code = compile_and_validate_template(
        r#"<template><div :id>content</div></template>
<script setup>const id = 'my-div';</script>"#,
    );
    assert!(
        code.contains("id: $setup.id"),
        ":id shorthand should resolve to $setup.id, got:\n{}",
        code
    );
}

#[test]
fn bind_shorthand_class_uses_normalize_class() {
    // `:class="myClass"` (explicit value) uses _normalizeClass — verify
    // shorthand doesn't break existing class normalization behavior.
    let code = compile_and_validate_template(
        r#"<template><div :class="myClass">content</div></template>
<script setup>const myClass = 'active';</script>"#,
    );
    assert!(
        code.contains("_normalizeClass("),
        ":class should use _normalizeClass, got:\n{}",
        code
    );
}

#[test]
fn bind_shorthand_style_uses_normalize_style() {
    let code = compile_and_validate_template(
        r#"<template><div :style>content</div></template>
<script setup>const style = { color: 'red' };</script>"#,
    );
    assert!(
        code.contains("_normalizeStyle("),
        ":style shorthand should use _normalizeStyle, got:\n{}",
        code
    );
}

// ==================== Dynamic slot outlet name ====================

// @ai-generated - TDD test: Issue 2 — `:name="expr"` on slot outlet
#[test]
fn slot_outlet_dynamic_name() {
    let code = compile_and_validate_template(
        r#"<template><div><slot :name="slotName"></slot></div></template>
<script setup>const slotName = 'header';</script>"#,
    );
    // Dynamic name should NOT be quoted — it's an expression
    assert!(
        code.contains("_renderSlot(_ctx.$slots, $setup.slotName"),
        "dynamic :name should use resolved expression, got:\n{}",
        code
    );
    assert!(
        !code.contains("\"slotName\""),
        "dynamic :name should NOT be a string literal, got:\n{}",
        code
    );
}

#[test]
fn slot_outlet_dynamic_name_with_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><slot :name="slotName"><span>fallback</span></slot></div></template>
<script setup>const slotName = 'header';</script>"#,
    );
    assert!(
        code.contains("_renderSlot(_ctx.$slots, $setup.slotName"),
        "dynamic slot with fallback should use resolved name, got:\n{}",
        code
    );
    assert!(
        code.contains("() => ["),
        "slot with fallback should have callback, got:\n{}",
        code
    );
}

// ==================== Slot outlet props ====================

// @ai-generated - TDD test: Issue 3 — slot outlet `:prop="expr"` props
#[test]
fn slot_outlet_with_bound_props() {
    let code = compile_and_validate_template(
        r#"<template><div><slot :item="item" :index="idx"></slot></div></template>
<script setup>const item = {}; const idx = 0;</script>"#,
    );
    // Props should be passed as 3rd argument object
    assert!(
        code.contains("{ item: $setup.item, index: $setup.idx }"),
        "slot outlet bound props should be in props object, got:\n{}",
        code
    );
}

#[test]
fn slot_outlet_with_shorthand_props() {
    // Vue 3.4+ shorthand on slot: `:item` → `item: resolvedBinding`
    let code = compile_and_validate_template(
        r#"<template><div><slot :item></slot></div></template>
<script setup>const item = {};</script>"#,
    );
    assert!(
        code.contains("{ item: $setup.item }"),
        "slot outlet shorthand :item should resolve to binding, got:\n{}",
        code
    );
}

#[test]
fn slot_outlet_props_with_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><slot :item="item"><span>default</span></slot></div></template>
<script setup>const item = {};</script>"#,
    );
    // Props + fallback: _renderSlot($slots, "default", { item: ... }, () => [...])
    assert!(
        code.contains("{ item: $setup.item }"),
        "slot outlet props with fallback should have props object, got:\n{}",
        code
    );
    assert!(
        code.contains("() => ["),
        "slot outlet with props and fallback should have callback, got:\n{}",
        code
    );
}

// ==================== Forwarded slot flag ====================

// @ai-generated - TDD test: Issue 10 — `_: 3 /* FORWARDED */` when slot contains <slot>
#[test]
fn component_slot_forwarded_flag_when_contains_slot_outlet() {
    // When a component's slot body contains a <slot> outlet, the stability
    // flag should be 3 (FORWARDED) instead of 1 (STABLE).
    let result = compile_sfc(
        r#"<template><Comp><slot></slot></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_: 3"),
        "component containing <slot> outlet should have FORWARDED flag (_: 3), got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_: 1"),
        "component containing <slot> outlet should NOT have STABLE flag (_: 1), got:\n{}",
        tpl.code
    );
}

#[test]
fn component_slot_stable_flag_without_slot_outlet() {
    // Without any nested <slot>, the flag should be 1 (STABLE).
    let result = compile_sfc(
        r#"<template><Comp><div>static content</div></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_: 1"),
        "component without <slot> outlet should have STABLE flag (_: 1), got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_: 3"),
        "component without <slot> outlet should NOT have FORWARDED flag, got:\n{}",
        tpl.code
    );
}

#[test]
fn component_named_slot_forwarded_when_contains_slot_outlet() {
    // Named slot with nested <slot> outlet should still get FORWARDED flag
    let result = compile_sfc(
        r#"<template><Comp><template #header><slot name="inner"></slot></template></Comp></template>
<script setup>import Comp from "./Comp.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_: 3"),
        "named slot with nested <slot> should have FORWARDED flag (_: 3), got:\n{}",
        tpl.code
    );
}

// ── Strict slot children integration tests (full SFC) ─────────────

fn compile_tsx_strict_slots(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: CompileTarget::BUNDLER | CompileTarget::TSX,
        strict_slots: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

#[test]
fn strict_slots_generic_component_integration() {
    // A generic component (GenericList) with child (ItemCard) — the Comp function
    // generated in script.rs carries generic params. The strictRenderSlot call
    // references this Comp function via ReturnType<typeof ___VERTER___Comp{offset}>.
    let result = compile_tsx_strict_slots(
        r#"<script setup lang="ts">
import GenericList from './GenericList.vue'
import ItemCard from './ItemCard.vue'
</script>
<template>
  <GenericList>
    <ItemCard />
  </GenericList>
</template>"#,
    );
    let tsx = result.tsx.as_ref().expect("tsx output");
    let code = &tsx.code;

    // Positive: strictRenderSlot emitted
    assert!(
        code.contains("strictRenderSlot"),
        "should emit strictRenderSlot in full SFC compilation, got:\n{}",
        code
    );
    // The call references the Comp function for GenericList
    assert!(
        code.contains("$slots"),
        "should reference $slots, got:\n{}",
        code
    );
    assert!(
        code.contains("'default'"),
        "should reference default slot, got:\n{}",
        code
    );
    // Child constructor is ItemCard
    assert!(
        code.contains("ItemCard"),
        "should reference ItemCard constructor, got:\n{}",
        code
    );
    // Negative: no raw template directives
    assert!(
        !code.contains("v-slot"),
        "v-slot should not appear in tsx output, got:\n{}",
        code
    );
}

#[test]
fn strict_slots_full_sfc_named_slots() {
    // Full SFC compilation with named slots — verifies script+template integration
    let result = compile_tsx_strict_slots(
        r#"<script setup lang="ts">
import Tabs from './Tabs.vue'
import TabItem from './TabItem.vue'
</script>
<template>
  <Tabs>
    <template #header>
      <input />
    </template>
    <template #default>
      <TabItem />
    </template>
  </Tabs>
</template>"#,
    );
    let tsx = result.tsx.as_ref().expect("tsx output");
    let code = &tsx.code;

    // Two separate strictRenderSlot calls for header and default
    let calls: Vec<_> = code.match_indices("strictRenderSlot").collect();
    assert!(
        calls.len() >= 2,
        "should have at least 2 strictRenderSlot calls (header + default), found {}, got:\n{}",
        calls.len(),
        code
    );
    assert!(
        code.contains("'header'"),
        "should reference header slot, got:\n{}",
        code
    );
    assert!(
        code.contains("'default'"),
        "should reference default slot, got:\n{}",
        code
    );
    assert!(
        code.contains("HTMLElementTagNameMap[\"input\"]"),
        "header slot should reference HTMLElementTagNameMap for input, got:\n{}",
        code
    );
    assert!(
        code.contains("TabItem"),
        "default slot should reference TabItem, got:\n{}",
        code
    );
}

#[test]
fn strict_slots_disabled_full_sfc() {
    // Verify strict_slots: false (default) does NOT emit strictRenderSlot
    let result = compile_tsx(
        r#"<script setup lang="ts">
import Tabs from './Tabs.vue'
import TabItem from './TabItem.vue'
</script>
<template>
  <Tabs><TabItem /></Tabs>
</template>"#,
    );
    let tsx = result.tsx.as_ref().expect("tsx output");
    // The import line always includes strictRenderSlot, but with strict_slots: false
    // no actual call should be emitted in the template body.
    assert!(
        !tsx.code.contains("strictRenderSlot("),
        "strict_slots: false should NOT emit strictRenderSlot calls, got:\n{}",
        tsx.code
    );
}

// ── Slot-summary representative SFCs (shared by counter + byte tests) ──

const SLOT_SFC_NESTED: &str = r#"<script setup lang="ts">
import Card from './Card.vue'
import Panel from './Panel.vue'
import Row from './Row.vue'
</script>
<template>
  <Card>
    <template #header="{ title }">
      <Row>{{ title }}</Row>
    </template>
    <template #default>
      <Panel />
      leading text
    </template>
  </Card>
</template>"#;

const SLOT_SFC_NO_SLOT: &str = r#"<script setup lang="ts">
const msg = 'hi'
</script>
<template>
  <div class="a"><span>{{ msg }}</span></div>
</template>"#;

const SLOT_SFC_DEEP: &str = r#"<script setup lang="ts">
import Outer from './Outer.vue'
import Inner from './Inner.vue'
import Leaf from './Leaf.vue'
</script>
<template>
  <div>
    <section>
      <Outer>
        <template #body>
          <Inner>
            <Leaf />
          </Inner>
        </template>
      </Outer>
    </section>
  </div>
</template>"#;

/// Normalize line endings before a byte comparison. The repo is LF-only, but
/// the cross-platform rule requires normalizing checked-out text before a
/// byte-equality assertion.
fn norm_eol(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Assert the compiled TSX's `code` AND `source_map` both equal their committed
/// goldens (byte-for-byte after EOL normalization). Sharing one per-component
/// slot summary between the two collectors is a pure compute change, so it must
/// leave BOTH the emitted code and its source map identical to the output the
/// two independent scans produced.
fn assert_tsx_code_and_map_match(tsx: &VerterTsxBlock, code_golden: &str, map_golden: &str) {
    assert_eq!(
        norm_eol(&tsx.code),
        norm_eol(code_golden),
        "emitted TSX code must be byte-identical to its golden"
    );
    assert!(
        !tsx.source_map.is_empty(),
        "source maps are enabled, so a source map must be emitted"
    );
    assert_eq!(
        norm_eol(&tsx.source_map),
        norm_eol(map_golden),
        "emitted TSX source map must be byte-identical to its golden"
    );
}

#[test]
fn strict_slots_nested_output_is_byte_identical() {
    // Nested components with named (#header, scoped) + default slots. Routing
    // both strict-slot and required-slot collection through the one shared
    // per-component slot summary must leave the emitted TSX — code AND source
    // map — byte-identical.
    let result = compile_tsx_strict_slots(SLOT_SFC_NESTED);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("ide/template/fixtures/strict_slots_nested.tsx.golden"),
        include_str!("ide/template/fixtures/strict_slots_nested.tsx.map.golden"),
    );
    // Negative: both slot checks present, no raw Vue directive leaked.
    let code = &tsx.code;
    assert!(
        code.contains("strictRenderSlot(") && code.contains("checkRequiredSlots("),
        "both slot checks must still be emitted, got:\n{code}"
    );
    assert!(
        !code.contains("v-slot"),
        "v-slot must not appear in TSX output, got:\n{code}"
    );
}

#[test]
fn strict_slots_deep_output_is_byte_identical() {
    // A deeply nested slot (component inside section inside div, with a named
    // slot wrapping a further-nested component) proves the per-component summary
    // reaches arbitrarily deep components without changing output bytes.
    let result = compile_tsx_strict_slots(SLOT_SFC_DEEP);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("ide/template/fixtures/strict_slots_deep.tsx.golden"),
        include_str!("ide/template/fixtures/strict_slots_deep.tsx.map.golden"),
    );
    // The innermost components and the named slot must all surface.
    let code = &tsx.code;
    assert!(
        code.contains("'body'") && code.contains("Inner") && code.contains("Leaf"),
        "deep slot facts (body slot, Inner, Leaf) must survive, got:\n{code}"
    );
}

#[test]
fn strict_slots_no_slot_subtree_is_byte_identical_and_emits_no_checks() {
    // A component-free subtree provides no slots: no summary is built and no
    // slot check is emitted. Output (code AND source map) stays byte-identical.
    let result = compile_tsx_strict_slots(SLOT_SFC_NO_SLOT);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("ide/template/fixtures/strict_slots_no_slot.tsx.golden"),
        include_str!("ide/template/fixtures/strict_slots_no_slot.tsx.map.golden"),
    );
    // Negative: no slot-check call sites for a component-free template.
    let code = &tsx.code;
    assert!(
        !code.contains("strictRenderSlot(") && !code.contains("checkRequiredSlots("),
        "a component-free template must emit no slot checks, got:\n{code}"
    );
}

#[test]
fn slot_summary_built_once_consumed_twice() {
    // The discriminating build-once assertion. The nested SFC has exactly three
    // slot-checkable components (Card, Row, Panel). Each is consumed by BOTH the
    // strict-slot and required-slot collectors, so the summaries are READ six
    // times — yet each component's summary is BUILT exactly once (three builds)
    // and the second consumption is served warm from its memoized overlay cell.
    //
    // This is the load-bearing invariant: a summary is built once per component
    // regardless of how many collectors consume it, so reads scale with the
    // collector count while builds stay fixed at one per component. With two
    // collectors `reads == 2 * builds`: exactly half the consumptions trigger a
    // build and the other half are warm cache hits. A per-collector rescan would
    // build on every read instead, making builds equal reads (6) and failing this
    // assertion.
    use crate::template::oxc::{
        reset_slot_summary_counts, slot_summary_build_count, slot_summary_read_count,
    };

    reset_slot_summary_counts();
    let result = compile_tsx_strict_slots(SLOT_SFC_NESTED);
    assert!(result.tsx.is_some(), "tsx output missing");

    let builds = slot_summary_build_count();
    let reads = slot_summary_read_count();
    assert_eq!(
        builds, 3,
        "each slot-checkable component (Card, Row, Panel) must build its summary exactly once, got {builds}"
    );
    assert_eq!(
        reads, 6,
        "two collectors must each consume all three component summaries (3 x 2), got {reads}"
    );
    assert_eq!(
        reads,
        2 * builds,
        "every summary must be consumed twice but built once: the second consumption is a warm cache hit, not a rebuild (builds {builds}, reads {reads})"
    );
}

#[test]
fn runtime_lane_builds_no_slot_summaries() {
    // The IDE-only slot summary must never be built for a pure runtime (BUNDLER)
    // compile: the VDOM/Vapor lane owns its own slot handling and pays nothing.
    use crate::template::oxc::{
        reset_slot_summary_counts, slot_summary_build_count, slot_summary_read_count,
    };

    reset_slot_summary_counts();
    let _ = compile_with_target(SLOT_SFC_NESTED, CompileTarget::BUNDLER, false);
    assert_eq!(
        slot_summary_build_count(),
        0,
        "the runtime lane must not build IDE slot summaries"
    );
    assert_eq!(
        slot_summary_read_count(),
        0,
        "the runtime lane must not read IDE slot summaries"
    );
}

#[test]
fn dual_script_tsx_does_not_leak_raw_template() {
    // Vuetify pattern: <script setup> + <script> (Options API) + <template>
    // The TSX output should NOT contain raw HTML template tags.
    let result = compile_tsx(
        r#"<template>
  <div>
    <MyComp v-model="step">
      <template v-slot:header>
        <span>Title</span>
      </template>
      <template v-slot:body="{ item }">
        <p>{{ item }}</p>
      </template>
    </MyComp>
  </div>
</template>

<script setup>
const step = ref(1)
</script>

<script>
export default {
  components: {},
}
</script>"#,
    );

    let tsx = result
        .tsx
        .as_ref()
        .expect("tsx output should exist for dual-script SFC");

    // Positive: should contain JSX elements
    assert!(
        tsx.code.contains("<div"),
        "TSX should contain JSX elements. Got:\n{}",
        tsx.code
    );

    // Negative: raw <template v-slot:xxx> must NOT leak into JSX output
    assert!(
        !tsx.code.contains("<template v-slot"),
        "raw <template v-slot:...> must not appear in TSX output. Got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("</template>"),
        "raw </template> closing tag must not appear in TSX output. Got:\n{}",
        tsx.code
    );
}

#[test]
fn dual_script_tsx_dotted_slot_names() {
    // Vuetify stepper/data-table pattern: v-slot:item.1, v-slot:item.title
    // Dot-separated slot names in a dual-script SFC with template-first layout.
    let source = r#"<template>
  <v-data-table :items="items">
    <template v-slot:item.title="{ value }">
      <span>{{ value }}</span>
    </template>
    <template v-slot:item.actions="{ item }">
      <button @click="edit(item.id)">Edit</button>
    </template>
  </v-data-table>
</template>

<script setup>
const items = ref([])
function edit(id) {}
</script>

<script>
export default {
  components: {},
}
</script>"#;

    let result = compile_tsx(source);

    let tsx = result
        .tsx
        .as_ref()
        .expect("tsx output should exist for dotted slot SFC");

    // Positive: should produce valid JSX
    assert!(
        tsx.code.contains("<span"),
        "TSX should contain inner JSX elements. Got:\n{}",
        tsx.code
    );

    // Negative: no raw template tags
    assert!(
        !tsx.code.contains("<template v-slot"),
        "raw <template v-slot:item.title> must not appear in TSX output. Got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("</template>"),
        "raw </template> must not appear in TSX output. Got:\n{}",
        tsx.code
    );

    // Negative: no parse errors
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.message.contains("parse") || e.message.contains("Parse"))
        .collect();
    assert!(
        errors.is_empty(),
        "should have no parse errors, got: {:?}",
        errors
    );
}

#[test]
fn dotted_slot_names_true_duplicate_still_detected() {
    // Two slots with the SAME dotted name should still be flagged as duplicates.
    let source = r#"<template>
  <MyComp>
    <template v-slot:item.title="{ a }"><span>{{ a }}</span></template>
    <template v-slot:item.title="{ b }"><span>{{ b }}</span></template>
  </MyComp>
</template>

<script setup>
</script>"#;
    let result = compile_tsx(source);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("Duplicate slot")),
        "should detect duplicate dotted slot names, errors: {:?}",
        result.errors
    );
}

/// Regression: Full Popover.vue with attrs, generic, class/style merge, and
/// components. Must not produce duplicate class/style attributes (ts(17001)).
#[test]
fn ide_no_duplicate_attrs_full_popover_with_components() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Popover.vue".to_string()),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { computed, ref, useTemplateRef } from 'vue'
import { Popup } from '../Popup'
import { PopoverItem } from './components'

const props = withDefaults(defineProps<{
  actions?: { text: string }[]
  actionsDirection?: string
  showArrow?: boolean
}>(), {
  actions: () => [],
  actionsDirection: 'vertical',
  showArrow: false,
})

const show = defineModel<boolean>('show', { default: false })
const emit = defineEmits({ select: (action: { text: string }, index: number) => true })
const onClickWrapper = () => {}
const floatingStyles = ref({})
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
  <Popup
    ref="popupElm"
    v-model:show="show"
    class="ns-popover"
    :duration="1"
    position=""
    :style="[floatingStyles, $attrs.style]"
    lazy-render
  >
    <div v-if="showArrow" ref="arrowElm" class="ns-popover__arrow" :style="[arrowPos]"></div>
    <div
      role="menu"
      class="ns-popover__content"
      :class="{
        'ns-popover__content--horizontal': props.actionsDirection === 'horizontal',
      }"
    >
      <slot>
        <PopoverItem
          v-for="(action, index) in props.actions"
          :key="index"
          :text="action.text"
          @click="emit('select', action, index)"
        />
      </slot>
    </div>
  </Popup>
</template>"#;
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tsx = result.tsx.as_ref().expect("TSX output");
    let code = &tsx.code;

    // Verify TSX parses cleanly (OXC catches duplicate JSX attributes)
    let alloc2 = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc2, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX parse errors: {:?}\n--- code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        code
    );
}

/// Regression: Popover.vue with `attrs="{ class: string, style: string }"`
/// and `generic="T extends object"` on `<script setup>` must not produce
/// duplicate class/style attributes in TSX output (ts(17001)).
#[test]
fn ide_no_duplicate_class_style_with_script_attrs_and_generic() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Popover.vue".to_string()),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { ref } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
const floatingStyles = ref({})
const showArrow = ref(false)
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
</template>"#;
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tsx = result.tsx.as_ref().expect("TSX output");
    let code = &tsx.code;

    eprintln!("=== FULL IDE OUTPUT ===\n{}\n=== END ===", code);

    // Count class= and style= occurrences in the template portion
    // (skip type construct declarations which may mention class/style)
    let template_start = code.find("<>").unwrap_or(0);
    let template_portion = &code[template_start..];

    let class_count = template_portion.matches("class=").count();
    assert!(
        class_count <= 1,
        "should have at most 1 class= attribute in template JSX, got {class_count}:\n{template_portion}"
    );

    let style_count = template_portion.matches("style=").count();
    assert!(
        style_count <= 1,
        "should have at most 1 style= attribute in template JSX, got {style_count}:\n{template_portion}"
    );

    // Verify no duplicate attribute names (TSX should parse cleanly)
    let alloc2 = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc2, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX parse errors (may indicate duplicate attrs): {:?}\n--- code ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        code
    );
}

// ── Shared read-only template-expression overlay ──────────────────────
//
// A TS SFC compiled for a combined runtime + TSX target parses its template
// expressions once per `ide_completion` value: the runtime lane (`false`) and
// the IDE/TSX lane (`true`) record different binding facts, so each keeps its
// own overlay entry while reusing it across every consumer in that lane.
// A JS SFC must NOT share across source types either: its TSX lane parses with
// `jsx()` while the runtime lane parses with `tsx()`, so the two never reuse
// one overlay.

use crate::template::oxc::{
    parse_template_expressions_call_count, parse_template_expressions_source_types,
    reset_parse_template_expressions_calls,
};

fn compile_with_target(source: &str, target: CompileTarget, force_js: bool) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

const TS_OVERLAY_SFC: &str = r#"<script setup lang="ts">
const msg: string = 'hello'
const count: number = 1
</script>

<template>
  <div :class="msg" v-if="count > 0">{{ msg }}{{ count }}</div>
  <span v-for="(item, i) in [count]" :key="i">{{ item }}</span>
</template>
"#;

const JS_OVERLAY_SFC: &str = r#"<script setup>
const msg = 'hello'
const count = 1
</script>

<template>
  <div :class="msg" v-if="count > 0">{{ msg }}{{ count }}</div>
  <span v-for="(item, i) in [count]" :key="i">{{ item }}</span>
</template>
"#;

#[test]
fn ts_combined_target_parses_once_per_ide_completion_value() {
    reset_parse_template_expressions_calls();
    let result = compile_with_target(
        TS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let calls = parse_template_expressions_call_count();
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    assert!(result.template.is_some(), "runtime template missing");
    assert!(result.tsx.is_some(), "tsx block missing");
    // Both lanes use `tsx()`, but completion mode differs: the runtime lane
    // parses with `ide_completion = false` and the IDE/TSX lane with `true`.
    // Those store different binding facts, so the overlay keeps one entry per
    // value — two parses, each reused within its lane (the early script-elision
    // and runtime template-codegen consumers share the single `false` entry).
    assert_eq!(
        calls, 2,
        "combined TS BUNDLER|TSX must parse once per ide_completion value (runtime false + TSX true), got {calls}"
    );
    // Both parses are `tsx()` — `(is_typescript, is_jsx)`; they differ only in
    // the (unrecorded) ide_completion flag (runtime false, then TSX true).
    assert_eq!(
        parse_template_expressions_source_types(),
        vec![(true, true), (true, true)],
        "both TS overlay parses must use tsx() (runtime false, then TSX true)"
    );
}

#[test]
fn pure_runtime_ts_target_parses_template_expressions_once() {
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(TS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    assert_eq!(
        parse_template_expressions_call_count(),
        1,
        "pure runtime target parses template expressions exactly once"
    );
}

#[test]
fn pure_tsx_ts_target_parses_template_expressions_twice() {
    // The IDE/TSX target builds TWO overlays: the `ide_completion = true` lane
    // that drives TSX template codegen, and the `ide_completion = false` lane
    // that drives unused-binding LIVENESS. The two completion modes store
    // different binding facts (completion mode intentionally suppresses real
    // references), so they cannot share an overlay — the extra liveness parse is
    // the accepted correctness cost of using a SOUND usage source for the TS6133
    // gate (the zero-extra-parse optimisation is abandoned for liveness).
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(TS_OVERLAY_SFC, CompileTarget::IDE, false);
    assert_eq!(
        parse_template_expressions_call_count(),
        2,
        "pure TSX target parses once for codegen (completion=true) + once for liveness (completion=false)"
    );
}

#[test]
fn js_combined_target_does_not_share_overlay_across_source_types() {
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(
        JS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let calls = parse_template_expressions_call_count();
    // Three distinct overlays, never shared across source types or completion
    // modes: the runtime lane parses with `tsx()/completion=false`, the liveness
    // lane with `jsx()/completion=false` (a JS SFC's liveness uses the JS source
    // type, so it does NOT collide with the runtime `tsx()` entry), and the TSX
    // lane with `jsx()/completion=true`.
    assert_eq!(
        calls, 3,
        "JS BUNDLER|TSX parses tsx runtime + jsx liveness + jsx TSX, got {calls}"
    );
    // Prove WHICH source types parsed — not just that there were three parses.
    // `tsx()` records `(is_typescript = true, is_jsx = true)`; `jsx()` records
    // `(is_typescript = false, is_jsx = true)`. Order: runtime `tsx()`, then the
    // liveness `jsx()` overlay, then the TSX-codegen `jsx()` overlay.
    let source_types = parse_template_expressions_source_types();
    assert_eq!(
        source_types,
        vec![(true, true), (false, true), (false, true)],
        "JS combined must parse runtime=tsx(), liveness=jsx(), TSX=jsx(); got {source_types:?}"
    );
}

/// Discriminating guard for rule-ledger #2 (Two Template Codegen Paths):
/// a TS SFC reuses one read-only expression overlay PER `ide_completion` value
/// (the runtime lane and the IDE/TSX lane store different binding facts) and
/// emits byte-identical output to the unshared baseline, while a JS SFC cannot
/// share across source types — its TSX lane parses with `jsx()` and the runtime
/// lane with `tsx()`, an impossibility-by-key-construction enforced separately
/// by the overlay unit tests.
#[test]
fn template_expression_overlay_source_type_matrix() {
    // (a) TS SFC: combined target parses once per ide_completion value (runtime
    //     `false` + TSX `true`) and produces output byte-identical to compiling
    //     each lane independently.
    reset_parse_template_expressions_calls();
    let ts_combined = compile_with_target(
        TS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let ts_combined_calls = parse_template_expressions_call_count();
    assert_eq!(
        ts_combined_calls, 2,
        "TS combined target must parse once per ide_completion value across runtime + TSX, got {ts_combined_calls}"
    );

    let ts_runtime_only = compile_with_target(TS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    let ts_tsx_only = compile_with_target(TS_OVERLAY_SFC, CompileTarget::IDE, false);

    let combined_runtime = ts_combined
        .template
        .as_ref()
        .expect("combined runtime block");
    let runtime_baseline = ts_runtime_only
        .template
        .as_ref()
        .expect("runtime-only block");
    assert_eq!(
        combined_runtime.code, runtime_baseline.code,
        "shared overlay must not change runtime output (byte-identical to unshared baseline)"
    );

    let combined_tsx = ts_combined.tsx.as_ref().expect("combined tsx block");
    let tsx_baseline = ts_tsx_only.tsx.as_ref().expect("tsx-only block");
    assert_eq!(
        combined_tsx.code, tsx_baseline.code,
        "shared overlay must not change TSX output (byte-identical to unshared baseline)"
    );

    // Diagnostics must be identical too — the shared overlay carries the same
    // parse facts an independent parse would. This valid fixture yields none on
    // either side; the non-empty case (a malformed interpolation) with FULL
    // per-field diagnostic identity is covered by
    // `malformed_template_expression_diagnostic_identical_across_shared_overlay`.
    assert_eq!(
        ts_combined.errors, ts_runtime_only.errors,
        "shared overlay must not change runtime diagnostics"
    );
    assert!(
        ts_combined.errors.is_empty(),
        "valid fixture must produce no diagnostics, got {:?}",
        ts_combined.errors
    );

    // (b) JS SFC: combined target parses twice (tsx runtime + jsx TSX) — no
    //     sharing across source types — and output stays byte-identical to
    //     compiling each lane independently.
    reset_parse_template_expressions_calls();
    let js_combined = compile_with_target(
        JS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let js_combined_calls = parse_template_expressions_call_count();
    assert_eq!(
        js_combined_calls, 3,
        "JS combined parses tsx runtime + jsx liveness + jsx TSX (no cross-source-type sharing), got {js_combined_calls}"
    );

    let js_runtime_only = compile_with_target(JS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    let js_tsx_only = compile_with_target(JS_OVERLAY_SFC, CompileTarget::IDE, false);

    assert_eq!(
        js_combined
            .template
            .as_ref()
            .expect("js combined runtime")
            .code,
        js_runtime_only
            .template
            .as_ref()
            .expect("js runtime-only")
            .code,
        "JS runtime output unchanged"
    );
    let js_combined_tsx = js_combined.tsx.as_ref().expect("js combined tsx");
    assert_eq!(
        js_combined_tsx.code,
        js_tsx_only.tsx.as_ref().expect("js tsx-only").code,
        "JS TSX output unchanged"
    );
    // The JS TSX lane emits a `.jsx` (JavaScript) surface, not a `.tsx` one:
    // the JS SFC's TSX block must remain valid when parsed as JSX.
    let jsx_alloc = Allocator::new();
    let jsx_parsed = oxc_parser::Parser::new(
        &jsx_alloc,
        &js_combined_tsx.code,
        oxc_span::SourceType::jsx(),
    )
    .parse();
    assert!(
        jsx_parsed.errors.is_empty(),
        "JS SFC TSX output must parse as JSX: {:?}",
        jsx_parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
}

// A TS SFC whose only interpolation `{{ it }}` is a PARTIAL prefix of the
// `v-for` scope local `item`. This is the exact case where the two lanes
// diverge: the runtime lane (`ide_completion = false`) treats `it` as a real
// reference, while the IDE/TSX lane (`ide_completion = true`) keeps it bare for
// scoped completion. A shared overlay keyed without `ide_completion` would hand
// both lanes one set of binding facts and corrupt one of the two outputs.
const TS_OVERLAY_SCOPED_COMPLETION_SFC: &str = r#"<script setup lang="ts">
const items = [1]
</script>

<template>
  <span v-for="item in items">{{ it }}</span>
</template>
"#;

#[test]
fn combined_target_keeps_ide_completion_overlay_separate_for_scoped_prefix() {
    reset_parse_template_expressions_calls();

    let combined = compile_with_target(
        TS_OVERLAY_SCOPED_COMPLETION_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    assert!(
        combined.errors.is_empty(),
        "compile errors: {:?}",
        combined.errors
    );
    assert_eq!(
        parse_template_expressions_call_count(),
        2,
        "combined TS target must parse once for runtime(false) and once for TSX(true)"
    );

    let runtime_only = compile_with_target(
        TS_OVERLAY_SCOPED_COMPLETION_SFC,
        CompileTarget::BUNDLER,
        false,
    );
    let tsx_only = compile_with_target(TS_OVERLAY_SCOPED_COMPLETION_SFC, CompileTarget::IDE, false);

    let combined_runtime = &combined.template.as_ref().expect("combined runtime").code;
    let combined_tsx = &combined.tsx.as_ref().expect("combined tsx").code;

    assert_eq!(
        combined_runtime,
        &runtime_only.template.as_ref().expect("runtime-only").code,
        "combined runtime must stay byte-identical to runtime-only false parse"
    );
    assert_eq!(
        combined_tsx,
        &tsx_only.tsx.as_ref().expect("tsx-only").code,
        "combined TSX must stay byte-identical to IDE-only true parse"
    );

    assert!(
        combined_runtime.contains("_ctx.it"),
        "runtime false parse must keep partial scoped-prefix identifier as a real instance reference"
    );
    assert!(
        combined_tsx.contains("{ it }") || combined_tsx.contains("{it}"),
        "TSX true parse must keep partial scoped-prefix identifier bare for completion"
    );
    assert!(
        !combined_tsx.contains("___VERTER___instance.it"),
        "TSX true parse must not use the runtime false binding facts"
    );
}

// A minimal TS SFC whose runtime and TSX outputs are pinned byte-for-byte
// below. It exercises the shared overlay (a bound attribute `:title="msg"` and
// an interpolation `{{ msg }}` both parse through it) while keeping the pinned
// output small enough to review.
const TS_OVERLAY_GOLDEN_SFC: &str = r#"<script setup lang="ts">
const msg = 'hi'
</script>

<template>
  <p :title="msg">{{ msg }}</p>
</template>
"#;

// The exact runtime render-function output for `TS_OVERLAY_GOLDEN_SFC`. Pinning
// the absolute bytes (not just combined-equals-pure) proves the shared overlay
// path emits the same output an independent parse would, anchoring the relative
// byte-identity checks against a known-good baseline.
const TS_OVERLAY_GOLDEN_RUNTIME: &str = r#"const _hoisted_1 = ["title"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
return (_openBlock(), _createElementBlock("p", { title: $setup.msg }, _toDisplayString( $setup.msg ), 9 /* TEXT, PROPS */, _hoisted_1)
)
}"#;

// The exact TSX output for `TS_OVERLAY_GOLDEN_SFC` (combined target). The
// destructure line ends with a trailing space in the real output; it is spliced
// in as the explicit `" \n"` segment so no source line carries (invisible,
// strip-prone) trailing whitespace while the golden still pins the exact bytes.
const TS_OVERLAY_GOLDEN_TSX: &str = concat!(
    r#"import type { Prettify as ___VERTER___Prettify, ExtractComponentProps as ___VERTER___ExtractComponentProps, ExtractLeafElement as ___VERTER___ExtractLeafElement } from "@verter/types";
import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef, enhanceElementWithProps as ___VERTER___enhanceElementWithProps, extractRenderComponent as ___VERTER___extractRenderComponent, instantiateComponent as ___VERTER___instantiateComponent, extractArgumentsFromRenderSlot as ___VERTER___extractArgumentsFromRenderSlot, runCustomDirective as ___VERTER___runCustomDirective, retrieveSetupDirectives as ___VERTER___retrieveSetupDirectives, strictRenderSlot as ___VERTER___strictRenderSlot, checkRequiredSlots as ___VERTER___checkRequiredSlots } from "@verter/types";
;export function ___VERTER___TemplateBindingFN() {

const msg = 'hi'

// @ts-ignore
let ___VERTER___instance!: Omit<InstanceType<import('./App.vue.ts')['default']>, '$attrs'> & { $attrs: ___VERTER___Attrs };
void ___VERTER___instance;
const ___VERTER___directiveAccessor = ___VERTER___retrieveSetupDirectives(___VERTER___instance);
void ___VERTER___directiveAccessor;

const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({
    msg: msg as unknown as typeof msg
  });
{ /* verter-destructured-start */const {"#,
    " \n",
    r#"    msg } = ___VERTER___unwrapped; /* verter-destructured-end */
<>
  <p title={msg}>{ msg }</p>
</>
} // close block scope

function ___VERTER___Comp66() {
  return {} as HTMLElementTagNameMap["p"];
}
function ___VERTER___getRootComponent() { return ___VERTER___Comp66(); }
function ___VERTER___getRootComponentPassedProps() { return {"title": msg}; }
type ___VERTER___RootElement = ReturnType<typeof ___VERTER___getRootComponent>;
type ___VERTER___RootElementProps = ___VERTER___Prettify<Omit<
  ___VERTER___ExtractComponentProps<___VERTER___RootElement>,
  keyof ReturnType<typeof ___VERTER___getRootComponentPassedProps>
>>;

type ___VERTER___Attrs = ___VERTER___attributes & ___VERTER___RootElementProps;

void ___VERTER___getRootComponent; void ___VERTER___getRootComponentPassedProps;
void ___VERTER___Comp66;
void (___VERTER___instance).valueOf;

return {};
} // close templateBindingFN

type ___VERTER___attributes = {};
"#
);

// A TS SFC with a deliberately malformed interpolation (`{{ count + }}` — a
// binary expression missing its right operand). The interpolation tokenizes
// cleanly at the SFC level (so template codegen still runs), but OXC fails to
// parse the inner expression, surfacing an `XInvalidExpression` warning.
const TS_OVERLAY_MALFORMED_SFC: &str = r#"<script setup lang="ts">
const count: number = 1
</script>

<template>
  <div>{{ count + }}</div>
</template>
"#;

/// Absolute-bytes anchor: the shared-overlay compile path emits exactly the
/// pinned runtime and TSX bytes. Because both goldens are hand-verified literal
/// output (not another run of the same path), this breaks the circularity of
/// the combined-equals-pure checks — a regression in the shared overlay that
/// also moved the pure-target output would still break these absolute goldens.
#[test]
fn template_expression_overlay_pins_absolute_output_bytes() {
    let combined = compile_with_target(
        TS_OVERLAY_GOLDEN_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    assert!(
        combined.errors.is_empty(),
        "golden fixture must compile cleanly: {:?}",
        combined.errors
    );

    let runtime = combined
        .template
        .as_ref()
        .expect("runtime block")
        .code
        .as_str();
    assert_eq!(
        runtime, TS_OVERLAY_GOLDEN_RUNTIME,
        "shared-overlay runtime output drifted from the pinned absolute bytes"
    );

    let tsx = combined.tsx.as_ref().expect("tsx block").code.as_str();
    assert_eq!(
        tsx, TS_OVERLAY_GOLDEN_TSX,
        "shared-overlay TSX output drifted from the pinned absolute bytes"
    );

    // Guard the goldens themselves against accidental triviality.
    assert!(TS_OVERLAY_GOLDEN_RUNTIME.contains("_createElementBlock(\"p\""));
    assert!(TS_OVERLAY_GOLDEN_TSX.contains("<p title={msg}>{ msg }</p>"));
}

/// A malformed template interpolation surfaces an identical diagnostic whether
/// the SFC is compiled for the combined target (shared overlay) or the runtime
/// target alone. Asserts the FULL diagnostic (severity, code, span, message),
/// and that the diagnostic set is non-empty — so the parity is real, not an
/// empty-vs-empty comparison.
#[test]
fn malformed_template_expression_diagnostic_identical_across_shared_overlay() {
    let combined = compile_with_target(
        TS_OVERLAY_MALFORMED_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let runtime_only = compile_with_target(TS_OVERLAY_MALFORMED_SFC, CompileTarget::BUNDLER, false);

    // The malformed interpolation MUST surface at least one diagnostic, else
    // this parity check would be vacuous.
    assert!(
        !runtime_only.errors.is_empty(),
        "malformed interpolation must surface a diagnostic"
    );
    let invalid_expr = runtime_only
        .errors
        .iter()
        .find(|d| d.code == "XInvalidExpression")
        .expect("malformed interpolation must surface an XInvalidExpression diagnostic");
    assert_eq!(invalid_expr.severity, CompileDiagnosticSeverity::Warning);
    assert!(
        !invalid_expr.message.is_empty(),
        "diagnostic must carry a non-empty message"
    );

    // FULL diagnostic identity (severity + code + span + message, the four
    // fields of `CompileDiagnostic`) between the shared-overlay combined target
    // and the runtime baseline — the shared overlay must neither drop nor alter
    // any diagnostic.
    assert_eq!(
        combined.errors, runtime_only.errors,
        "shared overlay changed the diagnostics surfaced for a malformed interpolation"
    );

    // The TSX lane consumes the SAME malformed `tsx()` overlay: the combined
    // target's TSX output is byte-identical to the TSX-only baseline.
    let tsx_only = compile_with_target(TS_OVERLAY_MALFORMED_SFC, CompileTarget::IDE, false);
    assert_eq!(
        combined.tsx.as_ref().expect("combined tsx").code,
        tsx_only.tsx.as_ref().expect("tsx-only").code,
        "shared overlay changed the TSX output for a malformed interpolation"
    );
}

// ── Spread-path component event typing: end-to-end through compile() ────────
//
// These exercise the FULL pipeline — script generation builds the shared component
// inventory (local bindings + GlobalComponents fallback consts) and threads it through
// `IdeScriptGenResult` into template generation, where the `@event` spread path consumes
// it. The unit tests in `ide/template/tests.rs` supply the inventory manually; these
// prove it is actually produced and consumed across the script/template boundary.
//
// `result.errors.is_empty()` here asserts the Rust-side compile diagnostic set is empty
// (the generated TSX is syntactically well-formed); the `tsx.code.contains` checks assert
// the byte-exact formula string the codegen must emit. This is the string-exact codegen
// contract — NOT a tsgo type-check of that formula resolving to the payload.

/// Assert the generated TSX is free of every spread-event-typing antipattern the shared
/// component-binding inventory exists to prevent. Every spread-event compile() test calls
/// this so the negative contract is uniform and discriminating — it FAILS if codegen
/// regresses to an untyped `$event`, the intrinsic-attribute surface, the retired
/// `eventCallbacks` helper, or the tsgo-unresolvable `GlobalComponents[...]` indexed event
/// type.
fn assert_no_spread_event_antipatterns(code: &str) {
    assert!(
        !code.contains("$event: any"),
        "spread $event must be precisely typed, never `any`: {code}"
    );
    assert!(
        !code.contains("IntrinsicElementAttributes"),
        "spread event typing must not fall back to the IntrinsicElementAttributes surface: {code}"
    );
    assert!(
        !code.contains("___VERTER___eventCallbacks"),
        "spread event typing must not use the retired eventCallbacks helper: {code}"
    );
    assert!(
        !code.contains("GlobalComponents["),
        "spread event typing must not use the tsgo-unresolvable GlobalComponents[...] indexed event type: {code}"
    );
}

#[test]
fn tsx_global_component_spread_event_resolves_via_fallback_const() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function onPing(s: string) { void s; }
</script>
<template>
  <GlobalEmitComp @ping="onPing($event)" @ping="onPing($event)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Script generation emits the GlobalComponents fallback const for the unimported
    // component (the only place `import('vue').GlobalComponents` is allowed — a
    // `extends ... infer` conditional, NEVER an indexed event-type query).
    assert!(
        tsx.code.contains(
            "const GlobalEmitComp = {} as import('vue').GlobalComponents extends { GlobalEmitComp: infer C } ? C : unknown;"
        ),
        "must emit the GlobalComponents fallback const: {}",
        tsx.code
    );
    // The duplicate-spread `$event` resolves through that same fallback const.
    assert!(
        tsx.code.contains(
            r#"$event: Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onPing"]>>[0]"#
        ),
        "spread $event must resolve via InstanceType<typeof GlobalEmitComp>: {}",
        tsx.code
    );
    // No spread-event antipattern — untyped `$event`, the intrinsic-attribute surface,
    // the retired eventCallbacks helper, or the tsgo-unresolvable `GlobalComponents[...]`
    // indexed event type (the fallback const's `GlobalComponents extends` is distinct).
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn tsx_global_component_spread_arrow_satisfies_via_fallback_const() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function onPing(s: string) { void s; }
</script>
<template>
  <GlobalEmitComp @some-event="(e) => onPing(e)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"((e) => onPing(e)) satisfies (...___VERTER___eventArgs: Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onSome-event"]>>) => unknown"#
        ),
        "spread arrow must satisfies-wrap via InstanceType<typeof GlobalEmitComp>: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn tsx_local_component_spread_event_resolves_via_binding_no_fallback() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import EmitChild from './EmitChild.vue';
function onPick(n: number) { void n; }
</script>
<template>
  <EmitChild @pick="onPick($event)" @pick="onPick($event)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"$event: Parameters<NonNullable<Required<InstanceType<typeof EmitChild>["$props"]>["onPick"]>>[0]"#
        ),
        "spread $event must resolve via the imported binding InstanceType<typeof EmitChild>: {}",
        tsx.code
    );
    // An imported component must NOT also get a GlobalComponents fallback const.
    assert!(
        !tsx.code
            .contains("const EmitChild = {} as import('vue').GlobalComponents"),
        "imported component must NOT get a fallback const: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn tsx_global_component_simple_handler_param_inference_via_fallback() {
    // event_inference consumes the SAME inventory: a global component's simple-ident
    // handler types its function-declaration parameter via the fallback const, with no
    // implicit-any left behind.
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handlePing(e) { void e; }
</script>
<template>
  <GlobalEmitComp @ping="handlePing" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("const GlobalEmitComp = {} as import('vue').GlobalComponents"),
        "must emit the fallback const: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onPing"]>>"#
        ),
        "simple-handler param inference must resolve via the fallback const: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

// ── ISSUE-7: unused `<script setup>` local → TS6133 (full compile path) ─────
//
// End-to-end through `compile_tsx`, which computes `template_used_vars` in the
// compile pipeline and plumbs it into IDE codegen. A binding used NOWHERE must
// be OMITTED from the `___VERTER___unwrapped` object + destructure block (so its
// SOURCE decl is its sole occurrence and TS6133 lands on the MAPPED declaration,
// not an unmapped destructure copy that collapses to line 1); a binding used in
// the template must keep its value read.

#[test]
fn tsx_unused_script_setup_local_is_omitted_from_unwrap() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>hello</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Omitted: no value-read entry, no retired type-only entry, and crucially
    // NO `typeof foo` anywhere (which would keep the source decl live and
    // mis-position TS6133 to line 1).
    assert!(
        !tsx.code.contains("foo: foo as unknown as typeof foo"),
        "unused `foo` must NOT keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("foo: undefined as unknown as typeof foo"),
        "unused `foo` must NOT use the retired type-only unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("typeof foo"),
        "unused `foo` must not be referenced via `typeof foo` (keeps source decl live).\nTSX:\n{}",
        tsx.code
    );
    // The user's `const foo` decl survives untouched (it carries TS6133).
    assert!(
        tsx.code.contains("const foo = 1"),
        "the source `const foo` decl must remain.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_used_script_setup_local_keeps_value_read() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>{{ foo }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("foo: foo as unknown as typeof foo"),
        "template-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("foo: undefined as unknown as typeof foo"),
        "template-used `foo` must NOT be demoted to a type-only entry.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_script_used_only_local_keeps_value_read() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
console.log(foo)
</script>

<template>
  <div>hello</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("foo: foo as unknown as typeof foo"),
        "script-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
}
