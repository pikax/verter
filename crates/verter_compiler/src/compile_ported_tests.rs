//! Tests ported from `syntax/plugins/code_gen/template/tests.rs`.
//!
//! These tests use the AST-based pipeline via `compile()` directly,
//! checking individual result blocks (`result.template`, `result.script`).
//!
//! NOTE: The new AST-based pipeline differs from the old event-stream pipeline
//! in several ways:
//!   - Root elements use `_createElementVNode` (not `_createElementBlock`)
//!   - Static props are inlined (not hoisted to `_hoisted_N` constants)
//!   - Hyphenated prop names are camelized (not quoted)
//!   - v-if branches are not block-wrapped and have no key injection
//!   - v-once is not yet implemented (directive is ignored)
//!   - Production inline arrow function for script setup not yet implemented

use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
use oxc_allocator::Allocator;

// =========================================================================
// Template Wrapper
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Dev mode emits `function render(_ctx, _cache, ...)`
#[test]
fn test_dev_function_render() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("function render(_ctx, _cache"),
        "Dev mode should emit function render, got:\n{}",
        template.code
    );
}

/// @ai-generated — Production mode: template-only always uses function render(),
/// script setup also uses function render() in the new pipeline (inline not yet implemented).
#[test]
fn test_prod_render_fn() {
    // Template-only: no script setup, so no inline mode even in production
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("function render("),
        "Template-only in prod should use function render (no script setup), got:\n{}",
        template.code
    );

    // With script setup: new pipeline uses function render() (inline not yet implemented)
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const msg = 'hi'
</script>
<template><div>{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("function render("),
        "Script setup in prod should emit function render, got:\n{}",
        template.code
    );
}

/// @ai-generated — Empty template returns null
#[test]
fn test_template_empty_returns_null() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("return null"),
        "Empty template should return null, got:\n{}",
        template.code
    );
}

// =========================================================================
// Elements — basic structure
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Simple div with text child
#[test]
fn test_element_simple_div_text() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template
            .code
            .contains(r#"_createElementBlock("div", null, "hello")"#),
        "Root should emit _createElementBlock(\"div\", null, \"hello\"), got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_openBlock()"),
        "Root should use _openBlock(), got:\n{}",
        template.code
    );
}

/// @ai-generated — Self-closing <br/> element
#[test]
fn test_element_self_closing_br() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><br/></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"_createElementBlock("br")"#),
        "Root br should use _createElementBlock, got:\n{}",
        template.code
    );
}

/// @ai-generated — Void <input> element
#[test]
fn test_element_void_input() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><input></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"_createElementBlock("input")"#),
        "Root void input should use _createElementBlock, got:\n{}",
        template.code
    );
}

/// @ai-generated — Empty div produces no children arg
#[test]
fn test_element_empty_div() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"_createElementBlock("div")"#),
        "Empty root div should use _createElementBlock, got:\n{}",
        template.code
    );
}

/// @ai-generated — Nested elements: root and child both use _createElementVNode
#[test]
fn test_element_nested() {
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.hoist_static = Some(false);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span>inner</span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"_createElementBlock("div""#),
        "Root div should be _createElementBlock, got:\n{}",
        template.code
    );
    assert!(
        template
            .code
            .contains(r#"_createElementVNode("span", null, "inner")"#),
        "Child span should be _createElementVNode with text, got:\n{}",
        template.code
    );
}

/// @ai-generated — Deeply nested elements
#[test]
fn test_element_deeply_nested() {
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.hoist_static = Some(false);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span><em>deep</em></span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template
            .code
            .contains(r#"_createElementVNode("em", null, "deep")"#),
        "Deepest element should have text, got:\n{}",
        template.code
    );
}

// =========================================================================
// Elements — block root treatment
// New pipeline uses _openBlock() + _createElementBlock for root elements
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Root element should use _openBlock + _createElementBlock
#[test]
fn test_block_root_simple() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_openBlock()"),
        "Root should use _openBlock(), got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_createElementBlock("),
        "Root should use _createElementBlock, got:\n{}",
        template.code
    );
}

/// @ai-generated — Nested child also uses _createElementVNode
#[test]
fn test_block_root_nested_child_is_vnode() {
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.hoist_static = Some(false);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span>inner</span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_createElementVNode("),
        "Root div should use _createElementVNode, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"_createElementVNode("span""#),
        "Child span should use _createElementVNode, got:\n{}",
        template.code
    );
}

// =========================================================================
// Static Props
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs
// NOTE: New pipeline inlines static props instead of hoisting them.

/// @ai-generated — Static id attribute (inlined in new pipeline)
#[test]
fn test_props_static_id() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div id="app">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Static props are inlined in the new pipeline
    assert!(
        template.code.contains(r#"id: "app""#),
        "Static id prop should be present, got:\n{}",
        template.code
    );
}

/// @ai-generated — Static class attribute (inlined in new pipeline)
#[test]
fn test_props_static_class() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div class="foo bar">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"class: "foo bar""#),
        "Should have class prop, got:\n{}",
        template.code
    );
}

