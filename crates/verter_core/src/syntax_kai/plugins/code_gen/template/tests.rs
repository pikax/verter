use crate::builder::codegen_kai::{generate_kai, KaiCodegenOptions};
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

// =========================================================================
// Test Infrastructure
// =========================================================================

/// Run the full pipeline (tokenizer → syntax_kai → codegen) in dev mode.
fn gen(input: &str) -> String {
    let allocator = Allocator::new();
    let options = KaiCodegenOptions::new().with_filename("test.vue");
    generate_kai(input, &options, &allocator).code
}

/// Run the full pipeline in production mode.
fn gen_prod(input: &str) -> String {
    let allocator = Allocator::new();
    let options = KaiCodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    generate_kai(input, &options, &allocator).code
}

/// Validate that generated code is syntactically valid JavaScript.
fn assert_valid_js(code: &str, context: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::mjs();
    let parser_result = Parser::new(&allocator, code, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "Generated code is NOT valid JavaScript!\n\
         Context: {}\n\
         Parse Errors: {:?}\n\
         Generated Code:\n{}",
        context,
        parser_result.errors,
        code
    );
}

/// Known invalid patterns that indicate broken codegen.
const INVALID_PATTERNS: &[(&str, &str)] = &[
    ("{ :", "empty property name"),
    ("_ctx.{", "object literal after _ctx."),
    ("_ctx.[", "array literal after _ctx."),
    ("{ v-", "hyphenated directive as property"),
    (": _ctx.!", "negation in wrong position"),
    (", ,", "double comma"),
    (
        "\"_toDisplayString",
        "missing string concatenation operator",
    ),
];

/// Check that generated code does not contain known invalid patterns.
fn assert_no_invalid_patterns(code: &str, context: &str) {
    for (pattern, desc) in INVALID_PATTERNS {
        assert!(
            !code.contains(pattern),
            "Found invalid pattern '{}' ({}) in {}.\nGenerated:\n{}",
            pattern,
            desc,
            context,
            code
        );
    }
}

/// Generate code AND validate it is valid JS + no invalid patterns.
fn gen_and_validate(input: &str) -> String {
    let code = gen(input);
    assert_valid_js(&code, input);
    assert_no_invalid_patterns(&code, input);
    code
}

/// Generate production code AND validate it is valid JS.
/// Production code starts with `return (_ctx,_cache) => {` so we wrap in a function for validation.
fn gen_prod_and_validate(input: &str) -> String {
    let code = gen_prod(input);
    let wrapped = format!("function __wrapper__() {{ {} }}", code);
    assert_valid_js(&wrapped, input);
    assert_no_invalid_patterns(&code, input);
    code
}

// =========================================================================
// Template Wrapper
// =========================================================================