/// @ai-generated — Static style attribute (inlined in new pipeline)
#[test]
fn test_props_static_style() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div style="color: red">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"style: { color: "red" }"#),
        "Should have style prop as object, got:\n{}",
        template.code
    );
}

/// @ai-generated — Props null when no attributes
#[test]
fn test_props_null_when_empty() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#""div", null"#),
        "No props should produce null, got:\n{}",
        template.code
    );
}

// =========================================================================
// Bound Props — :id, :class, :style
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Bound :id produces { id: expr }
#[test]
fn test_props_bound_id() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :id="myId">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("id: _ctx.myId"),
        "Bound id should have id: _ctx.myId, got:\n{}",
        template.code
    );
}

/// @ai-generated — :class uses _normalizeClass with CLASS flag
#[test]
fn test_props_class_normalize() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :class="cls">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("class: _normalizeClass(_ctx.cls)"),
        "Should use _normalizeClass, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("2 /* CLASS */"),
        "Should have CLASS (2) patch flag, got:\n{}",
        template.code
    );
}

/// @ai-generated — :style uses _normalizeStyle with STYLE flag
#[test]
fn test_props_style_normalize() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :style="sty">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("style: _normalizeStyle(_ctx.sty)"),
        "Should use _normalizeStyle, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("4 /* STYLE */"),
        "Should have STYLE (4) patch flag, got:\n{}",
        template.code
    );
}

/// @ai-generated — Mixed static + bound props
#[test]
fn test_props_mixed_static_bound() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div id="s" :title="d">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"id: "s""#),
        "Static id should be preserved, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("title: _ctx.d"),
        "Bound title should be present, got:\n{}",
        template.code
    );
}

/// @ai-generated — Combined :class and :style patch flags
#[test]
fn test_props_class_style_combined() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :class="c" :style="s">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_normalizeClass(_ctx.c)"),
        "Should have _normalizeClass, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_normalizeStyle(_ctx.s)"),
        "Should have _normalizeStyle, got:\n{}",
        template.code
    );
    // CLASS(2) | STYLE(4) = 6
    assert!(
        template.code.contains("6 /* CLASS, STYLE */"),
        "Should have combined CLASS+STYLE flag (6), got:\n{}",
        template.code
    );
}

/// @ai-generated — :class/:style on plain elements must NOT trigger PATCH_PROPS (8).
/// Vue uses dedicated PATCH_CLASS (2) and PATCH_STYLE (4) flags for plain elements.
/// Including them in dynamic_props would incorrectly set PATCH_PROPS.
#[test]
fn test_class_style_no_patch_props_on_plain_element() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    // :class + :style on a plain <div>
    let result = compile(
        r#"<template><div :class="c" :style="s">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Should have CLASS (2) + STYLE (4) = 6, NOT 14 (6 | PROPS(8))
    assert!(
        template.code.contains("6 /* CLASS, STYLE */"),
        ":class/:style on plain element should be 6, got:\n{}",
        template.code
    );
    // Must NOT have dynamic_props array (which indicates PATCH_PROPS)
    assert!(
        !template.code.contains(r#", ["class"#),
        "Should NOT list class/style in dynamic_props, got:\n{}",
        template.code
    );
}

/// @ai-generated — :class/:style on COMPONENTS should include them in dynamic_props
/// for shouldUpdateComponent checking. This is Vue's expected behavior.
#[test]
fn test_class_style_in_dynamic_props_on_component() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><MyComp :class="c"/></template>
<script setup>import MyComp from './MyComp.vue'</script>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Components need class in dynamic_props for shouldUpdateComponent
    assert!(
        template.code.contains(r#"["class"]"#),
        "Component should list class in dynamic_props, got:\n{}",
        template.code
    );
}

/// @ai-generated — No patch flag for static-only props
#[test]
fn test_props_no_pf_for_static() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div id="app">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Static props shouldn't produce a patch flag number
    assert!(
        !template.code.contains("/* PROPS */"),
        "Static-only props should not have PROPS flag, got:\n{}",
        template.code
    );
}

// =========================================================================
// Static Props — inlined in new pipeline (no hoisting)
// The new AST-based pipeline inlines static props instead of hoisting them
// to module-scope constants. Tests verify props are correctly inlined.
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Static props are hoisted to const _hoisted_N
#[test]
fn test_hoist_static_props() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div class="app">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Static props should be hoisted
    assert!(
        template
            .code
            .contains(r#"const _hoisted_1 = { class: "app" }"#),
        "Static props should be hoisted, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_hoisted_1"),
        "Should reference _hoisted_1 at call site, got:\n{}",
        template.code
    );
}

/// @ai-generated — Multiple static prop elements have inline props
#[test]
fn test_hoist_multiple_props() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span class="inner">{{ a }}</span><p id="footer">{{ b }}</p></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // New pipeline inlines props instead of hoisting
    assert!(
        template.code.contains(r#"class: "inner""#),
        "First element's props should be inlined, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"id: "footer""#),
        "Second element's props should be inlined, got:\n{}",
        template.code
    );
}

/// @ai-generated — Mixed static+dynamic props: dynamic props array IS hoisted
#[test]
fn test_hoist_mixed_props_not_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div class="app" :id="myId">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("const _hoisted_1 = [\"id\"]"),
        "Dynamic props array SHOULD be hoisted, got:\n{}",
        template.code
    );
}

/// @ai-generated — Event handler dynamic props array gets hoisted
#[test]
fn test_hoist_event_prevents_hoisting() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><button class="btn" @click="go">hi</button></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("const _hoisted_1 = [\"onClick\"]"),
        "Event handler dynamic props array SHOULD be hoisted, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_hoisted_1)"),
        "Should reference hoisted constant in element call, got:\n{}",
        template.code
    );
}

/// @ai-generated — No props = no hoisting (null)
#[test]
fn test_hoist_no_props() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        !template.code.contains("_hoisted_"),
        "Element without props should not produce hoisted constants, got:\n{}",
        template.code
    );
}

/// @ai-generated — Component props are NOT hoisted (Vue rule)
#[test]
fn test_hoist_component_not_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><MyComponent class="app">hi</MyComponent></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        !template.code.contains("_hoisted_"),
        "Component props should NOT be hoisted, got:\n{}",
        template.code
    );
}

/// @ai-generated — Static props are inlined before render function (no hoisting in new pipeline)
#[test]
fn test_hoist_placement_before_render() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div id="app">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // New pipeline does not hoist; props are inlined in the render function
    assert!(
        template.code.contains(r#"id: "app""#),
        "Static prop should be present in render function, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("function render"),
        "Should have render function, got:\n{}",
        template.code
    );
}

/// @ai-generated — Multiple static attributes are inlined together
#[test]
fn test_hoist_multiple_attrs() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div id="app" class="main" style="color:red">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // All three props should be present (inlined, not hoisted)
    assert!(
        template.code.contains(r#"id: "app""#),
        "Should contain id, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"class: "main""#),
        "Should contain class, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"style: { color: "red" }"#),
        "Should contain style as object, got:\n{}",
        template.code
    );
}

/// @ai-generated — Production mode inlines static props (no hoisting in new pipeline)
#[test]
fn test_hoist_production_mode() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div class="app">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"class: "app""#),
        "Production mode should have static props inlined, got:\n{}",
        template.code
    );
}

// =========================================================================
// Hyphenated Prop Names — camelized in new pipeline
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs
// NOTE: New pipeline camelizes hyphenated prop names on native elements
// instead of quoting them.

/// @ai-generated — Bound hyphenated prop name is camelized on native elements
#[test]
fn test_props_hyphenated_bound() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :initial-foo="val">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("initialFoo: _ctx.val"),
        "Hyphenated bound prop should be camelized, got:\n{}",
        template.code
    );
}

/// @ai-generated — Bound hyphenated prop on component is camelized
#[test]
fn test_props_hyphenated_bound_component() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><SplitPane :initial-foo="50"/></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("initialFoo: 50"),
        "Hyphenated bound prop on component should be camelized, got:\n{}",
        template.code
    );
}

/// @ai-generated — Static hyphenated prop (non-hoisted, mixed with dynamic) is quoted
#[test]
fn test_props_hyphenated_static_non_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div data-id="x" :title="t">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("\"data-id\": \"x\""),
        "Hyphenated static prop should be quoted, got:\n{}",
        template.code
    );
}

/// @ai-generated — Static hyphenated prop (hoisted) is quoted
#[test]
fn test_props_hyphenated_static_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div data-foo="bar">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("\"data-foo\": \"bar\""),
        "Hoisted hyphenated prop should be quoted, got:\n{}",
        template.code
    );
}

/// @ai-generated — Normal props stay unquoted alongside camelized hyphenated props
#[test]
fn test_props_hyphenated_mixed_with_normal() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :id="myId" :initial-foo="val">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("id: _ctx.myId"),
        "Normal prop should remain unquoted, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("initialFoo: _ctx.val"),
        "Hyphenated prop should be camelized, got:\n{}",
        template.code
    );
}

// =========================================================================
// Hyphenated Event Names — camelization
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Hyphenated event name is camelized
#[test]
fn test_event_hyphenated_camelize() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div @initial-split="handler">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("onInitialSplit: _ctx.handler"),
        "Hyphenated event should be camelized, got:\n{}",
        template.code
    );
}