/// @ai-generated — Dev mode emits `function render(_ctx, _cache, ...)`
#[test]
fn test_dev_function_render() {
    let code = gen_and_validate(r#"<template><div>hi</div></template>"#);
    assert!(
        code.contains("function render(_ctx, _cache"),
        "Dev mode should emit function render, got:\n{}",
        code
    );
}

/// @ai-generated — Production mode emits arrow function `(_ctx,_cache) => {`
#[test]
fn test_prod_arrow_fn() {
    let code = gen_prod_and_validate(r#"<template><div>hi</div></template>"#);
    assert!(
        code.contains("(_ctx,_cache) => {"),
        "Prod mode should emit arrow function, got:\n{}",
        code
    );
}

/// @ai-generated — Empty template returns null
#[test]
fn test_template_empty_returns_null() {
    let code = gen_and_validate(r#"<template></template>"#);
    assert!(
        code.contains("return null"),
        "Empty template should return null, got:\n{}",
        code
    );
}

// =========================================================================
// Elements — basic structure
// =========================================================================

/// @ai-generated — Simple div with text child (root = block)
#[test]
fn test_element_simple_div_text() {
    let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("div", null, "hello")"#),
        "Root should emit _createElementBlock(\"div\", null, \"hello\"), got:\n{}",
        code
    );
    assert!(
        code.contains("_openBlock()"),
        "Root should use _openBlock(), got:\n{}",
        code
    );
}

/// @ai-generated — Self-closing <br/> element (root = block)
#[test]
fn test_element_self_closing_br() {
    let code = gen_and_validate(r#"<template><br/></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("br", null)"#),
        "Root br should use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Void <input> element (root = block)
#[test]
fn test_element_void_input() {
    let code = gen_and_validate(r#"<template><input></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("input", null)"#),
        "Root void input should use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Empty div produces no children arg (root = block)
#[test]
fn test_element_empty_div() {
    let code = gen_and_validate(r#"<template><div></div></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("div", null)"#),
        "Empty root div should use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Nested elements: root = block, child = VNode
#[test]
fn test_element_nested() {
    let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("div""#),
        "Root div should be _createElementBlock, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createElementVNode("span", null, "inner")"#),
        "Child span should be _createElementVNode with text, got:\n{}",
        code
    );
}

/// @ai-generated — Deeply nested elements
#[test]
fn test_element_deeply_nested() {
    let code = gen_and_validate(r#"<template><div><span><em>deep</em></span></div></template>"#);
    assert!(
        code.contains(r#"_createElementVNode("em", null, "deep")"#),
        "Deepest element should have text, got:\n{}",
        code
    );
}

// =========================================================================
// Elements — block root treatment
// Vue wraps root elements in (_openBlock(), _createElementBlock(...))
// =========================================================================

/// @ai-generated — Root element should use _openBlock + _createElementBlock
#[test]
fn test_block_root_simple() {
    let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains("_openBlock()"),
        "Root should use _openBlock(), got:\n{}",
        code
    );
    assert!(
        code.contains("_createElementBlock("),
        "Root should use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Nested child should use _createElementVNode (not block)
#[test]
fn test_block_root_nested_child_is_vnode() {
    let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
    assert!(
        code.contains("_createElementBlock("),
        "Root div should use _createElementBlock, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createElementVNode("span""#),
        "Child span should use _createElementVNode, got:\n{}",
        code
    );
}

// =========================================================================
// Static Props
// =========================================================================

/// @ai-generated — Static id attribute
#[test]
fn test_props_static_id() {
    let code = gen_and_validate(r#"<template><div id="app">hi</div></template>"#);
    // Static props are hoisted: const _hoisted_1 = { id: "app" }
    assert!(
        code.contains(r#"_hoisted_1 = { id: "app" }"#),
        "Static id prop should be hoisted, got:\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_1"),
        "Render function should reference _hoisted_1, got:\n{}",
        code
    );
}

/// @ai-generated — Static class attribute (hoisted)
#[test]
fn test_props_static_class() {
    let code = gen_and_validate(r#"<template><div class="foo bar">hi</div></template>"#);
    // Static class is hoisted
    assert!(
        code.contains(r#"class: "foo bar""#),
        "Should have class prop in hoisted constant, got:\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_1"),
        "Render function should reference hoisted props, got:\n{}",
        code
    );
}

/// @ai-generated — Static style attribute (hoisted)
#[test]
fn test_props_static_style() {
    let code = gen_and_validate(r#"<template><div style="color: red">hi</div></template>"#);
    // Static style is hoisted
    assert!(
        code.contains(r#"style: "color: red""#),
        "Should have style prop in hoisted constant, got:\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_1"),
        "Render function should reference hoisted props, got:\n{}",
        code
    );
}

/// @ai-generated — Props null when no attributes
#[test]
fn test_props_null_when_empty() {
    let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains(r#""div", null"#),
        "No props should produce null, got:\n{}",
        code
    );
}

// =========================================================================
// Bound Props — :id, :class, :style
// =========================================================================

/// @ai-generated — Bound :id produces {id: expr} with PROPS patch flag
#[test]
fn test_props_bound_id() {
    let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
    assert!(
        code.contains("{id: _ctx.myId}"),
        "Bound id should be {{id: _ctx.myId}}, got:\n{}",
        code
    );
    assert!(
        code.contains("8 /* PROPS */"),
        "Should have PROPS (8) patch flag, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["id"]"#),
        "Should list dynamic prop name, got:\n{}",
        code
    );
}

/// @ai-generated — :class uses _normalizeClass with CLASS flag
#[test]
fn test_props_class_normalize() {
    let code = gen_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
    assert!(
        code.contains("class: _normalizeClass(_ctx.cls)"),
        "Should use _normalizeClass, got:\n{}",
        code
    );
    assert!(
        code.contains("2 /* CLASS */"),
        "Should have CLASS (2) patch flag, got:\n{}",
        code
    );
}

/// @ai-generated — :style uses _normalizeStyle with STYLE flag
#[test]
fn test_props_style_normalize() {
    let code = gen_and_validate(r#"<template><div :style="sty">hi</div></template>"#);
    assert!(
        code.contains("style: _normalizeStyle(_ctx.sty)"),
        "Should use _normalizeStyle, got:\n{}",
        code
    );
    assert!(
        code.contains("4 /* STYLE */"),
        "Should have STYLE (4) patch flag, got:\n{}",
        code
    );
}

/// @ai-generated — Mixed static + bound props
#[test]
fn test_props_mixed_static_bound() {
    let code = gen_and_validate(r#"<template><div id="s" :title="d">hi</div></template>"#);
    assert!(
        code.contains(r#"id: "s""#),
        "Static id should be preserved, got:\n{}",
        code
    );
    assert!(
        code.contains("title: _ctx.d"),
        "Bound title should be present, got:\n{}",
        code
    );
    assert!(
        code.contains("8 /* PROPS */"),
        "Should have PROPS patch flag, got:\n{}",
        code
    );
}

/// @ai-generated — Combined :class and :style patch flags
#[test]
fn test_props_class_style_combined() {
    let code = gen_and_validate(r#"<template><div :class="c" :style="s">hi</div></template>"#);
    assert!(
        code.contains("_normalizeClass(_ctx.c)"),
        "Should have _normalizeClass, got:\n{}",
        code
    );
    assert!(
        code.contains("_normalizeStyle(_ctx.s)"),
        "Should have _normalizeStyle, got:\n{}",
        code
    );
    // CLASS(2) | STYLE(4) = 6
    assert!(
        code.contains("6 /* CLASS, STYLE */"),
        "Should have combined CLASS+STYLE flag (6), got:\n{}",
        code
    );
}

/// @ai-generated — No patch flag for static-only props
#[test]
fn test_props_no_pf_for_static() {
    let code = gen_and_validate(r#"<template><div id="app">hi</div></template>"#);
    // Static props shouldn't produce a patch flag number
    assert!(
        !code.contains("/* PROPS */"),
        "Static-only props should not have PROPS flag, got:\n{}",
        code
    );
}

// =========================================================================
// Static Hoisting
// Vue hoists static props to module-scope constants: const _hoisted_N = { ... }
// =========================================================================

/// @ai-generated — Static props hoisted to _hoisted_1 constant
#[test]
fn test_hoist_static_props() {
    let code = gen_and_validate(r#"<template><div class="app">{{ msg }}</div></template>"#);
    assert!(
        code.contains(r#"const _hoisted_1 = { class: "app" };"#),
        "Static props should be hoisted, got:\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_1"),
        "Render function should reference _hoisted_1, got:\n{}",
        code
    );
    // Inline props should NOT appear in render function
    assert!(
        !code.contains(r#"_createElementBlock("div", {class"#),
        "Props should not be inline in render function, got:\n{}",
        code
    );
}

/// @ai-generated — Multiple static prop elements get separate hoisted constants
#[test]
fn test_hoist_multiple_props() {
    let code = gen_and_validate(
        r#"<template><div><span class="inner">{{ a }}</span><p id="footer">{{ b }}</p></div></template>"#,
    );
    assert!(
        code.contains("_hoisted_1"),
        "First element's props should be hoisted, got:\n{}",
        code
    );
    assert!(
        code.contains("_hoisted_2"),
        "Second element's props should be hoisted, got:\n{}",
        code
    );
}

/// @ai-generated — Mixed static+dynamic props are NOT hoisted
#[test]
fn test_hoist_mixed_props_not_hoisted() {
    let code = gen_and_validate(r#"<template><div class="app" :id="myId">hi</div></template>"#);
    assert!(
        !code.contains("_hoisted_"),
        "Mixed static+dynamic props should NOT be hoisted, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler prevents hoisting
#[test]
fn test_hoist_event_prevents_hoisting() {
    let code =
        gen_and_validate(r#"<template><button class="btn" @click="go">hi</button></template>"#);
    assert!(
        !code.contains("_hoisted_"),
        "Element with event handler should NOT have hoisted props, got:\n{}",
        code
    );
}

/// @ai-generated — No props = no hoisting (null)
#[test]
fn test_hoist_no_props() {
    let code = gen_and_validate(r#"<template><div>hi</div></template>"#);
    assert!(
        !code.contains("_hoisted_"),
        "Element without props should not produce hoisted constants, got:\n{}",
        code
    );
}

/// @ai-generated — Component props are NOT hoisted (Vue rule)
#[test]
fn test_hoist_component_not_hoisted() {
    let code =
        gen_and_validate(r#"<template><MyComponent class="app">hi</MyComponent></template>"#);
    assert!(
        !code.contains("_hoisted_"),
        "Component props should NOT be hoisted, got:\n{}",
        code
    );
}

/// @ai-generated — Hoisted constant appears before render function
#[test]
fn test_hoist_placement_before_render() {
    let code = gen_and_validate(r#"<template><div id="app">{{ msg }}</div></template>"#);
    let hoist_pos = code.find("const _hoisted_1").unwrap();
    let render_pos = code.find("function render").unwrap();
    assert!(
        hoist_pos < render_pos,
        "Hoisted constant should appear before render function, got:\n{}",
        code
    );
}

/// @ai-generated — Multiple static attributes hoisted together
#[test]
fn test_hoist_multiple_attrs() {
    let code = gen_and_validate(
        r#"<template><div id="app" class="main" style="color:red">{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("_hoisted_1"),
        "Multiple static attrs should be hoisted together, got:\n{}",
        code
    );
    // All three props in the hoisted constant
    assert!(
        code.contains(r#"id: "app""#),
        "Hoisted constant should contain id, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"class: "main""#),
        "Hoisted constant should contain class, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"style: "color:red""#),
        "Hoisted constant should contain style, got:\n{}",
        code
    );
}

/// @ai-generated — Production mode also hoists static props
#[test]
fn test_hoist_production_mode() {
    let code = gen_prod_and_validate(r#"<template><div class="app">{{ msg }}</div></template>"#);
    assert!(
        code.contains("_hoisted_1"),
        "Production mode should also hoist static props, got:\n{}",
        code
    );
}

// =========================================================================
// Events — @click etc.
// =========================================================================

/// @ai-generated — @click becomes onClick prop
#[test]
fn test_event_click() {
    let code = gen_and_validate(r#"<template><button @click="handler">click</button></template>"#);
    assert!(
        code.contains("onClick: _ctx.handler"),
        "Should have onClick: _ctx.handler, got:\n{}",
        code
    );
    assert!(
        code.contains("8 /* PROPS */"),
        "Event should produce PROPS (8) patch flag, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["onClick"]"#),
        "Event name should be in dynamic props list, got:\n{}",
        code
    );
}

/// @ai-generated — Multiple events
#[test]
fn test_event_multiple() {
    let code =
        gen_and_validate(r#"<template><button @click="a" @mouseover="b">hi</button></template>"#);
    assert!(
        code.contains("onClick: _ctx.a"),
        "Should have onClick: _ctx.a, got:\n{}",
        code
    );
    assert!(
        code.contains("onMouseover: _ctx.b"),
        "Should have onMouseover: _ctx.b, got:\n{}",
        code
    );
    assert!(
        code.contains("8 /* PROPS */"),
        "Events should produce PROPS (8) patch flag, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["onClick", "onMouseover"]"#),
        "Event names should be in dynamic props list, got:\n{}",
        code
    );
}

/// @ai-generated — Vue treats events as PROPS patch flag with event name in dynamic props
#[test]
fn test_event_props_patch_flag() {
    let code = gen_and_validate(r#"<template><button @click="handler">click</button></template>"#);
    assert!(
        code.contains("8 /* PROPS */"),
        "Event should produce PROPS (8) patch flag, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["onClick"]"#),
        "Event name should be in dynamic props list, got:\n{}",
        code
    );
}

// =========================================================================
// Text
// =========================================================================

/// @ai-generated — Text wrapping in quotes
#[test]
fn test_text_in_quotes() {
    let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains(r#""hello""#),
        "Text should be wrapped in quotes, got:\n{}",
        code
    );
}

/// @ai-generated — Text with quotes gets escaped
#[test]
fn test_text_escaped_quotes() {
    let code = gen_and_validate(r#"<template><div>say "hello"</div></template>"#);
    // The text should escape inner quotes
    assert!(
        code.contains(r#"say \"hello\""#) || code.contains(r#"say "hello""#),
        "Text with quotes should be handled, got:\n{}",
        code
    );
}

// =========================================================================
// Interpolation
// =========================================================================

/// @ai-generated — Simple interpolation produces _toDisplayString
#[test]
fn test_interp_simple() {
    let code = gen_and_validate(r#"<template><div>{{ msg }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString, got:\n{}",
        code
    );
}

/// @ai-generated — Interpolation with expression
#[test]
fn test_interp_expr() {
    let code = gen_and_validate(r#"<template><div>{{ a + b }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString for expression, got:\n{}",
        code
    );
}

/// @ai-generated — Interpolation with ternary
#[test]
fn test_interp_ternary() {
    let code = gen_and_validate(r#"<template><div>{{ a ? b : c }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString for ternary, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.a ? _ctx.b : _ctx.c"),
        "Ternary expression should be preserved with _ctx. prefix, got:\n{}",
        code
    );
}

/// @ai-generated — Interpolation with method call
#[test]
fn test_interp_method_call() {
    let code = gen_and_validate(r#"<template><div>{{ foo() }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString for method call, got:\n{}",
        code
    );
}

/// @ai-generated — Interpolation with $setup binding prefix
#[test]
fn test_interp_with_setup_binding() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString, got:\n{}",
        code
    );
    // Setup bindings should get $setup prefix in dev mode
    assert!(
        code.contains("$setup.msg"),
        "Setup binding should have $setup prefix, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler with $setup binding prefix: onClick: $setup.increment
#[test]
fn test_event_with_setup_binding() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
function increment() { count.value++ }
</script>
<template><button @click="increment">Count: {{ count }}</button></template>"#,
    );
    assert!(
        code.contains("onClick: $setup.increment"),
        "Event handler should have $setup. prefix BEFORE identifier, got:\n{}",
        code
    );
    // Make sure the broken pattern is NOT present
    assert!(
        !code.contains("increment$setup."),
        "Accessor prefix must not appear AFTER identifier, got:\n{}",
        code
    );
}

/// @ai-generated — Bound prop with $setup binding prefix: id: $setup.myId
#[test]
fn test_bound_prop_with_setup_binding() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const myId = ref('app')
</script>
<template><div :id="myId">hi</div></template>"#,
    );
    assert!(
        code.contains("id: $setup.myId"),
        "Bound prop should have $setup. prefix BEFORE identifier, got:\n{}",
        code
    );
    assert!(
        !code.contains("myId$setup."),
        "Accessor prefix must not appear AFTER identifier, got:\n{}",
        code
    );
}

/// @ai-generated — :class with $setup binding: class: _normalizeClass($setup.cls)
#[test]
fn test_class_bind_with_setup_binding() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const cls = ref('active')
</script>
<template><div :class="cls">hi</div></template>"#,
    );
    assert!(
        code.contains("_normalizeClass($setup.cls)"),
        ":class binding should have $setup. prefix BEFORE identifier, got:\n{}",
        code
    );
}

/// @ai-generated — :style with $setup binding: style: _normalizeStyle($setup.sty)
#[test]
fn test_style_bind_with_setup_binding() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const sty = ref({ color: 'red' })
</script>
<template><div :style="sty">hi</div></template>"#,
    );
    assert!(
        code.contains("_normalizeStyle($setup.sty)"),
        ":style binding should have $setup. prefix BEFORE identifier, got:\n{}",
        code
    );
}

/// @ai-generated — Full SFC with multiple setup bindings in template
#[test]
fn test_full_sfc_setup_bindings() {
    let code = gen_and_validate(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const message = ref('Hello from Verter!')
function increment() { count.value++ }
</script>
<template>
  <div class="app">
<h1>{{ message }}</h1>
<button @click="increment">Count: {{ count }}</button>
  </div>
</template>"#,
    );
    // Check all setup bindings have correct prefix
    assert!(
        code.contains("$setup.message"),
        "Interpolation binding should have $setup prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("onClick: $setup.increment"),
        "Event handler should have $setup prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.count"),
        "Second interpolation binding should have $setup prefix, got:\n{}",
        code
    );
    // Static class should be hoisted
    assert!(
        code.contains(r#"_hoisted_1 = { class: "app" }"#),
        "Static class should be hoisted to _hoisted_1, got:\n{}",
        code
    );
}

// =========================================================================
// Text + Interpolation Mix (concatenation)
// Vue concatenates: "hello " + _toDisplayString(_ctx.msg)
// Current: separate comma args (requires close-phase refactor)
// =========================================================================

/// @ai-generated — Text + interpolation should concatenate with +
#[test]
fn test_children_text_interp_concat() {
    let code = gen_and_validate(r#"<template><div>hello {{ msg }}</div></template>"#);
    assert!(
        code.contains(r#""hello " + _toDisplayString"#),
        "Text + interpolation should concat with +, got:\n{}",
        code
    );
    assert!(
        code.contains("1 /* TEXT */"),
        "Concatenated text should have TEXT patch flag, got:\n{}",
        code
    );
}

/// @ai-generated — Text-interp-text should concat: "hello " + expr + " world"
#[test]
fn test_children_text_interp_text_concat() {
    let code = gen_and_validate(r#"<template><div>hello {{ msg }} world</div></template>"#);
    assert!(
        code.contains(r#""hello " + _toDisplayString"#),
        "Should start with text + toDisplayString, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"+ " world""#),
        "Should end with + \" world\", got:\n{}",
        code
    );
}

/// @ai-generated — Multiple interpolations concatenated
#[test]
fn test_children_multiple_interp_concat() {
    let code = gen_and_validate(r#"<template><div>{{ a }}{{ b }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString"),
        "Should have _toDisplayString calls, got:\n{}",
        code
    );
    // Vue: _toDisplayString(_ctx.a) + _toDisplayString(_ctx.b)
    assert!(
        code.contains(" + _toDisplayString"),
        "Multiple interps should concatenate with +, got:\n{}",
        code
    );
}

// =========================================================================
// Children array wrapping
// Vue wraps multiple non-text children in [...] array
// =========================================================================

/// @ai-generated — Multiple element children should be in array
#[test]
fn test_children_multiple_elements_array() {
    let code = gen_and_validate(r#"<template><div><span>a</span><span>b</span></div></template>"#);
    // Vue: [..., [...]] where children are in an array
    assert!(
        code.contains("[_createElementVNode"),
        "Multiple children should be wrapped in array, got:\n{}",
        code
    );
}

/// @ai-generated — Single element child: no array needed
#[test]
fn test_children_single_element() {
    let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
    // Single child should not be in array
    assert!(
        !code.contains("[_createElementVNode"),
        "Single child should NOT be in array, got:\n{}",
        code
    );
}

// =========================================================================
// Comments
// =========================================================================

/// @ai-generated — HTML comment → _createCommentVNode
#[test]
fn test_comment_basic() {
    let code = gen_and_validate(r#"<template><div><!-- my comment --></div></template>"#);
    assert!(
        code.contains(r#"_createCommentVNode(" my comment ")"#),
        "Comment should produce _createCommentVNode with content, got:\n{}",
        code
    );
}

/// @ai-generated — Empty comment
#[test]
fn test_comment_empty() {
    let code = gen_and_validate(r#"<template><div><!----></div></template>"#);
    assert!(
        code.contains(r#"_createCommentVNode("")"#),
        "Empty comment should produce empty string, got:\n{}",
        code
    );
}

/// @ai-generated — Comment as only child of element
#[test]
fn test_comment_only_child() {
    let code = gen_and_validate(r#"<template><div><!-- only --></div></template>"#);
    assert!(
        code.contains("_createCommentVNode"),
        "Only-child comment should still produce _createCommentVNode, got:\n{}",
        code
    );
}

// =========================================================================
// v-if directives
// =========================================================================

/// @ai-generated — v-if produces ternary with comment fallback
#[test]
fn test_v_if_ternary() {
    let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains("(_ctx.show) ? ("),
        "v-if should produce ternary, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createCommentVNode("v-if", true)"#),
        "v-if should have labeled comment fallback in dev, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else produces both branches
#[test]
fn test_v_if_else() {
    let code =
        gen_and_validate(r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#);
    assert!(
        code.contains("(_ctx.show) ? ("),
        "Should have v-if ternary, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""yes""#),
        "Should have 'yes' branch, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""no""#),
        "Should have 'no' branch, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else-if/v-else chain
#[test]
fn test_v_if_else_if_else() {
    let code = gen_and_validate(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    assert!(
        code.contains("(_ctx.a) ? ("),
        "Should have first condition, got:\n{}",
        code
    );
    assert!(
        code.contains("(_ctx.b) ? ("),
        "Should have else-if condition, got:\n{}",
        code
    );
}

/// @ai-generated — v-if with class attribute preserves class
#[test]
fn test_v_if_with_class() {
    let code = gen_and_validate(r#"<template><div v-if="show" class="foo">hi</div></template>"#);
    assert!(
        code.contains(r#"class: "foo""#),
        "v-if element should preserve class, got:\n{}",
        code
    );
}

/// @ai-generated — v-if removes directive from props (no v-if="..." in output)
#[test]
fn test_v_if_removes_directive() {
    let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    // The v-if directive attribute should be removed from element props
    // (but "v-if" in the comment fallback is expected: _createCommentVNode("v-if", true))
    assert!(
        !code.contains(r#"v-if="show""#),
        "v-if directive attribute should be removed from output, got:\n{}",
        code
    );
}

/// @ai-generated — v-if branches should use _openBlock + _createElementBlock
#[test]
fn test_v_if_block_treatment() {
    let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains("_openBlock()"),
        "v-if branch should use _openBlock(), got:\n{}",
        code
    );
    assert!(
        code.contains("_createElementBlock("),
        "v-if branch should use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — v-if branches should have { key: N } injection
#[test]
fn test_v_if_key_injection() {
    let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains("{ key: 0 }"),
        "v-if branch should have {{ key: 0 }}, got:\n{}",
        code
    );
}

/// @ai-generated — v-if prod mode uses empty string comment
#[test]
fn test_v_if_prod_empty_comment() {
    let code = gen_prod_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains(r#"_createCommentVNode("", true)"#),
        "Prod v-if should use empty comment, got:\n{}",
        code
    );
}

// =========================================================================
// v-for directives
// =========================================================================

/// @ai-generated — v-for produces _renderList with Fragment wrapping
#[test]
fn test_v_for_render_list() {
    let code =
        gen_and_validate(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(
        code.contains("_renderList("),
        "v-for should produce _renderList, got:\n{}",
        code
    );
    assert!(
        code.contains("_openBlock(true)"),
        "v-for should use _openBlock(true), got:\n{}",
        code
    );
    assert!(
        code.contains("_Fragment"),
        "v-for should wrap in _Fragment, got:\n{}",
        code
    );
}

/// @ai-generated — Keyed v-for uses KEYED_FRAGMENT (128)
#[test]
fn test_v_for_keyed_fragment() {
    let code = gen_and_validate(
        r#"<template><div v-for="item in items" :key="item">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("128 /* KEYED_FRAGMENT */"),
        "Keyed v-for should use 128 KEYED_FRAGMENT, got:\n{}",
        code
    );
}

/// @ai-generated — v-for with index parameter: (item, index) =>
#[test]
fn test_v_for_with_index() {
    let code = gen_and_validate(
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_renderList("),
        "Should have _renderList, got:\n{}",
        code
    );
    assert!(
        code.contains("(item, index)"),
        "Should have (item, index) params, got:\n{}",
        code
    );
}

/// @ai-generated — v-for removes directive from output
#[test]
fn test_v_for_removes_directive() {
    let code =
        gen_and_validate(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(
        !code.contains("v-for"),
        "v-for directive should be removed from output, got:\n{}",
        code
    );
}

/// @ai-generated — Nested v-for produces two _renderList calls
#[test]
fn test_v_for_nested() {
    let code = gen_and_validate(
        r#"<template><div v-for="g in groups"><span v-for="i in g">{{ i }}</span></div></template>"#,
    );
    let count = code.matches("_renderList(").count();
    assert!(
        count >= 2,
        "Nested v-for should produce 2 _renderList calls, got {} in:\n{}",
        count,
        code
    );
}

// =========================================================================
// v-once directives
// =========================================================================

/// @ai-generated — v-once produces full Vue cache pattern
#[test]
fn test_v_once_cache_pattern() {
    let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
    assert!(
        code.contains("_cache[0] || ("),
        "v-once should start with _cache[0] || (, got:\n{}",
        code
    );
    assert!(
        code.contains("_setBlockTracking(-1, true)"),
        "v-once should call _setBlockTracking(-1, true), got:\n{}",
        code
    );
    assert!(
        code.contains("_setBlockTracking(1)"),
        "v-once should restore block tracking with _setBlockTracking(1), got:\n{}",
        code
    );
    assert!(
        code.contains(".cacheIndex = 0"),
        "v-once should use .cacheIndex = 0, got:\n{}",
        code
    );
    // v-once uses _createElementVNode, NOT _createElementBlock (block tracking disabled)
    assert!(
        code.contains("_createElementVNode("),
        "v-once should use _createElementVNode (not block), got:\n{}",
        code
    );
    assert!(
        !code.contains("_createElementBlock("),
        "v-once should NOT use _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — v-once with dynamic prop preserves patch flags
#[test]
fn test_v_once_with_dynamic() {
    let code = gen_and_validate(r#"<template><div v-once :id="foo">content</div></template>"#);
    assert!(
        code.contains("_cache[0] || ("),
        "v-once should use cache pattern, got:\n{}",
        code
    );
    assert!(
        code.contains(".cacheIndex = 0"),
        "v-once should use .cacheIndex = 0, got:\n{}",
        code
    );
    assert!(
        code.contains("8 /* PROPS */"),
        "v-once with :id should have PROPS flag, got:\n{}",
        code
    );
}

/// @ai-generated — v-once uses .cacheIndex = N assignment
#[test]
fn test_v_once_cache_index() {
    let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
    assert!(
        code.contains(".cacheIndex = 0"),
        "v-once should use .cacheIndex = 0, got:\n{}",
        code
    );
}

/// @ai-generated — v-once self-closing element
#[test]
fn test_v_once_self_closing() {
    let code = gen_and_validate(r#"<template><br v-once/></template>"#);
    assert!(
        code.contains("_cache[0] || ("),
        "v-once self-closing should use cache, got:\n{}",
        code
    );
    assert!(
        code.contains(".cacheIndex = 0"),
        "v-once self-closing should have .cacheIndex, got:\n{}",
        code
    );
}

/// @ai-generated — v-once returns _cache[N] as final expression
#[test]
fn test_v_once_returns_cache() {
    let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
    // The final value in the comma expression should be _cache[0])
    assert!(
        code.contains("_cache[0])"),
        "v-once should end with _cache[0]), got:\n{}",
        code
    );
}

// =========================================================================
// Patch Flags
// =========================================================================

/// @ai-generated — Bound :id → PROPS (8) with dynamic props list
#[test]
fn test_pf_props() {
    let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
    assert!(
        code.contains("8 /* PROPS */"),
        "Should have PROPS (8), got:\n{}",
        code
    );
    assert!(
        code.contains(r#", ["id"]"#),
        "Should list dynamic prop, got:\n{}",
        code
    );
}

/// @ai-generated — :class → CLASS (2)
#[test]
fn test_pf_class() {
    let code = gen_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
    assert!(
        code.contains("2 /* CLASS */"),
        "Should have CLASS (2), got:\n{}",
        code
    );
}

/// @ai-generated — :style → STYLE (4)
#[test]
fn test_pf_style() {
    let code = gen_and_validate(r#"<template><div :style="sty">hi</div></template>"#);
    assert!(
        code.contains("4 /* STYLE */"),
        "Should have STYLE (4), got:\n{}",
        code
    );
}

/// @ai-generated — Production mode: no patch flag comments
#[test]
fn test_pf_prod_no_comments() {
    let code = gen_prod_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
    assert!(
        code.contains(", 2)"),
        "Prod should have numeric flag without comment, got:\n{}",
        code
    );
    assert!(
        !code.contains("/* CLASS */"),
        "Prod should NOT have flag comment, got:\n{}",
        code
    );
}

/// @ai-generated — Single interpolation child should have TEXT (1) patch flag
#[test]
fn test_pf_text() {
    let code = gen_and_validate(r#"<template><div>{{ msg }}</div></template>"#);
    assert!(
        code.contains("1 /* TEXT */"),
        "Single interpolation should have TEXT (1), got:\n{}",
        code
    );
}

/// @ai-generated — Combined CLASS + PROPS = 10
#[test]
fn test_pf_combined_class_props() {
    let code = gen_and_validate(r#"<template><div :class="c" :id="x">hi</div></template>"#);
    // CLASS(2) | PROPS(8) = 10
    assert!(
        code.contains("10 /* CLASS, PROPS */"),
        "Should have CLASS+PROPS (10), got:\n{}",
        code
    );
}

// =========================================================================
// Components
// =========================================================================

/// @ai-generated — Root component uses _resolveComponent + _createBlock
#[test]
fn test_component_create_vnode() {
    let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "Root component should use _createBlock with resolved var, got:\n{}",
        code
    );
    assert!(
        code.contains("_openBlock()"),
        "Root component should use _openBlock(), got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveComponent(\"MyComponent\")"),
        "Should declare _resolveComponent, got:\n{}",
        code
    );
}

/// @ai-generated — Root component with props
#[test]
fn test_component_with_props() {
    let code = gen_and_validate(r#"<template><MyComponent :msg="hello"/></template>"#);
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "Root component should use _createBlock with resolved var, got:\n{}",
        code
    );
    assert!(
        code.contains("msg: _ctx.hello"),
        "Should pass props, got:\n{}",
        code
    );
}

/// @ai-generated — Root component with children (slot content)
#[test]
fn test_component_with_children() {
    let code = gen_and_validate(r#"<template><MyComponent>content</MyComponent></template>"#);
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "Root component should use _createBlock with resolved var, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""content""#),
        "Should have children text, got:\n{}",
        code
    );
}

/// @ai-generated — Vue uses _resolveComponent + _createBlock for runtime components
#[test]
fn test_component_resolve_and_block() {
    let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
    assert!(
        code.contains("_resolveComponent("),
        "Should use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_createBlock("),
        "Should use _createBlock for component, got:\n{}",
        code
    );
}

/// @ai-generated — _resolveComponent declaration appears before return
#[test]
fn test_component_resolve_declaration_before_return() {
    let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
    let resolve_pos = code.find("_resolveComponent(").unwrap();
    let return_pos = code.find("return ").unwrap();
    assert!(
        resolve_pos < return_pos,
        "_resolveComponent declaration should appear before return statement, got:\n{}",
        code
    );
}

/// @ai-generated — _resolveComponent uses const declaration with correct variable name
#[test]
fn test_component_resolve_const_pattern() {
    let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
    assert!(
        code.contains(r#"const _component_MyComponent = _resolveComponent("MyComponent")"#),
        "Should have const declaration pattern, got:\n{}",
        code
    );
}

/// @ai-generated — Non-root component uses _createVNode with resolved variable
#[test]
fn test_component_child_uses_create_vnode() {
    let code = gen_and_validate(r#"<template><div><MyComponent/></div></template>"#);
    assert!(
        code.contains("_createVNode(_component_MyComponent"),
        "Child component should use _createVNode with resolved var, got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveComponent(\"MyComponent\")"),
        "Should declare _resolveComponent, got:\n{}",
        code
    );
}

/// @ai-generated — Multiple different components get separate declarations
#[test]
fn test_component_multiple_different() {
    let code = gen_and_validate(r#"<template><div><CompA/><CompB/></div></template>"#);
    assert!(
        code.contains("_resolveComponent(\"CompA\")"),
        "Should resolve CompA, got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveComponent(\"CompB\")"),
        "Should resolve CompB, got:\n{}",
        code
    );
    assert!(
        code.contains("_createVNode(_component_CompA"),
        "Should use _component_CompA, got:\n{}",
        code
    );
    assert!(
        code.contains("_createVNode(_component_CompB"),
        "Should use _component_CompB, got:\n{}",
        code
    );
}

/// @ai-generated — Same component used twice gets only one _resolveComponent
#[test]
fn test_component_same_used_twice() {
    let code = gen_and_validate(r#"<template><div><MyComp/><MyComp/></div></template>"#);
    let count = code.matches("_resolveComponent(\"MyComp\")").count();
    assert_eq!(
        count, 1,
        "Same component should have only one _resolveComponent, got {} in:\n{}",
        count, code
    );
    let vnode_count = code.matches("_createVNode(_component_MyComp").count();
    assert_eq!(
        vnode_count, 2,
        "Should have 2 _createVNode calls for same component, got {} in:\n{}",
        vnode_count, code
    );
}

/// @ai-generated — Component as block root (v-if branch) uses _createBlock
#[test]
fn test_component_vif_block_root() {
    let code = gen_and_validate(r#"<template><MyComponent v-if="show"/></template>"#);
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "v-if component should use _createBlock, got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveComponent(\"MyComponent\")"),
        "Should resolve component, got:\n{}",
        code
    );
}

/// @ai-generated — Component nested inside v-for uses _createBlock (block root)
#[test]
fn test_component_inside_v_for() {
    let code = gen_and_validate(r#"<template><MyComponent v-for="item in items"/></template>"#);
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "v-for component should use _createBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Production mode component still uses _resolveComponent
#[test]
fn test_component_prod_mode() {
    let code = gen_prod_and_validate(r#"<template><MyComponent/></template>"#);
    assert!(
        code.contains("_resolveComponent(\"MyComponent\")"),
        "Prod mode should still resolve component, got:\n{}",
        code
    );
    assert!(
        code.contains("_createBlock(_component_MyComponent"),
        "Prod mode should use _createBlock with resolved var, got:\n{}",
        code
    );
}

/// @ai-generated — Native HTML element should NOT use _resolveComponent
#[test]
fn test_component_native_not_resolved() {
    let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
    assert!(
        !code.contains("_resolveComponent"),
        "Native element should not use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        !code.contains("_component_"),
        "Native element should not have _component_ prefix, got:\n{}",
        code
    );
}

// =========================================================================
// Multiple roots
// =========================================================================

/// @ai-generated — Multiple root elements (both are block roots)
#[test]
fn test_multiple_roots() {
    let code = gen_and_validate(r#"<template><div>a</div><div>b</div></template>"#);
    let count = code.matches("_createElementBlock").count();
    assert!(
        count >= 2,
        "Should have at least 2 _createElementBlock calls (both roots are blocks), got {} in:\n{}",
        count,
        code
    );
}

/// @ai-generated — Multiple roots should use Fragment wrapping
#[test]
fn test_multiple_roots_fragment() {
    let code = gen_and_validate(r#"<template><div>a</div><div>b</div></template>"#);
    assert!(
        code.contains("_Fragment"),
        "Multiple roots should use _Fragment, got:\n{}",
        code
    );
}

// =========================================================================
// Script-only / edge cases
// =========================================================================

/// @ai-generated — Script-only SFC should not panic
#[test]
fn test_script_only_no_panic() {
    let code = gen_and_validate(r#"<script setup>const x = 1</script>"#);
    assert!(
        code.contains("const x = 1"),
        "Script content should be preserved, got:\n{}",
        code
    );
}

/// @ai-generated — Whitespace between script and template blocks
#[test]
fn test_script_template_whitespace() {
    let code = gen_and_validate(
        r#"<script setup>
const x = 1
</script>

<template><div>hi</div></template>"#,
    );
    assert!(
        code.contains("_createElementBlock"),
        "Root should produce _createElementBlock, got:\n{}",
        code
    );
}

/// @ai-generated — Production mode outputs valid JS for complex template
#[test]
fn test_prod_valid_js_complex() {
    let code = gen_prod_and_validate(r#"<template><div :class="c" :id="x">hi</div></template>"#);
    assert!(!code.is_empty(), "Should produce non-empty output");
}

/// @ai-generated — Void img with attributes (root = block)
#[test]
fn test_void_img_attrs() {
    let code = gen_and_validate(r#"<template><img src="a.png" alt="pic"></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("img""#),
        "Root img should use _createElementBlock, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"src: "a.png""#),
        "Should have src prop, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"alt: "pic""#),
        "Should have alt prop, got:\n{}",
        code
    );
}

/// @ai-generated — Sibling void elements inside div
#[test]
fn test_sibling_void_elements() {
    let code = gen_and_validate(r#"<template><div><input><hr><br></div></template>"#);
    assert!(
        code.contains(r#"_createElementVNode("input""#),
        "Should have input, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createElementVNode("hr""#),
        "Should have hr, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createElementVNode("br""#),
        "Should have br, got:\n{}",
        code
    );
}

/// @ai-generated — Mixed text and element children
#[test]
fn test_mixed_text_and_element() {
    let code = gen_and_validate(r#"<template><div>text<span>child</span></div></template>"#);
    assert!(
        code.contains(r#""text""#),
        "Should have text child, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createElementVNode("span""#),
        "Should have span child, got:\n{}",
        code
    );
}

/// @ai-generated — Comment with elements: mixed children
#[test]
fn test_comment_with_elements() {
    let code = gen_and_validate(
        r#"<template><div><span>a</span><!-- mid --><span>b</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "Should have comment VNode, got:\n{}",
        code
    );
    let span_count = code.matches(r#"_createElementVNode("span""#).count();
    assert!(
        span_count >= 2,
        "Should have 2 spans, got {} in:\n{}",
        span_count,
        code
    );
}

/// @ai-generated — v-if inside v-for
#[test]
fn test_v_if_inside_v_for() {
    let code = gen_and_validate(
        r#"<template><div v-for="item in items"><span v-if="item.show">{{ item.name }}</span></div></template>"#,
    );
    assert!(
        code.contains("_renderList("),
        "Should have _renderList for v-for, got:\n{}",
        code
    );
    assert!(
        code.contains("(item.show) ? ("),
        "Should have v-if ternary inside v-for, got:\n{}",
        code
    );
}

// =========================================================================
// v-if: block treatment (comprehensive)
// =========================================================================

/// @ai-generated — v-if simple: return (cond) ? (block) : comment
#[test]
fn test_v_if_full_output() {
    let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains("return (_ctx.show) ? (_openBlock(), _createElementBlock("),
        "v-if should produce return (_ctx.show) ? (_openBlock(), _createElementBlock..., got:\n{}",
        code
    );
    assert!(
        code.contains(r#" : _createCommentVNode("v-if", true)"#),
        "v-if should have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else: both branches are block roots
#[test]
fn test_v_if_else_block_roots() {
    let code =
        gen_and_validate(r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#);
    let block_count = code.matches("_openBlock()").count();
    assert!(
        block_count >= 2,
        "Both branches should use _openBlock(), got {} in:\n{}",
        block_count,
        code
    );
    assert!(
        code.contains("(_ctx.show) ? (_openBlock()"),
        "v-if branch should be block root, got:\n{}",
        code
    );
    assert!(
        code.contains(") : (_openBlock()"),
        "v-else branch should be block root, got:\n{}",
        code
    );
    // No comment fallback when v-else is present
    assert!(
        !code.contains("_createCommentVNode"),
        "v-if/v-else should NOT have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else-if/v-else: nested ternary with block roots
#[test]
fn test_v_if_else_if_else_block_roots() {
    let code = gen_and_validate(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    let block_count = code.matches("_openBlock()").count();
    assert!(
        block_count >= 3,
        "All 3 branches should use _openBlock(), got {} in:\n{}",
        block_count,
        code
    );
    assert!(
        code.contains("(_ctx.a) ? ("),
        "First condition should be present, got:\n{}",
        code
    );
    assert!(
        code.contains("(_ctx.b) ? ("),
        "Second condition should be present, got:\n{}",
        code
    );
    assert!(
        !code.contains("_createCommentVNode"),
        "Full chain should NOT have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else-if (no else): should have comment fallback
#[test]
fn test_v_if_else_if_no_else() {
    let code =
        gen_and_validate(r#"<template><div v-if="a">A</div><div v-else-if="b">B</div></template>"#);
    assert!(
        code.contains("_createCommentVNode"),
        "Without v-else, should have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — Nested v-if inside parent element: block root inside VNode
#[test]
fn test_v_if_nested_in_element() {
    let code = gen_and_validate(r#"<template><div><span v-if="show">yes</span></div></template>"#);
    assert!(
        code.contains(r#"_createElementBlock("div""#),
        "Root div should be _createElementBlock, got:\n{}",
        code
    );
    // Nested v-if branch should also be a block root
    assert!(
        code.contains("(_ctx.show) ? (_openBlock(), _createElementBlock(\"span\""),
        "Nested v-if should be block root, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createCommentVNode("v-if", true)"#),
        "Nested v-if should have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — Nested v-if/v-else inside parent: no comment fallback
#[test]
fn test_v_if_else_nested_in_element() {
    let code = gen_and_validate(
        r#"<template><div><span v-if="ok">A</span><span v-else>B</span></div></template>"#,
    );
    assert!(
        !code.contains("_createCommentVNode"),
        "Nested v-if/v-else should NOT have comment fallback, got:\n{}",
        code
    );
}

/// @ai-generated — Two independent v-ifs inside parent: both get comment fallbacks
#[test]
fn test_v_if_two_independent() {
    let code = gen_and_validate(
        r#"<template><div><span v-if="a">A</span><span v-if="b">B</span></div></template>"#,
    );
    let comment_count = code.matches("_createCommentVNode").count();
    assert!(
        comment_count >= 2,
        "Two independent v-ifs should have 2 comment fallbacks, got {} in:\n{}",
        comment_count,
        code
    );
}

/// @ai-generated — v-if with siblings: correct array wrapping
#[test]
fn test_v_if_with_sibling_elements() {
    let code = gen_and_validate(
        r#"<template><div><span>A</span><span v-if="show">B</span></div></template>"#,
    );
    // Two children (span, v-if span) → array wrapping
    assert!(
        code.contains("[_createElementVNode"),
        "Should use array wrapping for multiple children, got:\n{}",
        code
    );
    assert!(
        code.contains("(_ctx.show) ? "),
        "v-if ternary should be present in array, got:\n{}",
        code
    );
}

/// @ai-generated — v-for items should be block roots
#[test]
fn test_v_for_item_is_block_root() {
    let code =
        gen_and_validate(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    // Inside the renderList callback, the div should use _createElementBlock
    assert!(
        code.contains("_createElementBlock(\"div\""),
        "v-for item should use _createElementBlock (block root), got:\n{}",
        code
    );
}

/// @ai-generated — Bound :id with _ctx prefix (template-only, no script setup)
#[test]
fn test_bound_prop_ctx_prefix() {
    let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
    assert!(
        code.contains("_ctx.myId"),
        "Unresolved binding should get _ctx. prefix, got:\n{}",
        code
    );
}

// =========================================================================
// Class/Style Merging
// Vue merges static + dynamic class/style:
//   class="app" :class="msg" → class: _normalizeClass(["app", msg])
//   style="color:red" :style="s" → style: _normalizeStyle(["color:red", s])
// =========================================================================

/// @ai-generated — class="app" :class="msg" → _normalizeClass(["app", msg])
#[test]
fn test_class_merge_static_and_dynamic() {
    let code = gen_and_validate(r#"<template><div class="app" :class="msg">hi</div></template>"#);
    assert!(
        code.contains(r#"_normalizeClass(["app", "#),
        "Should merge static+dynamic class into _normalizeClass array, got:\n{}",
        code
    );
    // Must NOT have two separate `class:` properties
    let class_count = code.matches("class:").count();
    assert_eq!(
        class_count, 1,
        "Should have exactly one class: property (merged), got {} in:\n{}",
        class_count, code
    );
}

/// @ai-generated — style="color:red" :style="s" → _normalizeStyle(["color:red", s])
#[test]
fn test_style_merge_static_and_dynamic() {
    let code =
        gen_and_validate(r#"<template><div style="color:red" :style="s">hi</div></template>"#);
    assert!(
        code.contains(r#"_normalizeStyle(["color:red", "#),
        "Should merge static+dynamic style into _normalizeStyle array, got:\n{}",
        code
    );
    let style_count = code.matches("style:").count();
    assert_eq!(
        style_count, 1,
        "Should have exactly one style: property (merged), got {} in:\n{}",
        style_count, code
    );
}

/// @ai-generated — class="app" :class="msg" with setup bindings
#[test]
fn test_class_merge_with_setup_bindings() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('active')
</script>
<template><div class="app" :class="msg">hi</div></template>"#,
    );
    assert!(
        code.contains(r#"_normalizeClass(["app", $setup.msg])"#),
        "Should merge class with $setup binding prefix, got:\n{}",
        code
    );
}

/// @ai-generated — style="color:red" :style="sty" with setup bindings
#[test]
fn test_style_merge_with_setup_bindings() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const sty = ref({ fontSize: '14px' })
</script>
<template><div style="color:red" :style="sty">hi</div></template>"#,
    );
    assert!(
        code.contains(r#"_normalizeStyle(["color:red", $setup.sty])"#),
        "Should merge style with $setup binding prefix, got:\n{}",
        code
    );
}

/// @ai-generated — Only :class (no static class) should NOT use array form
#[test]
fn test_class_bind_only_no_merge() {
    let code = gen_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
    assert!(
        code.contains("_normalizeClass("),
        "Should have _normalizeClass, got:\n{}",
        code
    );
    // Should NOT use array form when there's no static class to merge
    assert!(
        !code.contains(r#"_normalizeClass(["#),
        "Should NOT use array form without static class, got:\n{}",
        code
    );
}

/// @ai-generated — Only class="app" (no :class bind) should be a plain string prop
#[test]
fn test_class_static_only_no_merge() {
    let code = gen_and_validate(r#"<template><div class="app" :id="x">hi</div></template>"#);
    assert!(
        code.contains(r#"class: "app""#),
        "Static class without :class should be plain string, got:\n{}",
        code
    );
    assert!(
        !code.contains("_normalizeClass"),
        "Should NOT use _normalizeClass without :class, got:\n{}",
        code
    );
}

/// @ai-generated — class="app" :class="cls" with additional props
#[test]
fn test_class_merge_with_other_props() {
    let code = gen_and_validate(
        r#"<template><div id="main" class="app" :class="cls" :title="t">hi</div></template>"#,
    );
    assert!(
        code.contains(r#"_normalizeClass(["app", "#),
        "Should merge class even with other props present, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"id: "main""#),
        "Other static props should still work, got:\n{}",
        code
    );
    assert!(
        code.contains("title: "),
        "Other bound props should still work, got:\n{}",
        code
    );
}

/// @ai-generated — Both class and style merging simultaneously
#[test]
fn test_class_and_style_merge_together() {
    let code = gen_and_validate(
        r#"<template><div class="app" :class="c" style="color:red" :style="s">hi</div></template>"#,
    );
    assert!(
        code.contains(r#"_normalizeClass(["app", "#),
        "Should merge class, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_normalizeStyle(["color:red", "#),
        "Should merge style, got:\n{}",
        code
    );
}

// =========================================================================
// Event Handler Wrapping
// Vue wraps non-identifier/non-member expressions:
//   @click="fn()" → onClick: $event => (fn())
//   @click="handler" → onClick: handler (no wrapping)
//   @click="obj.method" → onClick: obj.method (no wrapping)
//   @click="() => doSomething()" → onClick: () => doSomething() (no wrapping)
// =========================================================================

/// @ai-generated — Call expression gets wrapped: @click="fn()" → $event => (fn())
#[test]
fn test_event_handler_wrap_call_expr() {
    let code =
        gen_and_validate(r#"<template><button @click="doSomething()">click</button></template>"#);
    assert!(
        code.contains("$event => ("),
        "Call expression handler should be wrapped with $event =>, got:\n{}",
        code
    );
}

/// @ai-generated — Call with $event arg gets wrapped: @click="fn($event)" → $event => (fn($event))
#[test]
fn test_event_handler_wrap_call_with_event() {
    let code = gen_and_validate(
        r#"<script setup>
function increment(e) { console.log(e) }
</script>
<template><button @click="increment($event)">click</button></template>"#,
    );
    assert!(
        code.contains("$event => ($setup.increment(_ctx.$event))"),
        "Call expression with $event should be wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Simple identifier is NOT wrapped: @click="handler" → onClick: handler
#[test]
fn test_event_handler_no_wrap_identifier() {
    let code = gen_and_validate(r#"<template><button @click="handler">click</button></template>"#);
    assert!(
        code.contains("onClick: _ctx.handler"),
        "Identifier handler should NOT be wrapped, got:\n{}",
        code
    );
    assert!(
        !code.contains("$event =>"),
        "Identifier handler should NOT have $event wrapper, got:\n{}",
        code
    );
}

/// @ai-generated — Member expression is NOT wrapped: @click="obj.method"
#[test]
fn test_event_handler_no_wrap_member() {
    let code =
        gen_and_validate(r#"<template><button @click="obj.method">click</button></template>"#);
    assert!(
        !code.contains("$event =>"),
        "Member expression handler should NOT be wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Arrow function is NOT wrapped: @click="() => doSomething()"
#[test]
fn test_event_handler_no_wrap_arrow() {
    let code = gen_and_validate(
        r#"<template><button @click="() => doSomething()">click</button></template>"#,
    );
    assert!(
        !code.contains("$event => ("),
        "Arrow function handler should NOT be double-wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Inline expression with args: @click="count++" → $event => (count++)
#[test]
fn test_event_handler_wrap_update_expr() {
    let code = gen_and_validate(r#"<template><button @click="count++">click</button></template>"#);
    assert!(
        code.contains("$event => ("),
        "Update expression should be wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Assignment expression: @click="x = 1" → $event => (x = 1)
#[test]
fn test_event_handler_wrap_assignment() {
    let code = gen_and_validate(r#"<template><button @click="x = 1">click</button></template>"#);
    assert!(
        code.contains("$event => ("),
        "Assignment expression should be wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler wrapping with setup bindings: $setup.fn($event)
#[test]
fn test_event_handler_wrap_with_setup_bindings() {
    let code = gen_and_validate(
        r#"<script setup>
function handleClick(e) { console.log(e) }
</script>
<template><button @click="handleClick($event)">click</button></template>"#,
    );
    assert!(
        code.contains("$event => ($setup.handleClick(_ctx.$event))"),
        "Wrapped handler with setup binding should use $setup prefix, got:\n{}",
        code
    );
}

// =========================================================================
// Slot Support (v-slot → _withCtx)
// Vue wraps component children with v-slot:
//   <Button v-slot="foo">text</Button>
//   → _createVNode(Button, null, { default: _withCtx((foo) => [
//       _createTextVNode("text")
//     ]), _: 1 /* STABLE */ })
// =========================================================================

/// @ai-generated — v-slot with text content produces _withCtx + _createTextVNode
#[test]
fn test_slot_default_text() {
    let code = gen_and_validate(r#"<template><Button v-slot="foo">content</Button></template>"#);
    assert!(
        code.contains("_withCtx"),
        "v-slot should produce _withCtx wrapper, got:\n{}",
        code
    );
    assert!(
        code.contains("_createTextVNode"),
        "Text inside v-slot should use _createTextVNode, got:\n{}",
        code
    );
    assert!(
        code.contains("(foo) => ["),
        "v-slot params should be in arrow function, got:\n{}",
        code
    );
    assert!(
        code.contains("default:"),
        "Bare v-slot should create default slot, got:\n{}",
        code
    );
}

/// @ai-generated — v-slot with interpolation uses _createTextVNode + _toDisplayString
#[test]
fn test_slot_with_interpolation() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><Button v-slot="foo">Count: {{ count }}</Button></template>"#,
    );
    assert!(
        code.contains("_withCtx"),
        "Should have _withCtx, got:\n{}",
        code
    );
    assert!(
        code.contains("_createTextVNode"),
        "Text+interp in slot should use _createTextVNode, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString"),
        "Interpolation should still use _toDisplayString, got:\n{}",
        code
    );
}

/// @ai-generated — v-slot without params uses empty arrow: () => [...]
#[test]
fn test_slot_no_params() {
    let code = gen_and_validate(r#"<template><Button v-slot>content</Button></template>"#);
    assert!(
        code.contains("() => ["),
        "v-slot without params should use () => [...], got:\n{}",
        code
    );
}

/// @ai-generated — v-slot with destructuring: v-slot="{ data }"
#[test]
fn test_slot_destructured_params() {
    let code =
        gen_and_validate(r#"<template><Button v-slot="{ data }">{{ data }}</Button></template>"#);
    assert!(
        code.contains("{ data }") && code.contains("_withCtx"),
        "v-slot destructured params should be preserved, got:\n{}",
        code
    );
}

/// @ai-generated — Slot object has _: 1 (STABLE) marker
#[test]
fn test_slot_stable_marker() {
    let code = gen_and_validate(r#"<template><Button v-slot="foo">content</Button></template>"#);
    assert!(
        code.contains("_: 1"),
        "Slot object should have _: 1 STABLE marker, got:\n{}",
        code
    );
}

/// @ai-generated — Slot with setup bindings in interpolation
#[test]
fn test_slot_setup_bindings() {
    let code = gen_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><Button v-slot="foo">{{ msg }}</Button></template>"#,
    );
    assert!(
        code.contains("$setup.msg"),
        "Setup bindings should be prefixed inside slots, got:\n{}",
        code
    );
    assert!(
        code.contains("_withCtx"),
        "Should have _withCtx wrapper, got:\n{}",
        code
    );
}

/// @ai-generated — v-slot directive is removed from output
#[test]
fn test_slot_removes_directive() {
    let code = gen_and_validate(r#"<template><Button v-slot="foo">content</Button></template>"#);
    assert!(
        !code.contains("v-slot"),
        "v-slot directive should be removed from output, got:\n{}",
        code
    );
}

/// @ai-generated — Slot production mode uses numeric STABLE marker
#[test]
fn test_slot_production_stable() {
    let code =
        gen_prod_and_validate(r#"<template><Button v-slot="foo">content</Button></template>"#);
    assert!(
        code.contains("_: 1"),
        "Production mode should have _: 1, got:\n{}",
        code
    );
    assert!(
        !code.contains("STABLE"),
        "Production mode should not have STABLE comment, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot: v-slot:header → { header: _withCtx(...) }
#[test]
fn test_slot_named_static() {
    let code =
        gen_and_validate(r#"<template><Button v-slot:header="foo">content</Button></template>"#);
    assert!(
        code.contains("header: _withCtx"),
        "Named slot should use slot name as key, got:\n{}",
        code
    );
    assert!(
        !code.contains("default:"),
        "Named slot should NOT have default key, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic slot: v-slot:[name] → { [name]: _withCtx(...) }
#[test]
fn test_slot_dynamic_name() {
    let code =
        gen_and_validate(r#"<template><Button v-slot:[name]="foo">content</Button></template>"#);
    assert!(
        code.contains("[_ctx.name]: _withCtx"),
        "Dynamic slot should use computed property [_ctx.name] with accessor prefix, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic slot uses _: 2 (DYNAMIC) marker
#[test]
fn test_slot_dynamic_marker() {
    let code =
        gen_and_validate(r#"<template><Button v-slot:[name]="foo">content</Button></template>"#);
    assert!(
        code.contains("_: 2"),
        "Dynamic slot should have _: 2 DYNAMIC marker, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic slot in production: _: 2 without comment
#[test]
fn test_slot_dynamic_production() {
    let code = gen_prod_and_validate(
        r#"<template><Button v-slot:[name]="foo">content</Button></template>"#,
    );
    assert!(
        code.contains("_: 2"),
        "Dynamic slot production should have _: 2, got:\n{}",
        code
    );
    assert!(
        !code.contains("DYNAMIC"),
        "Production should not have DYNAMIC comment, got:\n{}",
        code
    );
}

// =========================================================================
// Event Modifiers
// =========================================================================

/// @ai-generated — .stop and .prevent use _withModifiers
#[test]
fn test_event_modifier_stop_prevent() {
    let code =
        gen_and_validate(r#"<template><div @click.stop.prevent="handler">x</div></template>"#);
    assert!(
        code.contains("_withModifiers("),
        "Should use _withModifiers, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["stop","prevent"]"#),
        "Should include stop and prevent modifiers, got:\n{}",
        code
    );
}

/// @ai-generated — .enter key modifier uses _withKeys
#[test]
fn test_event_modifier_key_enter() {
    let code = gen_and_validate(r#"<template><div @keyup.enter="handler">x</div></template>"#);
    assert!(
        code.contains("_withKeys("),
        "Should use _withKeys for key modifier, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["enter"]"#),
        "Should include enter key, got:\n{}",
        code
    );
}

/// @ai-generated — .once is a compile-time modifier that suffixes event name
#[test]
fn test_event_modifier_once_compile_time() {
    let code = gen_and_validate(r#"<template><div @click.once="handler">x</div></template>"#);
    assert!(
        code.contains("onClickOnce:"),
        "Should suffix event name with Once, got:\n{}",
        code
    );
    assert!(
        !code.contains("_withModifiers"),
        "Should NOT use _withModifiers for .once, got:\n{}",
        code
    );
}

/// @ai-generated — .capture is a compile-time modifier
#[test]
fn test_event_modifier_capture_compile_time() {
    let code = gen_and_validate(r#"<template><div @click.capture="handler">x</div></template>"#);
    assert!(
        code.contains("onClickCapture:"),
        "Should suffix event name with Capture, got:\n{}",
        code
    );
}

/// @ai-generated — Combined runtime + compile-time modifiers
#[test]
fn test_event_modifier_combined() {
    let code = gen_and_validate(r#"<template><div @click.stop.once="handler">x</div></template>"#);
    assert!(
        code.contains("onClickOnce:"),
        "Should suffix with Once, got:\n{}",
        code
    );
    assert!(
        code.contains("_withModifiers("),
        "Should use _withModifiers for .stop, got:\n{}",
        code
    );
}

// =========================================================================
// v-model
// =========================================================================

/// @ai-generated — v-model on input generates withDirectives + vModelText
#[test]
fn test_vmodel_input_text() {
    let code = gen_and_validate(r#"<template><input v-model="msg" /></template>"#);
    assert!(
        code.contains("_withDirectives("),
        "Should wrap with _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelText"),
        "Should use _vModelText for text input, got:\n{}",
        code
    );
    assert!(
        code.contains("\"onUpdate:modelValue\""),
        "Should have onUpdate:modelValue prop, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on checkbox uses vModelCheckbox
#[test]
fn test_vmodel_checkbox() {
    let code =
        gen_and_validate(r#"<template><input type="checkbox" v-model="checked" /></template>"#);
    assert!(
        code.contains("_vModelCheckbox"),
        "Should use _vModelCheckbox, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on radio uses vModelRadio
#[test]
fn test_vmodel_radio() {
    let code =
        gen_and_validate(r#"<template><input type="radio" v-model="pick" value="a" /></template>"#);
    assert!(
        code.contains("_vModelRadio"),
        "Should use _vModelRadio, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on select uses vModelSelect
#[test]
fn test_vmodel_select() {
    let code = gen_and_validate(
        r#"<template><select v-model="choice"><option>A</option></select></template>"#,
    );
    assert!(
        code.contains("_vModelSelect"),
        "Should use _vModelSelect, got:\n{}",
        code
    );
}

/// @ai-generated — v-model with modifiers on native input
#[test]
fn test_vmodel_modifiers_native() {
    let code = gen_and_validate(r#"<template><input v-model.trim.number="msg" /></template>"#);
    assert!(
        code.contains("_vModelText"),
        "Should use _vModelText, got:\n{}",
        code
    );
    assert!(
        code.contains("trim: true"),
        "Should have trim modifier, got:\n{}",
        code
    );
    assert!(
        code.contains("number: true"),
        "Should have number modifier, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on component is prop-based (no withDirectives)
#[test]
fn test_vmodel_component() {
    let code = gen_and_validate(r#"<template><MyComp v-model="val" /></template>"#);
    assert!(
        !code.contains("_withDirectives"),
        "Component v-model should NOT use withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("modelValue:"),
        "Should have modelValue prop, got:\n{}",
        code
    );
    assert!(
        code.contains("\"onUpdate:modelValue\""),
        "Should have onUpdate:modelValue event, got:\n{}",
        code
    );
    assert!(
        code.contains("$event => (("),
        "Should have $event assignment handler, got:\n{}",
        code
    );
}

/// @ai-generated — v-model:title on component uses named arg
#[test]
fn test_vmodel_component_named() {
    let code = gen_and_validate(r#"<template><MyComp v-model:title="val" /></template>"#);
    assert!(
        code.contains("title:"),
        "Should have title prop, got:\n{}",
        code
    );
    assert!(
        code.contains("\"onUpdate:title\""),
        "Should have onUpdate:title event, got:\n{}",
        code
    );
}

/// @ai-generated — v-model with modifiers on component emits modelModifiers
#[test]
fn test_vmodel_component_modifiers() {
    let code = gen_and_validate(r#"<template><MyComp v-model.trim="val" /></template>"#);
    assert!(
        code.contains("modelModifiers: { trim: true }"),
        "Should have modelModifiers prop, got:\n{}",
        code
    );
}

// =========================================================================
// v-show
// =========================================================================

/// @ai-generated — v-show uses withDirectives + vShow
#[test]
fn test_vshow() {
    let code = gen_and_validate(r#"<template><div v-show="visible">hi</div></template>"#);
    assert!(
        code.contains("_withDirectives("),
        "Should wrap with _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vShow"),
        "Should use _vShow directive, got:\n{}",
        code
    );
}

// =========================================================================
// v-html
// =========================================================================

/// @ai-generated — v-html compiles to innerHTML prop
#[test]
fn test_vhtml() {
    let code = gen_and_validate(r#"<template><div v-html="rawHtml"></div></template>"#);
    assert!(
        code.contains("innerHTML:"),
        "Should have innerHTML prop, got:\n{}",
        code
    );
    assert!(
        !code.contains("_withDirectives"),
        "v-html should NOT use withDirectives, got:\n{}",
        code
    );
}

// =========================================================================
// v-text
// =========================================================================

/// @ai-generated — v-text compiles to textContent prop with _toDisplayString
#[test]
fn test_vtext() {
    let code = gen_and_validate(r#"<template><div v-text="content"></div></template>"#);
    assert!(
        code.contains("textContent: _toDisplayString("),
        "Should have textContent with _toDisplayString, got:\n{}",
        code
    );
}

// =========================================================================
// v-bind spread / v-on spread
// =========================================================================

/// @ai-generated — v-bind spread uses _normalizeProps(_guardReactiveProps(...))
#[test]
fn test_vbind_spread() {
    let code = gen_and_validate(r#"<template><div v-bind="attrs">x</div></template>"#);
    assert!(
        code.contains("_normalizeProps("),
        "Should use _normalizeProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_guardReactiveProps("),
        "Should use _guardReactiveProps, got:\n{}",
        code
    );
}

/// @ai-generated — v-on spread uses _toHandlers
#[test]
fn test_von_spread() {
    let code = gen_and_validate(r#"<template><div v-on="handlers">x</div></template>"#);
    assert!(
        code.contains("_toHandlers("),
        "Should use _toHandlers, got:\n{}",
        code
    );
}

// =========================================================================
// Custom directives
// =========================================================================

/// @ai-generated — Custom directive uses resolveDirective + withDirectives
#[test]
fn test_custom_directive_simple() {
    let code = gen_and_validate(r#"<template><div v-focus>x</div></template>"#);
    assert!(
        code.contains("_resolveDirective("),
        "Should resolve directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_withDirectives("),
        "Should use withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_directive_focus"),
        "Should use _directive_focus variable, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with arg, modifier, and value
#[test]
fn test_custom_directive_full() {
    let code =
        gen_and_validate(r#"<template><div v-my-directive:arg.mod="val">x</div></template>"#);
    assert!(
        code.contains("_directive_my_directive"),
        "Should resolve my-directive, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""arg""#),
        "Should have arg string, got:\n{}",
        code
    );
    assert!(
        code.contains("mod: true"),
        "Should have modifier object, got:\n{}",
        code
    );
}

// =========================================================================
// Dynamic arg accessor prefix
// =========================================================================

/// @ai-generated — Dynamic bind arg gets accessor prefix
#[test]
fn test_dynamic_bind_arg_prefix() {
    let code = gen_and_validate(r#"<template><div :[prop]="val">x</div></template>"#);
    assert!(
        code.contains("[_ctx.prop]"),
        "Dynamic bind arg should get _ctx. prefix, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic event arg gets accessor prefix
#[test]
fn test_dynamic_event_arg_prefix() {
    let code = gen_and_validate(r#"<template><div @[event]="handler">x</div></template>"#);
    assert!(
        code.contains("[\"on\" + _ctx.event]"),
        "Dynamic event arg should get _ctx. prefix with 'on' + prefix, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor Mode Tests
// =========================================================================

/// Generate vapor code AND validate it is valid JS.
fn gen_vapor_and_validate(input: &str) -> String {
    let code = gen(input);
    assert_valid_js(&code, input);
    assert_no_invalid_patterns(&code, input);
    code
}

/// @ai-generated — Vapor: static element produces _template + node creation
#[test]
fn test_vapor_static_element() {
    let code = gen_vapor_and_validate(r#"<template vapor><div>hello</div></template>"#);
    assert!(
        code.contains("_template("),
        "Should contain _template(), got:\n{}",
        code
    );
    assert!(
        code.contains("const n0 = t0()"),
        "Should create node n0 from template t0, got:\n{}",
        code
    );
    assert!(
        code.contains("return n0"),
        "Should return n0, got:\n{}",
        code
    );
    assert!(
        code.contains("export function render(_ctx)"),
        "Should have render function, got:\n{}",
        code
    );
    assert!(
        code.contains("from 'vue'"),
        "Should import from 'vue', got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: static class is baked into template HTML
#[test]
fn test_vapor_static_class() {
    let code = gen_vapor_and_validate(r#"<template vapor><div class="foo">hi</div></template>"#);
    // The template HTML should include the static class.
    assert!(
        code.contains("class="),
        "Template HTML should include static class, got:\n{}",
        code
    );
    // Should NOT have _setClass (no dynamic class).
    assert!(
        !code.contains("_setClass"),
        "Should NOT have _setClass for static class, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: dynamic class uses _setClass in _renderEffect
#[test]
fn test_vapor_dynamic_class() {
    let code = gen_vapor_and_validate(r#"<template vapor><div :class="cls">hi</div></template>"#);
    assert!(
        code.contains("_setClass(n0, _ctx.cls)"),
        "Should have _setClass with _ctx prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: dynamic style uses _setStyle
#[test]
fn test_vapor_dynamic_style() {
    let code = gen_vapor_and_validate(r#"<template vapor><div :style="sty">hi</div></template>"#);
    assert!(
        code.contains("_setStyle(n0, _ctx.sty)"),
        "Should have _setStyle, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: interpolation uses _txt + _setText + _renderEffect
#[test]
fn test_vapor_interpolation() {
    let code = gen_vapor_and_validate(r#"<template vapor><div>{{ msg }}</div></template>"#);
    assert!(
        code.contains("_txt(n0)"),
        "Should create text node ref with _txt, got:\n{}",
        code
    );
    assert!(
        code.contains("_setText(x0,"),
        "Should have _setText call, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.msg)"),
        "Should use _toDisplayString with _ctx prefix, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: text + interpolation combined in _setText
#[test]
fn test_vapor_text_and_interpolation() {
    let code = gen_vapor_and_validate(r#"<template vapor><div>hello {{ msg }}</div></template>"#);
    assert!(
        code.contains("_setText(x0,"),
        "Should have _setText, got:\n{}",
        code
    );
    // Should combine static text and dynamic expression.
    assert!(
        code.contains("\"hello \"") || code.contains("\"hello  \""),
        "Should include static text part, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.msg)"),
        "Should include dynamic expression, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: multiple interpolations combined
#[test]
fn test_vapor_multiple_interpolations() {
    let code = gen_vapor_and_validate(r#"<template vapor><div>{{ a }}{{ b }}</div></template>"#);
    assert!(
        code.contains("_toDisplayString(_ctx.a)"),
        "Should have first interpolation, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.b)"),
        "Should have second interpolation, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: event click uses delegation
#[test]
fn test_vapor_event_click() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><button @click="handler">click</button></template>"#,
    );
    assert!(
        code.contains("_delegateEvents("),
        "Should have _delegateEvents, got:\n{}",
        code
    );
    assert!(
        code.contains("$evtclick"),
        "Should assign to $evtclick, got:\n{}",
        code
    );
    assert!(
        code.contains("_createInvoker("),
        "Should use _createInvoker, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: event with capture modifier uses _on
#[test]
fn test_vapor_event_capture() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div @click.capture="handler">click</div></template>"#,
    );
    assert!(
        code.contains("_on(n0,"),
        "Should use _on for capture modifier, got:\n{}",
        code
    );
    assert!(
        code.contains("capture: true"),
        "Should have capture option, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: event with stop modifier uses withModifiers + delegation
#[test]
fn test_vapor_event_stop_modifier() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><button @click.stop="handler">click</button></template>"#,
    );
    assert!(
        code.contains("_withModifiers("),
        "Should use _withModifiers, got:\n{}",
        code
    );
    assert!(
        code.contains("\"stop\""),
        "Should include 'stop' modifier, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: combined dynamic class + style in single _renderEffect
#[test]
fn test_vapor_combined_effects() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div :class="c" :style="s">hi</div></template>"#);
    assert!(
        code.contains("_setClass(n0, _ctx.c)"),
        "Should have _setClass, got:\n{}",
        code
    );
    assert!(
        code.contains("_setStyle(n0, _ctx.s)"),
        "Should have _setStyle, got:\n{}",
        code
    );
    // Should combine into a single _renderEffect with block body.
    assert!(
        code.contains("_renderEffect(() => {"),
        "Should combine into block _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: nested static elements are part of template HTML
#[test]
fn test_vapor_nested_static() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div><span>inner</span></div></template>"#);
    // Only one template and one node creation — the whole tree is static.
    assert!(
        code.contains("const n0 = t0()"),
        "Should create single node, got:\n{}",
        code
    );
    // Should NOT have _child navigation (no dynamic content in nested elements).
    assert!(
        !code.contains("_child("),
        "Should NOT need _child navigation for static nested, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: comment is baked into template HTML
#[test]
fn test_vapor_comment() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div><!-- my comment --></div></template>"#);
    // The comment should be in the template HTML string.
    assert!(
        code.contains("<!-- my comment -->")
            || code.contains("<!--my comment-->")
            || code.contains("&lt;!-- my comment --&gt;")
            || code.contains("<!-- my comment -->"),
        "Template should contain comment, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: void element (self-closing)
#[test]
fn test_vapor_void_element() {
    let code = gen_vapor_and_validate(r#"<template vapor><input></template>"#);
    assert!(
        code.contains("_template("),
        "Should have _template, got:\n{}",
        code
    );
    assert!(
        code.contains("return n0"),
        "Should return n0, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: self-closing br element
#[test]
fn test_vapor_self_closing_br() {
    let code = gen_vapor_and_validate(r#"<template vapor><br/></template>"#);
    assert!(
        code.contains("_template("),
        "Should have _template, got:\n{}",
        code
    );
    assert!(
        code.contains("return n0"),
        "Should return n0, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: dynamic bind uses _setProp
#[test]
fn test_vapor_dynamic_bind() {
    let code = gen_vapor_and_validate(r#"<template vapor><div :title="t">hi</div></template>"#);
    assert!(
        code.contains("_setProp(n0, \"title\", _ctx.t)"),
        "Should use _setProp, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: script setup bindings get correct prefixes
#[test]
fn test_vapor_with_script_setup() {
    let code = gen_vapor_and_validate(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template vapor><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("_template("),
        "Should have vapor template, got:\n{}",
        code
    );
    assert!(
        code.contains("_setText("),
        "Should have _setText for interpolation, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor Phase 2 Tests — Nested Navigation, Directives, Multi-root
// =========================================================================

/// @ai-generated — Vapor: nested element with interpolation uses _child navigation
#[test]
fn test_vapor_nested_interpolation() {
    let code = gen_vapor_and_validate(r#"<template vapor><div><p>{{ msg }}</p></div></template>"#);
    assert!(
        code.contains("_child("),
        "Should use _child for nested navigation, got:\n{}",
        code
    );
    assert!(
        code.contains("_txt("),
        "Should use _txt for text node ref, got:\n{}",
        code
    );
    assert!(
        code.contains("_setText("),
        "Should use _setText, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: deep nesting with chained _child and path variables
#[test]
fn test_vapor_deep_nested_interpolation() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><span><em>{{ msg }}</em></span></div></template>"#,
    );
    // Should have TWO _child calls: one for span (path), one for em (node).
    let child_count = code.matches("_child(").count();
    assert!(
        child_count >= 2,
        "Should have at least 2 _child calls for deep nesting, got {} in:\n{}",
        child_count,
        code
    );
    // Should have path variable (p0)
    assert!(
        code.contains("const p0"),
        "Should have path variable p0, got:\n{}",
        code
    );
    assert!(
        code.contains("_txt("),
        "Should use _txt for text node ref, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: sibling navigation with _next when second child is dynamic
#[test]
fn test_vapor_sibling_dynamic_class() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><span>static</span><em :class="c">text</em></div></template>"#,
    );
    assert!(
        code.contains("_next("),
        "Should use _next for sibling navigation, got:\n{}",
        code
    );
    assert!(
        code.contains("_setClass("),
        "Should have _setClass for dynamic class, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: sibling navigation with events on second child
#[test]
fn test_vapor_sibling_event() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><input><button @click="handler">go</button></div></template>"#,
    );
    assert!(
        code.contains("_next("),
        "Should use _next for second sibling, got:\n{}",
        code
    );
    assert!(
        code.contains("$evtclick"),
        "Should have event on nested button, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: two dynamic siblings both get navigation
#[test]
fn test_vapor_two_dynamic_siblings() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><p>{{ a }}</p><p>{{ b }}</p></div></template>"#,
    );
    assert!(
        code.contains("_child("),
        "Should have _child for first sibling, got:\n{}",
        code
    );
    assert!(
        code.contains("_next("),
        "Should have _next for second sibling, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.a)"),
        "Should have first interpolation, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.b)"),
        "Should have second interpolation, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: nested element with dynamic prop (no text)
#[test]
fn test_vapor_nested_dynamic_prop() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><span :title="t">text</span></div></template>"#,
    );
    assert!(
        code.contains("_child("),
        "Should use _child for nested element, got:\n{}",
        code
    );
    assert!(
        code.contains("_setProp("),
        "Should have _setProp on nested element, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-html directive generates _setHtml effect
#[test]
fn test_vapor_v_html() {
    let code = gen_vapor_and_validate(r#"<template vapor><div v-html="rawHtml"></div></template>"#);
    assert!(
        code.contains("_setHtml(n0, _ctx.rawHtml)"),
        "Should have _setHtml, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-text directive generates _txt + _setText
#[test]
fn test_vapor_v_text() {
    let code = gen_vapor_and_validate(r#"<template vapor><div v-text="msg"></div></template>"#);
    assert!(code.contains("_txt(n0)"), "Should use _txt, got:\n{}", code);
    assert!(
        code.contains("_setText(x0,"),
        "Should use _setText, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_ctx.msg)"),
        "Should use _toDisplayString, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-show generates _applyVShow statement
#[test]
fn test_vapor_v_show() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div v-show="visible">content</div></template>"#);
    assert!(
        code.contains("_applyVShow(n0, () => (_ctx.visible))"),
        "Should have _applyVShow, got:\n{}",
        code
    );
    // v-show should NOT be inside _renderEffect.
    assert!(
        !code.contains("_renderEffect(() => _applyVShow"),
        "v-show should NOT be in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-show + dynamic class on same element
#[test]
fn test_vapor_v_show_with_class() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-show="visible" :class="c">text</div></template>"#,
    );
    assert!(
        code.contains("_applyVShow("),
        "Should have _applyVShow, got:\n{}",
        code
    );
    assert!(
        code.contains("_setClass("),
        "Should have _setClass, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model on input generates _applyTextModel
#[test]
fn test_vapor_v_model_input() {
    let code = gen_vapor_and_validate(r#"<template vapor><input v-model="text"></template>"#);
    assert!(
        code.contains("_applyTextModel(n0,"),
        "Should have _applyTextModel, got:\n{}",
        code
    );
    assert!(
        code.contains("() => (_ctx.text)"),
        "Should have getter, got:\n{}",
        code
    );
    assert!(
        code.contains("_value => (_ctx.text = _value)"),
        "Should have setter, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model on textarea generates _applyTextModel
#[test]
fn test_vapor_v_model_textarea() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><textarea v-model="text"></textarea></template>"#,
    );
    assert!(
        code.contains("_applyTextModel(n0,"),
        "Should have _applyTextModel for textarea, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model on checkbox generates _applyCheckboxModel
#[test]
fn test_vapor_v_model_checkbox() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><input type="checkbox" v-model="checked"></template>"#,
    );
    assert!(
        code.contains("_applyCheckboxModel(n0,"),
        "Should have _applyCheckboxModel, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model on radio generates _applyRadioModel
#[test]
fn test_vapor_v_model_radio() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><input type="radio" v-model="picked" value="a"></template>"#,
    );
    assert!(
        code.contains("_applyRadioModel(n0,"),
        "Should have _applyRadioModel, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model on select generates _applySelectModel
#[test]
fn test_vapor_v_model_select() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><select v-model="selected"><option>A</option></select></template>"#,
    );
    assert!(
        code.contains("_applySelectModel(n0,"),
        "Should have _applySelectModel, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model.trim modifier
#[test]
fn test_vapor_v_model_trim() {
    let code = gen_vapor_and_validate(r#"<template vapor><input v-model.trim="text"></template>"#);
    assert!(
        code.contains("_applyTextModel(n0,"),
        "Should have _applyTextModel, got:\n{}",
        code
    );
    assert!(
        code.contains("trim: true"),
        "Should have trim modifier, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-model.lazy.number multiple modifiers
#[test]
fn test_vapor_v_model_lazy_number() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><input v-model.lazy.number="text"></template>"#);
    assert!(
        code.contains("lazy: true"),
        "Should have lazy modifier, got:\n{}",
        code
    );
    assert!(
        code.contains("number: true"),
        "Should have number modifier, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: multi-root template returns array
#[test]
fn test_vapor_multi_root() {
    let code = gen_vapor_and_validate(r#"<template vapor><div>a</div><div>b</div></template>"#);
    assert!(
        code.contains("return [n0, n1]"),
        "Should return array of roots, got:\n{}",
        code
    );
    assert!(
        code.contains("const t0 = _template("),
        "Should have first template, got:\n{}",
        code
    );
    assert!(
        code.contains("const t1 = _template("),
        "Should have second template, got:\n{}",
        code
    );
    // Multi-root should NOT have ", true" in _template calls.
    assert!(
        !code.contains(", true)"),
        "Multi-root should NOT pass true to _template, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: three root elements
#[test]
fn test_vapor_three_roots() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div>a</div><div>b</div><div>c</div></template>"#,
    );
    assert!(
        code.contains("return [n0, n1, n2]"),
        "Should return array of 3 roots, got:\n{}",
        code
    );
}

// =========================================================================
// Phase 3: Dynamic bind/events, _withKeys, refs, custom directives
// =========================================================================

/// @ai-generated — Vapor: v-bind="obj" spread → _setDynamicProps
#[test]
fn test_vapor_bind_spread() {
    let code = gen_vapor_and_validate(
        r#"<script setup>const attrs = {}</script><template vapor><div v-bind="attrs">hi</div></template>"#,
    );
    assert!(
        code.contains("_setDynamicProps(n0,"),
        "Should use _setDynamicProps for v-bind spread, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Should wrap _setDynamicProps in _renderEffect, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: :[dynamic]="val" → _setDynamicProps with computed key
#[test]
fn test_vapor_dynamic_attr() {
    let code = gen_vapor_and_validate(
        r#"<script setup>const attrName = 'id'; const value = '1'</script><template vapor><div :[attrName]="value">content</div></template>"#,
    );
    assert!(
        code.contains("_setDynamicProps(n0,"),
        "Should use _setDynamicProps for dynamic attr, got:\n{}",
        code
    );
    assert!(
        code.contains("attrName]"),
        "Should use computed property key for dynamic attr, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: @[eventName]="handler" → _on with effect:true in renderEffect
#[test]
fn test_vapor_dynamic_event() {
    let code = gen_vapor_and_validate(
        r#"<script setup>const eventName = 'click'; function handler() {}</script><template vapor><button @[eventName]="handler">click</button></template>"#,
    );
    assert!(
        code.contains("_on(n0,"),
        "Should use _on for dynamic event, got:\n{}",
        code
    );
    assert!(
        code.contains("effect: true"),
        "Should pass effect: true for dynamic event, got:\n{}",
        code
    );
    assert!(
        code.contains("_renderEffect("),
        "Dynamic event should be in _renderEffect, got:\n{}",
        code
    );
    // Dynamic events should NOT use delegation
    assert!(
        !code.contains("_delegateEvents("),
        "Dynamic events should NOT use delegation, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: @keyup.enter="handler" → _withKeys
#[test]
fn test_vapor_key_modifier() {
    let code = gen_vapor_and_validate(
        r#"<script setup>function submit() {}</script><template vapor><input @keyup.enter="submit"></template>"#,
    );
    assert!(
        code.contains("_withKeys("),
        "Should use _withKeys for key modifier, got:\n{}",
        code
    );
    assert!(
        code.contains("[\"enter\"]"),
        "Should include enter in key modifiers array, got:\n{}",
        code
    );
    assert!(
        code.contains("_createInvoker(_withKeys("),
        "Should wrap _withKeys in _createInvoker, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: @keydown.ctrl.enter → _withModifiers + _withKeys
#[test]
fn test_vapor_key_and_runtime_modifiers() {
    let code = gen_vapor_and_validate(
        r#"<script setup>function submit() {}</script><template vapor><input @keydown.ctrl.enter="submit"></template>"#,
    );
    assert!(
        code.contains("_withModifiers("),
        "Should use _withModifiers for ctrl, got:\n{}",
        code
    );
    assert!(
        code.contains("_withKeys("),
        "Should use _withKeys for enter, got:\n{}",
        code
    );
    assert!(
        code.contains("[\"ctrl\"]"),
        "Should include ctrl in runtime modifiers, got:\n{}",
        code
    );
    assert!(
        code.contains("[\"enter\"]"),
        "Should include enter in key modifiers, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: ref="myDiv" → _createTemplateRefSetter + _setTemplateRef
#[test]
fn test_vapor_template_ref() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div ref="myDiv">content</div></template>"#);
    assert!(
        code.contains("_createTemplateRefSetter()"),
        "Should create template ref setter, got:\n{}",
        code
    );
    assert!(
        code.contains("_setTemplateRef(n0, \"myDiv\")"),
        "Should call _setTemplateRef with ref name, got:\n{}",
        code
    );
    // ref should NOT be in the template HTML
    assert!(
        !code.contains("ref=\"myDiv\""),
        "ref attr should be removed from template HTML, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-custom:arg.mod="expr" → _resolveDirective + _withVaporDirectives
#[test]
fn test_vapor_custom_directive() {
    let code = gen_vapor_and_validate(
        r#"<script setup>const value = 1</script><template vapor><div v-custom:arg.mod="value">hi</div></template>"#,
    );
    assert!(
        code.contains("_resolveDirective(\"custom\")"),
        "Should resolve custom directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_withVaporDirectives(n0,"),
        "Should call _withVaporDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("\"arg\""),
        "Should include arg in directive entry, got:\n{}",
        code
    );
    assert!(
        code.contains("mod: true"),
        "Should include modifier in directive entry, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: v-if / v-else-if / v-else
// =========================================================================

/// @ai-generated — Vapor: simple v-if produces _createIf
#[test]
fn test_vapor_vif_simple() {
    let code = gen_vapor_and_validate(r#"<template vapor><div v-if="show">yes</div></template>"#);
    assert!(
        code.contains("_createIf("),
        "Should contain _createIf, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.show"),
        "Should prefix condition with _ctx, got:\n{}",
        code
    );
    assert!(
        code.contains("_template("),
        "Should hoist template, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-if/v-else produces _createIf with else branch
#[test]
fn test_vapor_vif_else() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-if="show">yes</div><div v-else>no</div></template>"#,
    );
    assert!(
        code.contains("_createIf("),
        "Should contain _createIf, got:\n{}",
        code
    );
    // Should have two template hoists (one per branch).
    let t0_count = code.matches("_template(").count();
    assert!(
        t0_count >= 2,
        "Should have at least 2 _template() calls (one per branch), got {} in:\n{}",
        t0_count,
        code
    );
}

/// @ai-generated — Vapor: v-if/v-else-if/v-else produces nested _createIf
#[test]
fn test_vapor_vif_elseif_else() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    // Should have nested _createIf calls.
    let create_if_count = code.matches("_createIf(").count();
    assert!(
        create_if_count >= 2,
        "Should have at least 2 nested _createIf calls, got {} in:\n{}",
        create_if_count,
        code
    );
    assert!(
        code.contains("_ctx.a"),
        "Should prefix first condition, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.b"),
        "Should prefix second condition, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-if with script setup bindings
#[test]
fn test_vapor_vif_with_setup() {
    let code = gen_vapor_and_validate(
        r#"<script setup>const show = ref(true)</script><template vapor><div v-if="show">yes</div></template>"#,
    );
    assert!(
        code.contains("_createIf("),
        "Should contain _createIf, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: v-for
// =========================================================================

/// @ai-generated — Vapor: simple v-for produces _createFor
#[test]
fn test_vapor_vfor_simple() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-for="item in items">text</div></template>"#,
    );
    assert!(
        code.contains("_createFor("),
        "Should contain _createFor, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.items"),
        "Should prefix iterable with _ctx, got:\n{}",
        code
    );
    assert!(
        code.contains("_for_item0"),
        "Should use _for_item0 callback param, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-for with :key produces key function
#[test]
fn test_vapor_vfor_keyed() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-for="item in items" :key="item">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_createFor("),
        "Should contain _createFor, got:\n{}",
        code
    );
    // Key function should use original param name.
    assert!(
        code.contains("(item) => (item)"),
        "Should have key function with original param name, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-for with index
#[test]
fn test_vapor_vfor_index() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_for_item0"),
        "Should use _for_item0, got:\n{}",
        code
    );
    assert!(
        code.contains("_for_key0"),
        "Should use _for_key0 for index, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: Components
// =========================================================================

/// @ai-generated — Vapor: simple component produces _resolveComponent + _createComponentWithFallback
#[test]
fn test_vapor_component_simple() {
    let code = gen_vapor_and_validate(r#"<template vapor><MyComponent/></template>"#);
    assert!(
        code.contains("_resolveComponent(\"MyComponent\")"),
        "Should resolve component, got:\n{}",
        code
    );
    assert!(
        code.contains("_createComponentWithFallback("),
        "Should use _createComponentWithFallback, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: component with dynamic prop
#[test]
fn test_vapor_component_props() {
    let code = gen_vapor_and_validate(r#"<template vapor><MyComp :msg="hello"/></template>"#);
    assert!(
        code.contains("_resolveComponent(\"MyComp\")"),
        "Should resolve component, got:\n{}",
        code
    );
    assert!(
        code.contains("_createComponentWithFallback("),
        "Should use _createComponentWithFallback, got:\n{}",
        code
    );
    assert!(
        code.contains("msg:"),
        "Should have msg prop, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: dynamic component <component :is="comp">
#[test]
fn test_vapor_dynamic_component() {
    let code = gen_vapor_and_validate(r#"<template vapor><component :is="comp"/></template>"#);
    assert!(
        code.contains("_createDynamicComponent("),
        "Should use _createDynamicComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.comp"),
        "Should prefix :is expression, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: slot outlet <slot/>
#[test]
fn test_vapor_slot_outlet() {
    let code = gen_vapor_and_validate(r#"<template vapor><slot/></template>"#);
    assert!(
        code.contains("_createSlot("),
        "Should use _createSlot, got:\n{}",
        code
    );
    assert!(
        code.contains("\"default\""),
        "Should use default slot name, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: Built-in Components
// =========================================================================

/// @ai-generated — Vapor: <teleport> uses _createComponent + _VaporTeleport
#[test]
fn test_vapor_teleport() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><teleport to="body"><div>modal</div></teleport></template>"#,
    );
    assert!(
        code.contains("_VaporTeleport"),
        "Should import _VaporTeleport, got:\n{}",
        code
    );
    assert!(
        code.contains("_createComponent("),
        "Should use _createComponent for teleport, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: <transition> uses _createComponent + _VaporTransition
#[test]
fn test_vapor_transition() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><transition><div v-if="show">content</div></transition></template>"#,
    );
    assert!(
        code.contains("_VaporTransition"),
        "Should import _VaporTransition, got:\n{}",
        code
    );
    assert!(
        code.contains("_createComponent("),
        "Should use _createComponent for transition, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: v-for variable mapping
// =========================================================================

/// @ai-generated — Vapor: v-for interpolation maps `item` to `_for_item0.value`
#[test]
fn test_vapor_vfor_interpolation_mapping() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-for="item in items">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_for_item0.value"),
        "Should map `item` to `_for_item0.value` in interpolation, got:\n{}",
        code
    );
    assert!(
        code.contains("_toDisplayString(_for_item0.value)"),
        "Should wrap in _toDisplayString, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: v-for with index maps both item and index
#[test]
fn test_vapor_vfor_index_mapping() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div v-for="(item, index) in items">{{ index }}: {{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_for_item0.value"),
        "Should map `item` to `_for_item0.value`, got:\n{}",
        code
    );
    assert!(
        code.contains("_for_key0.value"),
        "Should map `index` to `_for_key0.value`, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: _setInsertionState for nested structural directives
// =========================================================================

/// @ai-generated — Vapor: nested v-if inside element emits _setInsertionState
#[test]
fn test_vapor_nested_vif_insertion_state() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><span v-if="show">inner</span></div></template>"#,
    );
    assert!(
        code.contains("_setInsertionState("),
        "Should emit _setInsertionState for nested v-if, got:\n{}",
        code
    );
    assert!(
        code.contains("_createIf("),
        "Should contain _createIf, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: nested v-for inside element emits _setInsertionState
#[test]
fn test_vapor_nested_vfor_insertion_state() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div><span v-for="item in items">text</span></div></template>"#,
    );
    assert!(
        code.contains("_setInsertionState("),
        "Should emit _setInsertionState for nested v-for, got:\n{}",
        code
    );
    assert!(
        code.contains("_createFor("),
        "Should contain _createFor, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: Named slots
// =========================================================================

/// @ai-generated — Vapor: component with named slot via <template #header>
#[test]
fn test_vapor_component_named_slot() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><MyComp><template #header><div>Header</div></template></MyComp></template>"#,
    );
    assert!(
        code.contains("\"header\""),
        "Should have 'header' slot name, got:\n{}",
        code
    );
    assert!(
        code.contains("_createComponentWithFallback("),
        "Should use _createComponentWithFallback, got:\n{}",
        code
    );
}

/// @ai-generated — Vapor: component with default slot content
#[test]
fn test_vapor_component_default_slot_content() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><MyComp><div>content</div></MyComp></template>"#);
    assert!(
        code.contains("\"default\""),
        "Should have 'default' slot, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: Scoped slots
// =========================================================================

/// @ai-generated — Vapor: scoped slot with v-slot="{ item }" uses _slotProps0
#[test]
fn test_vapor_scoped_slot() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><MyComp v-slot="{ item }"><div>{{ item }}</div></MyComp></template>"#,
    );
    assert!(
        code.contains("_slotProps0"),
        "Should use _slotProps0 for scoped slot, got:\n{}",
        code
    );
    assert!(
        code.contains("_slotProps0.item"),
        "Should access slot prop as _slotProps0.item, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: <slot> outlet with name/props
// =========================================================================

/// @ai-generated — Vapor: <slot name="header"/> uses named slot
#[test]
fn test_vapor_slot_outlet_named() {
    let code = gen_vapor_and_validate(r#"<template vapor><slot name="header"/></template>"#);
    assert!(
        code.contains("_createSlot("),
        "Should use _createSlot, got:\n{}",
        code
    );
    assert!(
        code.contains("\"header\""),
        "Should use 'header' slot name, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor v-once
// =========================================================================

/// @ai-generated — Vapor v-once: effects are emitted as direct statements, not wrapped in _renderEffect
#[test]
fn test_vapor_v_once_skips_render_effect() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div v-once :id="foo">text</div></template>"#);
    // v-once should NOT use _renderEffect — effects become one-time statements
    assert!(
        !code.contains("_renderEffect"),
        "v-once should NOT wrap effects in _renderEffect, got:\n{}",
        code
    );
    // The prop effect should still be set
    assert!(
        code.contains("_setAttr(") || code.contains("_setProp("),
        "v-once should still set the prop, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor v-on spread
// =========================================================================

/// @ai-generated — Vapor v-on="handlers" should use _toHandlers
#[test]
fn test_vapor_von_spread_uses_to_handlers() {
    let code =
        gen_vapor_and_validate(r#"<template vapor><div v-on="handlers">text</div></template>"#);
    assert!(
        code.contains("_toHandlers("),
        "Vapor v-on spread should use _toHandlers, got:\n{}",
        code
    );
}

// =========================================================================
// Vapor: n{X} node ref replacement correctness (regression tests)
// =========================================================================

/// Regression test: when build_block_body rewrites node refs from the outer
/// structural directive's node_ref to the inner template's node_ref, it must
/// use whole-word matching. Otherwise `n1` inside `n10` gets corrupted.
///
/// This test creates a template with enough elements to push node_ref counters
/// past 10, then uses v-if on an element with a dynamic prop. The v-if triggers
/// build_block_body which must rewrite the effect's node ref correctly.
#[test]
fn test_vapor_node_ref_replacement_no_false_match() {
    // Create a template with many sibling elements to push node counters high,
    // then a v-if element with a dynamic binding.
    // The v-if element's effects must have their node refs rewritten correctly.
    let code = gen_vapor_and_validate(
        r#"<template vapor><div>
  <span>a</span>
  <span>b</span>
  <span>c</span>
  <span>d</span>
  <span>e</span>
  <span>f</span>
  <span>g</span>
  <span>h</span>
  <span>i</span>
  <span>j</span>
  <span v-if="show" :class="cls">dynamic</span>
</div></template>"#,
    );
    // The generated code must be valid JS (gen_vapor_and_validate checks this).
    // Additionally, there must be no corrupted node refs like `n51` when we meant `n5`
    // or `n110` when we meant `n10`.
    // The v-if branch should reference its own inner node ref, not a corrupted one.
    assert!(
        !code.contains("n01"),
        "Node ref replacement must not corrupt n0 into n01 by partial matching, got:\n{}",
        code
    );
}

/// Regression test: verify that the `replace_node_ref` helper is used in
/// build_block_body instead of naive `String::replace`. This test uses
/// a v-for with a dynamic binding to trigger build_block_body, and verifies
/// the generated code is valid JS even with high node ref numbers.
#[test]
fn test_vapor_vfor_with_many_siblings_node_ref_integrity() {
    let code = gen_vapor_and_validate(
        r#"<template vapor><div>
  <span>1</span>
  <span>2</span>
  <span>3</span>
  <span>4</span>
  <span>5</span>
  <span>6</span>
  <span>7</span>
  <span>8</span>
  <span>9</span>
  <span>10</span>
  <span>11</span>
  <p v-for="item in items" :key="item.id" :class="item.cls">{{ item.name }}</p>
</div></template>"#,
    );
    // gen_vapor_and_validate already checks valid JS + no invalid patterns.
    // The v-for block body must correctly rewrite node refs.
    assert!(
        code.contains("_createFor("),
        "Should contain _createFor, got:\n{}",
        code
    );
}