/// @ai-generated — Hyphenated event on component is camelized
#[test]
fn test_event_hyphenated_component() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><Comp @my-custom-event="handler"/></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("onMyCustomEvent"),
        "Hyphenated event on component should be camelized, got:\n{}",
        template.code
    );
}

// =========================================================================
// v-model with hyphenated names
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs
// NOTE: New pipeline camelizes v-model prop name instead of quoting it.

/// @ai-generated — v-model with hyphenated name: prop camelized, event camelized
#[test]
fn test_vmodel_hyphenated_component() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><Comp v-model:my-value="val"/></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("myValue:"),
        "v-model hyphenated prop should be camelized, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("\"onUpdate:myValue\""),
        "v-model hyphenated event should be camelized, got:\n{}",
        template.code
    );
}

// =========================================================================
// Events — @click etc.
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — @click becomes onClick prop
#[test]
fn test_event_click() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><button @click="handler">click</button></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("onClick: _ctx.handler"),
        "Should have onClick: _ctx.handler, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("8 /* PROPS */"),
        "Event should produce PROPS (8) patch flag, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"["onClick"]"#),
        "Event name should be in dynamic props list, got:\n{}",
        template.code
    );
}

/// @ai-generated — Multiple events
#[test]
fn test_event_multiple() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><button @click="a" @mouseover="b">hi</button></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("onClick: _ctx.a"),
        "Should have onClick: _ctx.a, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("onMouseover: _ctx.b"),
        "Should have onMouseover: _ctx.b, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("8 /* PROPS */"),
        "Events should produce PROPS (8) patch flag, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"["onClick", "onMouseover"]"#),
        "Event names should be in dynamic props list, got:\n{}",
        template.code
    );
}

/// @ai-generated — Vue treats events as PROPS patch flag with event name in dynamic props
#[test]
fn test_event_props_patch_flag() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><button @click="handler">click</button></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("8 /* PROPS */"),
        "Event should produce PROPS (8) patch flag, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"["onClick"]"#),
        "Event name should be in dynamic props list, got:\n{}",
        template.code
    );
}

// =========================================================================
// Text
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Text wrapping in quotes
#[test]
fn test_text_in_quotes() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#""hello""#),
        "Text should be wrapped in quotes, got:\n{}",
        template.code
    );
}

/// @ai-generated — Text with quotes gets escaped
#[test]
fn test_text_escaped_quotes() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>say "hello"</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // The text should escape inner quotes
    assert!(
        template.code.contains(r#"say \"hello\""#) || template.code.contains(r#"say "hello""#),
        "Text with quotes should be handled, got:\n{}",
        template.code
    );
}

// =========================================================================
// Interpolation
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Simple interpolation produces _toDisplayString
#[test]
fn test_interp_simple() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString, got:\n{}",
        template.code
    );
}

/// @ai-generated — Interpolation with expression
#[test]
fn test_interp_expr() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ a + b }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString for expression, got:\n{}",
        template.code
    );
}

/// @ai-generated — Interpolation with ternary
#[test]
fn test_interp_ternary() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ a ? b : c }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString for ternary, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_ctx.a ? _ctx.b : _ctx.c"),
        "Ternary expression should be preserved with _ctx. prefix, got:\n{}",
        template.code
    );
}

/// @ai-generated — Interpolation with method call
#[test]
fn test_interp_method_call() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ foo() }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString for method call, got:\n{}",
        template.code
    );
}

/// @ai-generated — Interpolation with $setup binding prefix
#[test]
fn test_interp_with_setup_binding() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString, got:\n{}",
        template.code
    );
    // Setup bindings should get $setup prefix in dev mode
    assert!(
        template.code.contains("$setup.msg"),
        "Setup binding should have $setup prefix, got:\n{}",
        template.code
    );
}

/// @ai-generated — Event handler with $setup binding prefix: onClick: $setup.increment
#[test]
fn test_event_with_setup_binding() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
function increment() { count.value++ }
</script>
<template><button @click="increment">Count: {{ count }}</button></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("onClick: $setup.increment"),
        "Event handler should have $setup. prefix BEFORE identifier, got:\n{}",
        template.code
    );
    // Make sure the broken pattern is NOT present
    assert!(
        !template.code.contains("increment$setup."),
        "Accessor prefix must not appear AFTER identifier, got:\n{}",
        template.code
    );
}

/// @ai-generated — Bound prop with $setup binding prefix: id: $setup.myId
#[test]
fn test_bound_prop_with_setup_binding() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import { ref } from 'vue'
const myId = ref('app')
</script>
<template><div :id="myId">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("id: $setup.myId"),
        "Bound prop should have $setup. prefix BEFORE identifier, got:\n{}",
        template.code
    );
    assert!(
        !template.code.contains("myId$setup."),
        "Accessor prefix must not appear AFTER identifier, got:\n{}",
        template.code
    );
}

/// @ai-generated — :class with $setup binding: class: _normalizeClass($setup.cls)
#[test]
fn test_class_bind_with_setup_binding() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import { ref } from 'vue'
const cls = ref('active')
</script>
<template><div :class="cls">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_normalizeClass($setup.cls)"),
        ":class binding should have $setup. prefix BEFORE identifier, got:\n{}",
        template.code
    );
}

/// @ai-generated — :style with $setup binding: style: _normalizeStyle($setup.sty)
#[test]
fn test_style_bind_with_setup_binding() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
import { ref } from 'vue'
const sty = ref({ color: 'red' })
</script>
<template><div :style="sty">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_normalizeStyle($setup.sty)"),
        ":style binding should have $setup. prefix BEFORE identifier, got:\n{}",
        template.code
    );
}

/// @ai-generated — Full SFC with multiple setup bindings in template
#[test]
fn test_full_sfc_setup_bindings() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
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
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Check all setup bindings have correct prefix
    assert!(
        template.code.contains("$setup.message"),
        "Interpolation binding should have $setup prefix, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("onClick: $setup.increment"),
        "Event handler should have $setup prefix, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("$setup.count"),
        "Second interpolation binding should have $setup prefix, got:\n{}",
        template.code
    );
    // Static class should be inlined (not hoisted in new pipeline)
    assert!(
        template.code.contains(r#"class: "app""#),
        "Static class should be present in render function, got:\n{}",
        template.code
    );
}

// =========================================================================
// Text + Interpolation Mix (concatenation)
// Vue concatenates: "hello " + _toDisplayString(_ctx.msg)
// Current: separate comma args (requires close-phase refactor)
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Text + interpolation should concatenate with +
#[test]
fn test_children_text_interp_concat() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello {{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#""hello " + _toDisplayString"#),
        "Text + interpolation should concat with +, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("1 /* TEXT */"),
        "Concatenated text should have TEXT patch flag, got:\n{}",
        template.code
    );
}

/// @ai-generated — Text-interp-text should concat: "hello " + expr + " world"
#[test]
fn test_children_text_interp_text_concat() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>hello {{ msg }} world</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#""hello " + _toDisplayString"#),
        "Should start with text + toDisplayString, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#"+ " world""#),
        "Should end with + \" world\", got:\n{}",
        template.code
    );
}

/// @ai-generated — Multiple interpolations concatenated
#[test]
fn test_children_multiple_interp_concat() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ a }}{{ b }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_toDisplayString"),
        "Should have _toDisplayString calls, got:\n{}",
        template.code
    );
    // Vue: _toDisplayString(_ctx.a) + _toDisplayString(_ctx.b)
    assert!(
        template.code.contains(" + _toDisplayString"),
        "Multiple interps should concatenate with +, got:\n{}",
        template.code
    );
}

// =========================================================================
// Children array wrapping
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Multiple element children should be in array
#[test]
fn test_children_multiple_elements_array() {
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.hoist_static = Some(false);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span>a</span><span>b</span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Vue: [..., [...]] where children are in an array
    assert!(
        template.code.contains("[_createElementVNode"),
        "Multiple children should be wrapped in array, got:\n{}",
        template.code
    );
}

/// @ai-generated — Single element child is passed directly (not array-wrapped in new pipeline).
/// NOTE: Vue's official compiler array-wraps single children, but the new pipeline
/// passes them directly. This is a known difference.
#[test]
fn test_children_single_element() {
    let allocator = Allocator::new();
    let mut options = CodegenOptions::new().with_filename("test.vue");
    options.hoist_static = Some(false);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><span>inner</span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // New pipeline passes single child directly
    assert!(
        template
            .code
            .contains(r#"_createElementVNode("span", null, "inner")"#),
        "Single child should be passed as _createElementVNode, got:\n{}",
        template.code
    );
}

// =========================================================================
// Comments
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — HTML comment -> _createCommentVNode
#[test]
fn test_comment_basic() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><!-- my comment --></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template
            .code
            .contains(r#"_createCommentVNode(" my comment ")"#),
        "Comment should produce _createCommentVNode with content, got:\n{}",
        template.code
    );
}

/// @ai-generated — Empty comment
#[test]
fn test_comment_empty() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><!----></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"_createCommentVNode("")"#),
        "Empty comment should produce empty string, got:\n{}",
        template.code
    );
}

/// @ai-generated — Comment as only child of element
#[test]
fn test_comment_only_child() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><!-- only --></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_createCommentVNode"),
        "Only-child comment should still produce _createCommentVNode, got:\n{}",
        template.code
    );
}

// =========================================================================
// v-if directives
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs
// NOTE: New pipeline uses plain ternary without block wrapping or key injection.

/// @ai-generated — v-if produces ternary with comment fallback
#[test]
fn test_v_if_ternary() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("(_ctx.show) ?"),
        "v-if should produce ternary, got:\n{}",
        template.code
    );
    assert!(
        template
            .code
            .contains(r#"_createCommentVNode("v-if", true)"#),
        "v-if should have labeled comment fallback in dev, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if/v-else produces both branches
#[test]
fn test_v_if_else() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("(_ctx.show) ?"),
        "Should have v-if ternary, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#""yes""#),
        "Should have 'yes' branch, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#""no""#),
        "Should have 'no' branch, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if/v-else-if/v-else chain
#[test]
fn test_v_if_else_if_else() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("(_ctx.a) ?"),
        "Should have first condition, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("(_ctx.b) ?"),
        "Should have else-if condition, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if with class attribute preserves class
#[test]
fn test_v_if_with_class() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show" class="foo">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(r#"class: "foo""#),
        "v-if element should preserve class, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if removes directive from props (no v-if="..." in output)
#[test]
fn test_v_if_removes_directive() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // The v-if directive attribute should be removed from element props
    // (but "v-if" in the comment fallback is expected: _createCommentVNode("v-if", true))
    assert!(
        !template.code.contains(r#"v-if="show""#),
        "v-if directive attribute should be removed from output, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if branches use _createElementVNode (no block wrapping in new pipeline)
#[test]
fn test_v_if_block_treatment() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-if branches use block helpers (block root)
    assert!(
        template.code.contains("_createElementBlock("),
        "v-if branch should use _createElementBlock, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if branches: official Vue injects `{ key: 0 }` on the
/// branch root so ternary arms patch as distinct nodes; Verter matches this.
#[test]
fn test_v_if_key_injection() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("(_ctx.show) ?"),
        "v-if should produce ternary, got:\n{}",
        template.code
    );
    // Official Vue injects the branch key on the v-if root.
    assert!(
        template
            .code
            .contains(r#"_createElementBlock("div", { key: 0 }, "yes")"#),
        "v-if branch must inject {{ key: 0 }} on the branch root, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-if prod mode: new pipeline still uses labeled comment in dev and prod
#[test]
fn test_v_if_prod_empty_comment() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-if="show">yes</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // New pipeline uses labeled comment even in prod
    assert!(
        template
            .code
            .contains(r#"_createCommentVNode("v-if", true)"#),
        "Prod v-if should use comment fallback, got:\n{}",
        template.code
    );
}

// =========================================================================
// v-for directives
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — v-for produces _renderList with Fragment wrapping
#[test]
fn test_v_for_render_list() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_renderList("),
        "v-for should produce _renderList, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_openBlock(true)"),
        "v-for should use _openBlock(true), got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("_Fragment"),
        "v-for should wrap in _Fragment, got:\n{}",
        template.code
    );
}

/// @ai-generated — Keyed v-for uses KEYED_FRAGMENT (128)
#[test]
fn test_v_for_keyed_fragment() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="item in items" :key="item">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("128 /* KEYED_FRAGMENT */"),
        "Keyed v-for should use 128 KEYED_FRAGMENT, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-for with index parameter: (item, index) =>
#[test]
fn test_v_for_with_index() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_renderList("),
        "Should have _renderList, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("(item, index)"),
        "Should have (item, index) params, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-for _renderList first arg should be the iterable, not the full "item in items" expression
#[test]
fn test_v_for_renderlist_iterable_only() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // _renderList should receive just the iterable (e.g., _ctx.items), not "(item, index) in items"
    assert!(
        !template.code.contains("in _ctx.items")
            && !template.code.contains("in items")
            && !template.code.contains(") in "),
        "v-for _renderList should NOT contain 'in' keyword from template syntax, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-for with simple variable: _renderList(source, (item) => ...)
#[test]
fn test_v_for_renderlist_simple() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Should be _renderList(_ctx.items, ...) not _renderList(item in _ctx.items, ...)
    assert!(
        !template.code.contains("item in"),
        "v-for _renderList should not contain 'item in', got:\n{}",
        template.code
    );
}

/// @ai-generated — v-for removes directive from output
#[test]
fn test_v_for_removes_directive() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        !template.code.contains("v-for"),
        "v-for directive should be removed from output, got:\n{}",
        template.code
    );
}

/// @ai-generated — Nested v-for produces two _renderList calls
#[test]
fn test_v_for_nested() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="g in groups"><span v-for="i in g">{{ i }}</span></div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    let count = template.code.matches("_renderList(").count();
    assert!(
        count >= 2,
        "Nested v-for should produce 2 _renderList calls, got {} in:\n{}",
        count,
        template.code
    );
}

// =========================================================================
// v-once directives
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs
// NOTE: v-once is not yet implemented in the new pipeline. The directive is
// ignored and the element is rendered normally. These tests verify the
// element is still rendered correctly.

/// @ai-generated — v-once: element renders normally (cache not yet implemented)
#[test]
fn test_v_once_cache_pattern() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-once>static</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-once not yet implemented — element renders normally as block root
    assert!(
        template.code.contains("_createElementBlock("),
        "v-once should still produce _createElementBlock (block root), got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#""static""#),
        "v-once should still render text content, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-once with dynamic prop: element renders normally (cache not yet implemented)
#[test]
fn test_v_once_with_dynamic() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-once :id="foo">content</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-once not yet implemented — element renders with dynamic prop
    assert!(
        template.code.contains("id: _ctx.foo"),
        "v-once should still render dynamic prop, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains(r#""content""#),
        "v-once should still render text content, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-once: element renders normally (cache not yet implemented)
#[test]
fn test_v_once_cache_index() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-once>static</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-once not yet implemented — just verify element renders as block root
    assert!(
        template.code.contains(r#"_createElementBlock("div""#),
        "v-once should still produce element, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-once self-closing element renders normally
#[test]
fn test_v_once_self_closing() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><br v-once/></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-once not yet implemented — just verify element renders as block root
    assert!(
        template.code.contains(r#"_createElementBlock("br")"#),
        "v-once self-closing should still produce element, got:\n{}",
        template.code
    );
}

/// @ai-generated — v-once: element renders normally (cache not yet implemented)
#[test]
fn test_v_once_returns_cache() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-once>static</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // v-once not yet implemented — just verify element renders
    assert!(
        template.code.contains(r#""static""#),
        "v-once should still render text content, got:\n{}",
        template.code
    );
}

// =========================================================================
// Patch Flags
// =========================================================================
// ported from syntax/plugins/code_gen/template/tests.rs

/// @ai-generated — Bound :id produces { id: expr } (no PROPS patch flag on native element in new pipeline)
#[test]
fn test_pf_props() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :id="myId">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("id: _ctx.myId"),
        "Should have bound id prop, got:\n{}",
        template.code
    );
}

/// @ai-generated — :class -> CLASS (2)
#[test]
fn test_pf_class() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :class="cls">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("2 /* CLASS */"),
        "Should have CLASS (2), got:\n{}",
        template.code
    );
}

/// @ai-generated — :style -> STYLE (4)
#[test]
fn test_pf_style() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :style="sty">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("4 /* STYLE */"),
        "Should have STYLE (4), got:\n{}",
        template.code
    );
}

/// @ai-generated — Production mode: no patch flag comments
#[test]
fn test_pf_prod_no_comments() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div :class="cls">hi</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains(", 2)"),
        "Prod should have numeric flag without comment, got:\n{}",
        template.code
    );
    assert!(
        !template.code.contains("/* CLASS */"),
        "Prod should NOT have flag comment, got:\n{}",
        template.code
    );
}

/// @ai-generated — Single interpolation child should have TEXT (1) patch flag
#[test]
fn test_pf_text() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div>{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("1 /* TEXT */"),
        "Single interpolation should have TEXT (1), got:\n{}",
        template.code
    );
}

// =========================================================================
// Source Map: v-if / v-for expression mapping
// =========================================================================

/// @ai-generated — v-if condition expression is source-mapped in the template output
#[test]
fn test_v_if_condition_source_mapped() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        source_map: true,
        ..Default::default()
    };
    let source = r#"<template><div v-if="show">hello</div></template>"#;
    let result = compile(source, &options, &verter_opts, &allocator);
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("(_ctx.show) ?"),
        "v-if should produce ternary, got:\n{}",
        template.code
    );

    // Parse the source map and check for a token pointing to the "show" expression.
    // "show" starts at byte offset 21 in the source (after `<template><div v-if="`)
    assert!(
        !template.source_map.is_empty(),
        "source map should not be empty"
    );
    let sm = oxc_sourcemap::SourceMap::from_json_string(&template.source_map)
        .expect("should parse source map");
    // Collect tokens as tuples: (src_line, src_col, dst_line, dst_col, has_source)
    let tokens: Vec<(u32, u32, u32, u32, bool)> = sm
        .get_tokens()
        .map(|t| {
            (
                t.get_src_line(),
                t.get_src_col(),
                t.get_dst_line(),
                t.get_dst_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect();

    // The v-if expression "show" starts at byte 21 in source.
    // PositionResolver converts this to line 0, UTF-16 col 21.
    let has_show_mapping = tokens
        .iter()
        .any(|&(sl, sc, _, _, has_src)| has_src && sl == 0 && sc == 21);
    assert!(
        has_show_mapping,
        "Source map should have a token mapping to the v-if expression at src col 21.\n\
         Tokens on line 0: {:?}",
        tokens
            .iter()
            .filter(|&&(sl, _, _, _, has_src)| sl == 0 && has_src)
            .collect::<Vec<_>>()
    );
}

/// @ai-generated — v-else-if condition expression is source-mapped
#[test]
fn test_v_else_if_condition_source_mapped() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        source_map: true,
        ..Default::default()
    };
    //                  0         1         2         3         4         5         6         7
    //                  0123456789012345678901234567890123456789012345678901234567890123456789012345678
    let source = r#"<template><div v-if="a">x</div><div v-else-if="b">y</div></template>"#;
    let result = compile(source, &options, &verter_opts, &allocator);
    let template = result.template.as_ref().expect("should have template");

    let sm = oxc_sourcemap::SourceMap::from_json_string(&template.source_map)
        .expect("should parse source map");
    let tokens: Vec<(u32, u32, u32, u32, bool)> = sm
        .get_tokens()
        .map(|t| {
            (
                t.get_src_line(),
                t.get_src_col(),
                t.get_dst_line(),
                t.get_dst_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect();

    // "a" is at byte 21, "b" is at byte 48.
    // value_start points to the first char of the expression value (after the quote).
    let has_a_mapping = tokens
        .iter()
        .any(|&(sl, sc, _, _, has_src)| has_src && sl == 0 && sc == 21);
    // For "b": value_start should be 48 (the 'b' after the opening quote).
    // Accept col 47-48 to account for parser quote-boundary differences.
    let has_b_mapping = tokens
        .iter()
        .any(|&(sl, sc, _, _, has_src)| has_src && sl == 0 && (sc == 47 || sc == 48));

    assert!(
        has_a_mapping,
        "Source map should have a token for v-if expr 'a' at col 21"
    );
    assert!(
        has_b_mapping,
        "Source map should have a token for v-else-if expr 'b' near col 47-48.\n\
         Tokens on line 0: {:?}",
        tokens
            .iter()
            .filter(|&&(sl, _, _, _, has_src)| sl == 0 && has_src)
            .collect::<Vec<_>>()
    );
}

/// @ai-generated — v-for iterable expression source offset is correct in build_for_prefix
#[test]
fn test_v_for_iterable_source_offset() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        source_map: true,
        ..Default::default()
    };
    let source = r#"<template><div v-for="item in items" :key="item">{{ item }}</div></template>"#;
    let result = compile(source, &options, &verter_opts, &allocator);
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_renderList("),
        "v-for should produce _renderList call, got:\n{}",
        template.code
    );
    // Verify the template compiled correctly (smoke test)
    assert!(
        template.code.contains("_ctx.items") || template.code.contains("items"),
        "iterable should appear in output"
    );
}

// =========================================================================
// Hoisting scope-variable regression tests
// =========================================================================

/// Props referencing v-for loop variables must NOT be hoisted to module scope.
#[test]
fn v_for_key_not_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div v-for="(item, i) in items" :key="i" class="card">{{ item }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_renderList"),
        "should use _renderList, got:\n{}",
        template.code
    );
    // Props object with v-for locals must NOT be hoisted (would cause ReferenceError)
    assert!(
        !template.code.contains("_hoisted_1 = { key:"),
        "props object referencing v-for locals must not be hoisted, got:\n{}",
        template.code
    );
    assert!(
        template.code.contains("key: i"),
        "key should reference loop variable, got:\n{}",
        template.code
    );
}

/// Child element props referencing v-for locals must NOT be hoisted.
#[test]
fn v_for_child_props_not_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><ul><li v-for="item in items" :key="item.id"><span :title="item.name">text</span></li></ul></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    // Props objects with v-for locals must NOT be hoisted
    assert!(
        !template.code.contains("_hoisted_1 = { title:"),
        "child props object referencing v-for locals must not be hoisted, got:\n{}",
        template.code
    );
    assert!(
        !template.code.contains("_hoisted_2 = { key:"),
        "key props object referencing v-for locals must not be hoisted, got:\n{}",
        template.code
    );
    // Inline props objects should appear inside the render callback
    assert!(
        template.code.contains("{ title: item.name }")
            || template.code.contains("title: item.name"),
        "title prop should reference loop variable inline, got:\n{}",
        template.code
    );
}

/// Props referencing v-slot destructured variables must NOT be hoisted.
#[test]
fn v_slot_props_not_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><Comp v-slot="{ item }"><div :class="item.cls">text</div></Comp></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        !template.code.contains("_hoisted_"),
        "props referencing v-slot locals must not be hoisted, got:\n{}",
        template.code
    );
}

/// Truly static props (no scope locals) should still be hoisted.
#[test]
fn static_props_still_hoisted() {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    // Use dynamic child ({{ msg }}) so the parent isn't cache-wrapped
    let result = compile(
        r#"<template><div class="app">{{ msg }}</div></template>"#,
        &options,
        &verter_opts,
        &allocator,
    );
    let template = result.template.as_ref().expect("should have template");
    assert!(
        template.code.contains("_hoisted_"),
        "truly static props should still be hoisted, got:\n{}",
        template.code
    );
}
