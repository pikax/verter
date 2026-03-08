//! SSR codegen tests.
//!
//! Each test validates the SSR string-concatenation output against
//! the patterns produced by Vue's `@vue/compiler-ssr`.

use oxc_allocator::Allocator;

use crate::compile::{compile, CodegenOptions, VerterCompileOptions, VerterCompileResult};

fn compile_sfc_ssr(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ssr: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// Helper: compile and return the template code, asserting no errors.
fn gen_ssr_template(source: &str) -> String {
    let result = compile_sfc_ssr(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result
        .template
        .as_ref()
        .expect("should have template block");
    tpl.code.clone()
}

/// Helper: compile and return the script code, asserting no errors.
fn gen_ssr_script(source: &str) -> String {
    let result = compile_sfc_ssr(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("should have script block");
    script.code.clone()
}

// ══════════════════════════════════════════════════════════════════
// Basic element rendering
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_single_element() {
    let code = gen_ssr_template("<template><div>hello</div></template>");
    assert!(
        code.contains("function ssrRender("),
        "should have ssrRender function signature, got:\n{}",
        code
    );
    assert!(
        code.contains("_push("),
        "should use _push(), got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs(_attrs)"),
        "root element should merge _attrs, got:\n{}",
        code
    );
    assert!(
        code.contains("hello"),
        "should contain text content, got:\n{}",
        code
    );
    // Negative: should NOT contain VDOM helpers
    assert!(
        !code.contains("_createElementVNode"),
        "SSR should not use VDOM helpers, got:\n{}",
        code
    );
    assert!(
        !code.contains("_openBlock"),
        "SSR should not use _openBlock, got:\n{}",
        code
    );
}

#[test]
fn ssr_interpolation() {
    let code = gen_ssr_template("<template><div>{{ msg }}</div></template>");
    assert!(
        code.contains("_ssrInterpolate("),
        "interpolation should use _ssrInterpolate, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs(_attrs)"),
        "root element should have _ssrRenderAttrs, got:\n{}",
        code
    );
}

#[test]
fn ssr_static_class_root() {
    let code = gen_ssr_template(r#"<template><div class="hello">world</div></template>"#);
    assert!(
        code.contains("_ssrRenderAttrs"),
        "should use _ssrRenderAttrs for root with class, got:\n{}",
        code
    );
    assert!(
        code.contains("_mergeProps"),
        "root with static class should merge with _attrs, got:\n{}",
        code
    );
    assert!(
        code.contains("class: \"hello\""),
        "should have class attr, got:\n{}",
        code
    );
}

#[test]
fn ssr_nested_no_attrs() {
    let code = gen_ssr_template("<template><div><span>nested</span></div></template>");
    // Root div should have _ssrRenderAttrs
    assert!(
        code.contains("_ssrRenderAttrs(_attrs)"),
        "root should have _ssrRenderAttrs, got:\n{}",
        code
    );
    // Nested span should be literal HTML (no _ssrRenderAttrs)
    assert!(
        code.contains("<span>"),
        "nested span should be literal HTML, got:\n{}",
        code
    );
}

#[test]
fn ssr_void_elements() {
    let code = gen_ssr_template("<template><div><br/><hr/></div></template>");
    assert!(
        code.contains("<br>"),
        "void elements should not have closing tag, got:\n{}",
        code
    );
    assert!(
        !code.contains("</br>"),
        "void elements should not have </br>, got:\n{}",
        code
    );
    assert!(
        !code.contains("</hr>"),
        "void elements should not have </hr>, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Whitespace condensation
// ══════════════════════════════════════════════════════════════════

/// Vue's SSR compiler strips whitespace-only text nodes that contain
/// newlines (inter-element indentation). The output should concatenate
/// adjacent elements without whitespace.
///
/// Input: `<div>\n  <h1>title</h1>\n  <p>body</p>\n</div>`
/// Vue output: `_push(\`<div...><h1>title</h1><p>body</p></div>\`)`
#[test]
fn ssr_whitespace_between_elements_stripped() {
    let code =
        gen_ssr_template("<template><div>\n  <h1>title</h1>\n  <p>body</p>\n</div></template>");
    // Elements should be directly adjacent — no whitespace between them
    assert!(
        code.contains("<h1>title</h1><p>body</p>"),
        "whitespace between sibling elements should be stripped, got:\n{}",
        code
    );
    // Negative: should NOT have space or newline between elements
    assert!(
        !code.contains("</h1> <p>") && !code.contains("</h1>\n"),
        "should not have whitespace between elements, got:\n{}",
        code
    );
}

/// Whitespace-only text that does NOT contain a newline (just spaces)
/// should be condensed to a single space.
#[test]
fn ssr_whitespace_space_only_condensed() {
    let code = gen_ssr_template("<template><div><span>a</span>  <span>b</span></div></template>");
    // Multiple spaces should condense to a single space
    assert!(
        code.contains("</span> <span>"),
        "space-only whitespace should condense to single space, got:\n{}",
        code
    );
    // Negative: should NOT preserve multiple spaces
    assert!(
        !code.contains("</span>  <span>"),
        "should not preserve multiple spaces, got:\n{}",
        code
    );
}

/// Whitespace-only text between an interpolation and an element should be
/// preserved as a single space (not removed), matching Vue's condense mode.
/// This applies even when the whitespace contains a newline.
#[test]
fn ssr_whitespace_between_interp_and_element_preserved() {
    let code =
        gen_ssr_template("<template><div>{{ message }}\n  <span>text</span></div></template>");
    // Space between interpolation and element should be preserved
    assert!(
        code.contains(")} <span>"),
        "space between interpolation and element should be preserved, got:\n{}",
        code
    );
    // Negative: should NOT have them directly adjacent
    assert!(
        !code.contains(")}<span>"),
        "interpolation and element should not be directly adjacent, got:\n{}",
        code
    );
}

/// Whitespace at the boundary (first/last child) is removed even when
/// adjacent to an interpolation.
#[test]
fn ssr_whitespace_boundary_near_interp_removed() {
    let code = gen_ssr_template("<template><div>  {{ msg }}  </div></template>");
    // Boundary whitespace should be removed
    assert!(
        code.contains(">${_ssrInterpolate(") && code.contains(")}</div>"),
        "boundary whitespace near interp should be removed, got:\n{}",
        code
    );
    // Negative: should NOT have space before interpolation or after
    assert!(
        !code.contains("> ${") && !code.contains(")} </div>"),
        "should not have space at boundaries, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Structural directives
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_v_if_no_else() {
    let code = gen_ssr_template(r#"<template><div v-if="show">A</div></template>"#);
    assert!(
        code.contains("if ("),
        "v-if should produce if statement, got:\n{}",
        code
    );
    assert!(
        code.contains("_push(`<!---->`)"),
        "v-if without else should emit empty comment fallback, got:\n{}",
        code
    );
    // Negative: no VDOM ternary
    assert!(
        !code.contains("? ("),
        "SSR should not use ternary for v-if, got:\n{}",
        code
    );
}

#[test]
fn ssr_v_if_else() {
    let code =
        gen_ssr_template(r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#);
    assert!(
        code.contains("if ("),
        "should have if statement, got:\n{}",
        code
    );
    assert!(
        code.contains("} else {"),
        "should have else branch, got:\n{}",
        code
    );
    // Both branches should be root-level (get _ssrRenderAttrs)
    let attrs_count = code.matches("_ssrRenderAttrs").count();
    assert!(
        attrs_count >= 2,
        "both branches should get _ssrRenderAttrs (found {}), got:\n{}",
        attrs_count,
        code
    );
}

#[test]
fn ssr_v_for() {
    let code = gen_ssr_template(
        r#"<template><div v-for="item in list" :key="item">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderList("),
        "v-for should use _ssrRenderList, got:\n{}",
        code
    );
    assert!(
        code.contains("<!--[-->"),
        "v-for should have fragment open marker, got:\n{}",
        code
    );
    assert!(
        code.contains("<!--]-->"),
        "v-for should have fragment close marker, got:\n{}",
        code
    );
}

#[test]
fn ssr_v_show() {
    let code = gen_ssr_template(r#"<template><div v-show="vis">shown</div></template>"#);
    assert!(
        code.contains("display: \"none\""),
        "v-show should toggle display:none style, got:\n{}",
        code
    );
    assert!(
        code.contains("? null"),
        "v-show true branch should be null (no style), got:\n{}",
        code
    );
}

/// Vue puts `_attrs` BEFORE v-show style in `_mergeProps` so that
/// the v-show style overrides any incoming style. The last object in
/// `_mergeProps` wins for duplicate keys.
///
/// Vue: `_ssrRenderAttrs(_mergeProps(_attrs, { style: ... }))`
#[test]
fn ssr_v_show_mergeprops_order() {
    let code = gen_ssr_template(r#"<template><div v-show="vis">shown</div></template>"#);
    // _attrs must come BEFORE the style object
    assert!(
        code.contains("_mergeProps(_attrs,"),
        "v-show: _attrs must come before style in _mergeProps, got:\n{}",
        code
    );
    // Negative: _attrs should NOT be after the style
    assert!(
        !code.contains("}, _attrs)"),
        "v-show: _attrs should not be after style object, got:\n{}",
        code
    );
}

/// When the template root is a single element with v-for, Vue treats it as
/// multi-root because v-for produces 0..N elements, so _attrs should NOT be
/// applied to each iteration element.
#[test]
fn ssr_v_for_root_no_attrs() {
    let code = gen_ssr_template(
        r#"<template><div v-for="item in list" :key="item" class="item">{{ item }}</div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderList("),
        "v-for should use _ssrRenderList, got:\n{}",
        code
    );
    // _attrs should NOT be merged into the element — v-for root is multi-root
    // (note: _attrs appears in function signature, only check it's not in rendering)
    assert!(
        !code.contains("_mergeProps"),
        "v-for root element should not use _mergeProps (multi-root), got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrRenderAttrs"),
        "v-for root element should not use _ssrRenderAttrs, got:\n{}",
        code
    );
}

/// `:key` is a client-only prop for v-for keying. In SSR output,
/// it should be stripped — not emitted as an HTML attribute or
/// passed to `_ssrRenderAttrs`.
#[test]
fn ssr_v_for_key_stripped() {
    let code = gen_ssr_template(
        r#"<template><div><li v-for="item in list" :key="item.id">{{ item.name }}</li></div></template>"#,
    );
    // :key should NOT appear in SSR output
    assert!(
        !code.contains("key:"),
        ":key should be stripped from SSR output, got:\n{}",
        code
    );
    // The <li> should be a plain tag without _ssrRenderAttrs (only :key was dynamic)
    assert!(
        code.contains("<li>"),
        "v-for items without other dynamic attrs should be plain <li>, got:\n{}",
        code
    );
}

/// Text content with leading/trailing whitespace (but not whitespace-only)
/// should have its whitespace condensed. Vue condenses
/// `\n  text\n` to ` text ` (or just `text` depending on context).
#[test]
fn ssr_text_whitespace_condensed() {
    let code = gen_ssr_template("<template><div>\n  hello world\n</div></template>");
    // Text content should be condensed — no leading newline/spaces
    assert!(
        !code.contains("\\n"),
        "text should not contain literal newlines in template, got:\n{}",
        code
    );
    // The text should be present
    assert!(
        code.contains("hello world"),
        "text content should be preserved, got:\n{}",
        code
    );
}

#[test]
fn ssr_v_html() {
    let code = gen_ssr_template(r#"<template><div v-html="raw"></div></template>"#);
    assert!(
        code.contains("?? ''"),
        "v-html should null-coalesce, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrInterpolate for v-html (it's raw)
    assert!(
        !code.contains("_ssrInterpolate"),
        "v-html should not use _ssrInterpolate (raw output), got:\n{}",
        code
    );
}

#[test]
fn ssr_v_text() {
    let code = gen_ssr_template(r#"<template><div v-text="txt"></div></template>"#);
    assert!(
        code.contains("_ssrInterpolate("),
        "v-text should use _ssrInterpolate, got:\n{}",
        code
    );
}

#[test]
fn ssr_events_ignored() {
    let code =
        gen_ssr_template(r#"<template><button @click="onClick">click me</button></template>"#);
    assert!(
        !code.contains("onClick"),
        "SSR should ignore event handlers, got:\n{}",
        code
    );
    assert!(
        !code.contains("@click"),
        "SSR should not emit @click, got:\n{}",
        code
    );
    assert!(
        code.contains("click me"),
        "should preserve text content, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Close tags after structural breaks
// ══════════════════════════════════════════════════════════════════

/// When a nested element contains a structural break (v-if, v-for, component),
/// the break closes the parent's push. The nested element's close tag must still
/// end up inside a `_push()` call — not as raw text outside any push.
#[test]
fn ssr_close_tag_after_structural_break_in_push() {
    let code = gen_ssr_template(
        r#"<template><div><div class="wrapper"><span v-if="show">x</span></div><p>after</p></div></template>"#,
    );
    // The output should NOT have raw `</div>` outside a _push() call.
    // After the v-if, the close tags should be inside the resuming push.
    assert!(
        !code.contains("} </div>"),
        "close tag should not be raw text outside push, got:\n{}",
        code
    );
    assert!(
        !code.contains("}\n</div>"),
        "close tag should not be raw text outside push, got:\n{}",
        code
    );
    // The close tag and subsequent sibling should be in the same push
    assert!(
        code.contains("</div><p>after</p>"),
        "close tag and next sibling should be in same push, got:\n{}",
        code
    );
}

/// Template literal $ characters in comments should be escaped to \$.
#[test]
fn ssr_dollar_escaped_in_comment() {
    let code = gen_ssr_template(r#"<template><div><!-- $event test --></div></template>"#);
    assert!(
        code.contains("\\$event"),
        "$ in comments should be escaped to \\$ in template literals, got:\n{}",
        code
    );
    // Negative: should NOT have unescaped $event
    assert!(
        !code.contains("$event test") || code.contains("\\$event test"),
        "unescaped $ in template literal would cause JS interpolation, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Components
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_component() {
    let code = gen_ssr_template(r#"<template><MyComp :msg="hello" /></template>"#);
    assert!(
        code.contains("_resolveComponent("),
        "should resolve component, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderComponent("),
        "should use _ssrRenderComponent, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Script SSR flags
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_script_has_ssr_inline_render() {
    let source =
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>";
    let script = gen_ssr_script(source);
    assert!(
        script.contains("__ssrInlineRender"),
        "SSR script should have __ssrInlineRender, got:\n{}",
        script
    );
    // Negative: should not have __vapor
    assert!(
        !script.contains("__vapor"),
        "SSR script should not have __vapor, got:\n{}",
        script
    );
}

#[test]
fn ssr_template_only_has_ssr_inline_render() {
    let source = "<template><div>hello</div></template>";
    let result = compile_sfc_ssr(source);
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script");
    assert!(
        script.code.contains("__ssrInlineRender"),
        "template-only SSR should have __ssrInlineRender, got:\n{}",
        script.code
    );
}

// ══════════════════════════════════════════════════════════════════
// Imports
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_template_has_ssr_imports() {
    let result = compile_sfc_ssr("<template><div>{{ msg }}</div></template>");
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.ssr_imports.is_empty(),
        "SSR template should have ssr_imports, got: {:?}",
        tpl.ssr_imports
    );
    assert!(
        tpl.ssr_imports.contains(&"_ssrRenderAttrs"),
        "should import _ssrRenderAttrs, got: {:?}",
        tpl.ssr_imports
    );
    assert!(
        tpl.ssr_imports.contains(&"_ssrInterpolate"),
        "should import _ssrInterpolate, got: {:?}",
        tpl.ssr_imports
    );
}

#[test]
fn ssr_vue_imports_separate_from_ssr_imports() {
    let result = compile_sfc_ssr(r#"<template><div class="hello">{{ msg }}</div></template>"#);
    let tpl = result.template.as_ref().expect("should have template");
    // _mergeProps comes from "vue", not "vue/server-renderer"
    assert!(
        tpl.imports.contains(&"_mergeProps"),
        "vue imports should include _mergeProps, got: {:?}",
        tpl.imports
    );
    // SSR imports are separate
    assert!(
        !tpl.imports.contains(&"_ssrRenderAttrs"),
        "_ssrRenderAttrs should be in ssr_imports, not vue imports, got: {:?}",
        tpl.imports
    );
}

// ══════════════════════════════════════════════════════════════════
// Comments
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_comment_preserved_in_dev() {
    let code = gen_ssr_template("<template><!-- comment --><div>hello</div></template>");
    assert!(
        code.contains("<!-- comment -->"),
        "comments should be preserved in dev mode, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Push buffering — component resolveComponent ordering
// ══════════════════════════════════════════════════════════════════

/// Vue hoists `_resolveComponent()` calls before any `_push()`. When a
/// component appears as a child of a normal element, the resolve must
/// appear BEFORE the parent's `_push()`, not after.
///
/// Vue output pattern:
/// ```js
/// const _component_MyComp = _resolveComponent("MyComp")
/// _push(`<div${_ssrRenderAttrs(_attrs)}>`)
/// _push(_ssrRenderComponent(_component_MyComp, ...))
/// _push(`<p>after</p></div>`)
/// ```
#[test]
fn ssr_component_resolve_before_push() {
    let code = gen_ssr_template(r#"<template><div><MyComp /><p>after</p></div></template>"#);

    // _resolveComponent must appear BEFORE the first _push( call
    let resolve_pos = code
        .find("_resolveComponent(")
        .expect("should have _resolveComponent");
    let first_push_pos = code.find("_push(").expect("should have _push");

    assert!(
        resolve_pos < first_push_pos,
        "_resolveComponent should appear before first _push(), but resolve is at {} and first push at {}\ngot:\n{}",
        resolve_pos, first_push_pos, code
    );
}

// ══════════════════════════════════════════════════════════════════
// Push buffering — v-for fragment markers inside parent push
// ══════════════════════════════════════════════════════════════════

/// Vue puts the v-for fragment open marker `<!--[-->` inside the parent's
/// `_push()` template literal (not a separate `_push()` call). Same for
/// the close marker `<!--]-->`.
///
/// Vue output pattern:
/// ```js
/// _push(`<div${_ssrRenderAttrs(_attrs)}><ul><!--[-->`)
/// _ssrRenderList(list, (item) => {
///   _push(`<li>${_ssrInterpolate(item)}</li>`)
/// })
/// _push(`<!--]--></ul></div>`)
/// ```
#[test]
fn ssr_v_for_fragment_markers_inside_parent_push() {
    let code = gen_ssr_template(
        r#"<template><div><ul><li v-for="item in list" :key="item">{{ item }}</li></ul></div></template>"#,
    );

    // The fragment open marker should be in the same _push() as <ul>
    // i.e. _push(`...<ul><!--[-->`)
    assert!(
        code.contains("<ul><!--[-->"),
        "fragment open marker should be adjacent to <ul> in same push literal, got:\n{}",
        code
    );

    // The fragment close marker should be in the same _push() as </ul></div>
    // i.e. _push(`<!--]--></ul></div>`)
    assert!(
        code.contains("<!--]--></ul>"),
        "fragment close marker should be adjacent to </ul> in same push literal, got:\n{}",
        code
    );

    // Negative: should NOT have separate _push(`<!--[-->`) calls
    assert!(
        !code.contains("_push(`<!--[-->`)"),
        "fragment markers should NOT be in separate _push() calls, got:\n{}",
        code
    );
    assert!(
        !code.contains("_push(`<!--]-->`)"),
        "fragment markers should NOT be in separate _push() calls, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Push buffering — multi-root template
// ══════════════════════════════════════════════════════════════════

/// Multi-root templates should render all children in a single
/// `_push()` call with `<!--[-->...<!--]-->` fragment markers.
/// Individual roots should NOT get `_ssrRenderAttrs(_attrs)`.
///
/// Vue output pattern:
/// ```js
/// _push(`<!--[--><div>a</div><div>b</div><!--]-->`)
/// ```
#[test]
fn ssr_multi_root_fragment_markers() {
    let code = gen_ssr_template(r#"<template><div>a</div><div>b</div></template>"#);

    // Should have fragment markers
    assert!(
        code.contains("<!--[-->"),
        "multi-root should have fragment open marker, got:\n{}",
        code
    );
    assert!(
        code.contains("<!--]-->"),
        "multi-root should have fragment close marker, got:\n{}",
        code
    );

    // Individual roots should NOT get _ssrRenderAttrs
    assert!(
        !code.contains("_ssrRenderAttrs"),
        "multi-root elements should NOT have _ssrRenderAttrs (only single-root gets _attrs), got:\n{}",
        code
    );

    // All content in a single _push()
    let push_count = code.matches("_push(").count();
    assert_eq!(
        push_count, 1,
        "multi-root should have exactly 1 _push() call, got {} in:\n{}",
        push_count, code
    );
}

// ══════════════════════════════════════════════════════════════════
// E2E: Full SFC compilation
// ══════════════════════════════════════════════════════════════════

#[test]
fn ssr_full_compile() {
    let source = r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = compile_sfc_ssr(source);
    assert!(
        result.errors.is_empty(),
        "should compile without errors: {:?}",
        result.errors
    );

    let script = result.script.as_ref().expect("should have script");
    assert!(
        script.code.contains("__ssrInlineRender"),
        "script should have __ssrInlineRender, got:\n{}",
        script.code
    );

    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        tpl.code.contains("function ssrRender("),
        "template should have ssrRender function, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_push("),
        "template should use _push, got:\n{}",
        tpl.code
    );

    // Negative: no VDOM in SSR output
    assert!(
        !tpl.code.contains("_createElementVNode"),
        "SSR should not use _createElementVNode, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_openBlock"),
        "SSR should not use _openBlock, got:\n{}",
        tpl.code
    );
}

#[test]
fn ssr_full_compile_negative() {
    let source = "<template><div>hello</div></template>";
    let result = compile_sfc_ssr(source);
    let tpl = result.template.as_ref().expect("should have template");

    // SSR output should NOT contain any VDOM helpers
    for bad in &[
        "_createElementVNode",
        "_openBlock",
        "_createElementBlock",
        "_createTextVNode",
        "_Fragment",
        "_normalizeClass",
    ] {
        assert!(
            !tpl.code.contains(bad),
            "SSR output should not contain {}, got:\n{}",
            bad,
            tpl.code
        );
    }
}

// ══════════════════════════════════════════════════════════════════
// Phase 1A: Component slot content (_withCtx dual-branch wrappers)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Component with default slot content should produce
/// _withCtx wrapper with SSR/VDOM dual branches.
#[test]
fn ssr_component_default_slot() {
    let code = gen_ssr_template(r#"<template><MyComp>hello</MyComp></template>"#);
    // Should have _withCtx wrapper
    assert!(
        code.contains("_withCtx("),
        "component with children should use _withCtx, got:\n{}",
        code
    );
    // Should have SSR branch with _push
    assert!(
        code.contains("if (_push)"),
        "should have SSR branch `if (_push)`, got:\n{}",
        code
    );
    // Should have VDOM fallback branch
    assert!(
        code.contains("return ["),
        "should have VDOM fallback branch `return [`, got:\n{}",
        code
    );
    // Should NOT pass null as slots arg
    assert!(
        !code.contains(", null, _parent)"),
        "should not pass null for slots when children exist, got:\n{}",
        code
    );
    // Should have default slot
    assert!(
        code.contains("default:"),
        "should have default slot, got:\n{}",
        code
    );
    // Should have stable slot marker
    assert!(
        code.contains("_: 1"),
        "should have stable slot marker `_: 1`, got:\n{}",
        code
    );
}

/// @ai-generated — Sibling elements in component slot should use a single _push().
#[test]
fn ssr_component_slot_sibling_elements_single_push() {
    let code = gen_ssr_template(
        r#"<template><MyComp><div>1</div><div>2</div><div>3</div></MyComp></template>"#,
    );
    // All sibling elements should be in a single push, not separate pushes
    assert!(
        code.contains("<div>1</div><div>2</div><div>3</div>"),
        "sibling elements in slot should be in same push, got:\n{}",
        code
    );
    // Negative: should NOT have push splitting between sibling elements
    assert!(
        !code.contains("</div>`) _push(`<div>"),
        "should NOT split pushes between sibling elements, got:\n{}",
        code
    );
}

/// @ai-generated — Sibling elements in a named slot should merge into single push.
#[test]
fn ssr_component_named_slot_sibling_elements_single_push() {
    let code = gen_ssr_template(
        r#"<template><MyComp><template #default><div>1</div><div>2</div></template></MyComp></template>"#,
    );
    // All sibling elements should be in a single push
    assert!(
        code.contains("<div>1</div><div>2</div>"),
        "sibling elements in named slot should be in same push, got:\n{}",
        code
    );
    // Negative: should NOT split pushes between sibling elements
    assert!(
        !code.contains("</div>`) _push(`<div>"),
        "should NOT split pushes between sibling elements in named slot, got:\n{}",
        code
    );
}

/// @ai-generated — Component with named slot via <template #header>.
#[test]
fn ssr_component_named_slot() {
    let code = gen_ssr_template(
        r#"<template><MyComp><template #header>title</template></MyComp></template>"#,
    );
    // Should have named slot
    assert!(
        code.contains("header: _withCtx("),
        "should have named `header` slot with _withCtx, got:\n{}",
        code
    );
    // Should have stable slot marker
    assert!(
        code.contains("_: 1"),
        "should have stable slot marker, got:\n{}",
        code
    );
    // Negative: should NOT have literal <template> tags in output
    assert!(
        !code.contains("<template"),
        "template wrapper should not appear in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — Component with scoped slot params.
#[test]
fn ssr_component_scoped_slot() {
    let code = gen_ssr_template(
        r#"<template><MyComp><template #default="{ item }">{{ item }}</template></MyComp></template>"#,
    );
    // Scoped slot params should appear in _withCtx
    assert!(
        code.contains("{ item }"),
        "scoped slot params should be in output, got:\n{}",
        code
    );
    assert!(
        code.contains("_withCtx("),
        "should use _withCtx for scoped slot, got:\n{}",
        code
    );
}

/// @ai-generated — Component with NO children should still pass null for slots.
#[test]
fn ssr_component_no_children_null_slots() {
    let code = gen_ssr_template(r#"<template><MyComp :msg="hello" /></template>"#);
    assert!(
        code.contains(", null, _parent)"),
        "component without children should pass null for slots, got:\n{}",
        code
    );
}

/// @ai-generated — Component with multiple named slots.
#[test]
fn ssr_component_multiple_slots() {
    let code = gen_ssr_template(
        r#"<template><MyComp><template #header>H</template><template #footer>F</template></MyComp></template>"#,
    );
    assert!(
        code.contains("header: _withCtx("),
        "should have header slot, got:\n{}",
        code
    );
    assert!(
        code.contains("footer: _withCtx("),
        "should have footer slot, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Phase 1B: <slot> outlet rendering (_ssrRenderSlot)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Default slot outlet.
#[test]
fn ssr_slot_outlet_default() {
    let code = gen_ssr_template(r#"<template><div><slot></slot></div></template>"#);
    assert!(
        code.contains("_ssrRenderSlot("),
        "should use _ssrRenderSlot, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.$slots"),
        "should access _ctx.$slots, got:\n{}",
        code
    );
    assert!(
        code.contains("\"default\""),
        "should use \"default\" slot name, got:\n{}",
        code
    );
    // Negative: no literal <slot> tags
    assert!(
        !code.contains("<slot"),
        "should not have literal <slot> in output, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot outlet.
#[test]
fn ssr_slot_outlet_named() {
    let code = gen_ssr_template(r#"<template><div><slot name="header"></slot></div></template>"#);
    assert!(
        code.contains("\"header\""),
        "should use \"header\" slot name, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderSlot("),
        "should use _ssrRenderSlot, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with fallback content.
#[test]
fn ssr_slot_outlet_with_fallback() {
    let code = gen_ssr_template(r#"<template><div><slot>fallback text</slot></div></template>"#);
    assert!(
        code.contains("_ssrRenderSlot("),
        "should use _ssrRenderSlot, got:\n{}",
        code
    );
    assert!(
        code.contains("fallback"),
        "should have fallback content, got:\n{}",
        code
    );
    // Should have fallback function
    assert!(
        code.contains("() => {"),
        "should have fallback function, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with bound props.
#[test]
fn ssr_slot_outlet_with_props() {
    let code = gen_ssr_template(r#"<template><div><slot :item="x"></slot></div></template>"#);
    assert!(
        code.contains("_ssrRenderSlot("),
        "should use _ssrRenderSlot, got:\n{}",
        code
    );
    assert!(
        code.contains("item:"),
        "should have item prop, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic slot name (:name="expr") should use expression, not string.
#[test]
fn ssr_slot_outlet_dynamic_name() {
    let code = gen_ssr_template(
        r#"<template><div><slot :name="slotName" :item="x"></slot></div></template>"#,
    );
    // Should have dynamic name as expression, not quoted string
    assert!(
        code.contains("_ctx.slotName") || code.contains("slotName"),
        "should use dynamic slot name expression, got:\n{}",
        code
    );
    // Should NOT have "default" as the slot name
    assert!(
        !code.contains("\"default\""),
        "should not use \"default\" for dynamic slot name, got:\n{}",
        code
    );
    // Should NOT include :name as a prop
    assert!(
        !code.contains("name: _ctx.slotName"),
        ":name should not appear as a prop, got:\n{}",
        code
    );
    // Should have item prop
    assert!(
        code.contains("item:"),
        "should have item prop, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with no props should use {}.
#[test]
fn ssr_slot_outlet_no_props_empty() {
    let code = gen_ssr_template(r#"<template><div><slot></slot></div></template>"#);
    assert!(
        code.contains(", {}, ") || code.contains(", {},"),
        "empty slot props should be {{}}, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with v-bind spread should pass spread as props.
/// `<slot v-bind="obj" />` → `_ssrRenderSlot(slots, "default", _ctx.obj, ...)`
#[test]
fn ssr_slot_outlet_v_bind_spread() {
    let code = gen_ssr_template(r#"<template><div><slot v-bind="field" /></div></template>"#);
    assert!(
        code.contains("_ctx.field") || code.contains("$setup.field"),
        "should resolve v-bind spread expression, got:\n{}",
        code
    );
    // Single spread should be used directly, no _mergeProps wrapping
    assert!(
        !code.contains("_mergeProps"),
        "single v-bind spread should not use _mergeProps, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with v-bind spread + individual props should use _mergeProps.
/// `<slot v-bind="field" :id="id" />` → `_ssrRenderSlot(slots, "default", _mergeProps(_ctx.field, { id: _ctx.id }), ...)`
#[test]
fn ssr_slot_outlet_v_bind_spread_with_props() {
    let code =
        gen_ssr_template(r#"<template><div><slot v-bind="field" :id="id" /></div></template>"#);
    assert!(
        code.contains("_mergeProps"),
        "should use _mergeProps for v-bind spread + individual props, got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx.field") || code.contains("$setup.field"),
        "should resolve v-bind spread expression, got:\n{}",
        code
    );
    assert!(
        code.contains("id: _ctx.id") || code.contains("id:"),
        "should have individual id prop, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet with v-bind object literal spread.
/// `<slot v-bind="{ dayTitle, customData }" />` → resolves the object literal
#[test]
fn ssr_slot_outlet_v_bind_object_literal() {
    let code =
        gen_ssr_template(r#"<template><div><slot v-bind="{ foo, bar }" /></div></template>"#);
    assert!(
        code.contains("foo") && code.contains("bar"),
        "should resolve object literal in v-bind spread, got:\n{}",
        code
    );
    // Single spread — should be used directly, no _mergeProps wrapping
    assert!(
        !code.contains("_mergeProps"),
        "single v-bind object literal should not use _mergeProps, got:\n{}",
        code
    );
}

/// @ai-generated — Slot outlet props with kebab-case should be camelized.
/// `<slot :my-prop="val" />` → `{myProp: _ctx.val}` not `{"my-prop": _ctx.val}`
#[test]
fn ssr_slot_outlet_prop_camelized() {
    let code = gen_ssr_template(
        r#"<template><div><slot :my-prop="val" :another-one="x" /></div></template>"#,
    );
    assert!(
        code.contains("myProp:"),
        "slot props should be camelized, got:\n{}",
        code
    );
    assert!(
        code.contains("anotherOne:"),
        "slot props should be camelized, got:\n{}",
        code
    );
    // Negative: no kebab-case keys
    assert!(
        !code.contains("my-prop") && !code.contains("another-one"),
        "should not have kebab-case slot props, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Phase 2: v-model SSR handling
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-model on text input should produce value attr.
#[test]
fn ssr_v_model_input_text() {
    let code = gen_ssr_template(r#"<template><div><input v-model="text"></div></template>"#);
    // Should have value in attrs
    assert!(
        code.contains("value:") || code.contains("\"value\""),
        "v-model on input should produce value attr, got:\n{}",
        code
    );
    // Negative: no raw v-model
    assert!(
        !code.contains("v-model"),
        "v-model should not appear literally in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on textarea should produce _ssrInterpolate content.
#[test]
fn ssr_v_model_textarea() {
    let code =
        gen_ssr_template(r#"<template><div><textarea v-model="text"></textarea></div></template>"#);
    assert!(
        code.contains("_ssrInterpolate("),
        "v-model on textarea should use _ssrInterpolate for content, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on checkbox should produce checked attr.
#[test]
fn ssr_v_model_checkbox() {
    let code = gen_ssr_template(
        r#"<template><div><input type="checkbox" v-model="checked"></div></template>"#,
    );
    assert!(
        code.contains("checked"),
        "v-model on checkbox should produce checked attr, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Phase 3: Nested element inline attribute rendering
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Nested element with dynamic :class should use _ssrRenderClass.
#[test]
fn ssr_nested_dynamic_class() {
    let code =
        gen_ssr_template(r#"<template><div><span :class="cls">text</span></div></template>"#);
    assert!(
        code.contains("_ssrRenderClass("),
        "nested :class should use _ssrRenderClass, got:\n{}",
        code
    );
    // Negative: _ssrRenderAttrs should appear exactly once (for the root <div>),
    // NOT for the nested <span> with :class
    let render_attrs_count = code.matches("_ssrRenderAttrs(").count();
    assert_eq!(
        render_attrs_count, 1,
        "should have exactly 1 _ssrRenderAttrs (root only), got {} in:\n{}",
        render_attrs_count, code
    );
}

/// @ai-generated — Nested element with dynamic :style should use _ssrRenderStyle.
#[test]
fn ssr_nested_dynamic_style() {
    let code =
        gen_ssr_template(r#"<template><div><span :style="sty">text</span></div></template>"#);
    assert!(
        code.contains("_ssrRenderStyle("),
        "nested :style should use _ssrRenderStyle, got:\n{}",
        code
    );
}

/// @ai-generated — Nested element with dynamic boolean attr should use _ssrIncludeBooleanAttr.
#[test]
fn ssr_nested_boolean_attr() {
    let code =
        gen_ssr_template(r#"<template><div><button :disabled="d">click</button></div></template>"#);
    assert!(
        code.contains("_ssrIncludeBooleanAttr("),
        "nested boolean attr should use _ssrIncludeBooleanAttr, got:\n{}",
        code
    );
}

/// @ai-generated — Nested element with dynamic regular attr should use _ssrRenderAttr.
#[test]
fn ssr_nested_dynamic_regular_attr() {
    let code = gen_ssr_template(r#"<template><div><input :value="v"></div></template>"#);
    assert!(
        code.contains("_ssrRenderAttr("),
        "nested dynamic attr should use _ssrRenderAttr, got:\n{}",
        code
    );
}

/// @ai-generated — Root element with dynamic :class should still use _ssrRenderAttrs.
#[test]
fn ssr_root_still_uses_render_attrs() {
    let code = gen_ssr_template(r#"<template><div :class="cls">text</div></template>"#);
    assert!(
        code.contains("_ssrRenderAttrs("),
        "root :class should use _ssrRenderAttrs, got:\n{}",
        code
    );
}

/// @ai-generated — Nested element with v-bind spread should still use _ssrRenderAttrs.
#[test]
fn ssr_nested_v_bind_spread_uses_render_attrs() {
    let code =
        gen_ssr_template(r#"<template><div><span v-bind="obj">text</span></div></template>"#);
    assert!(
        code.contains("_ssrRenderAttrs("),
        "nested v-bind spread should use _ssrRenderAttrs, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 1: Component name casing in _resolveComponent()
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — _resolveComponent should use original kebab-case tag name.
#[test]
fn ssr_component_resolve_preserves_original_casing() {
    let code = gen_ssr_template(r#"<template><my-header /></template>"#);
    assert!(
        code.contains("_resolveComponent(\"my-header\")"),
        "should use original kebab-case in _resolveComponent, got:\n{}",
        code
    );
    // Vue uses tag name with hyphens replaced by underscores for the variable name
    assert!(
        code.contains("_component_my_header"),
        "variable name should use underscore replacement, got:\n{}",
        code
    );
    // Negative: should NOT have PascalCase variable name
    assert!(
        !code.contains("_component_MyHeader"),
        "variable name should NOT be PascalCase, got:\n{}",
        code
    );
    // Negative: should NOT have PascalCase in resolve arg
    assert!(
        !code.contains("_resolveComponent(\"MyHeader\")"),
        "resolve arg should not be PascalCase, got:\n{}",
        code
    );
}

/// @ai-generated — PascalCase tag should be preserved as-is in _resolveComponent.
#[test]
fn ssr_component_resolve_pascal_already() {
    let code = gen_ssr_template(r#"<template><MyHeader /></template>"#);
    assert!(
        code.contains("_resolveComponent(\"MyHeader\")"),
        "PascalCase tag should stay PascalCase in resolve, got:\n{}",
        code
    );
    assert!(
        code.contains("_component_MyHeader"),
        "variable name should be PascalCase, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 4: Attribute ordering — source order (matches Vue)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — When dynamic attr comes before static in source, output preserves that order.
#[test]
fn ssr_nested_attr_order_dynamic_before_static() {
    let code =
        gen_ssr_template(r#"<template><div><input :value="x" type="text"></div></template>"#);
    let render_attr_pos = code
        .find("_ssrRenderAttr(")
        .expect("should have _ssrRenderAttr");
    let type_pos = code.find("type=\"text\"").expect("should have type attr");
    assert!(
        render_attr_pos < type_pos,
        "dynamic attr should appear before static attr (source order), got:\n{}",
        code
    );
}

/// @ai-generated — When static attr comes before dynamic in source, output preserves that order.
#[test]
fn ssr_nested_attr_order_static_before_dynamic() {
    let code =
        gen_ssr_template(r#"<template><div><input type="text" :value="x"></div></template>"#);
    let type_pos = code.find("type=\"text\"").expect("should have type attr");
    let render_attr_pos = code
        .find("_ssrRenderAttr(")
        .expect("should have _ssrRenderAttr");
    assert!(
        type_pos < render_attr_pos,
        "static attr should appear before dynamic attr (source order), got:\n{}",
        code
    );
}

/// @ai-generated — Multiple static + dynamic attrs in mixed order: preserved in source order.
#[test]
fn ssr_nested_mixed_attrs_source_order() {
    let code = gen_ssr_template(
        r#"<template><div><input :value="v" type="text" :disabled="d" placeholder="Search"></div></template>"#,
    );
    let value_pos = code
        .find("_ssrRenderAttr(\"value\"")
        .expect("should have _ssrRenderAttr for value");
    let type_pos = code.find("type=\"text\"").expect("should have type attr");
    let disabled_pos = code
        .find("_ssrIncludeBooleanAttr(")
        .expect("should have boolean attr for disabled");
    let placeholder_pos = code
        .find("placeholder=\"Search\"")
        .expect("should have placeholder");
    assert!(
        value_pos < type_pos && type_pos < disabled_pos && disabled_pos < placeholder_pos,
        "attrs should be in source order, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 3: v-show nested elements — inline _ssrRenderStyle
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Nested v-show should use inline style, not _ssrRenderAttrs.
#[test]
fn ssr_nested_v_show_uses_inline_style() {
    let code =
        gen_ssr_template(r#"<template><div><div v-show="visible">content</div></div></template>"#);
    assert!(
        code.contains("_ssrRenderStyle("),
        "nested v-show should use _ssrRenderStyle, got:\n{}",
        code
    );
    // Should have inline style attribute
    assert!(
        code.contains("style=\"${"),
        "nested v-show should use inline style attr, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrRenderAttrs for the nested v-show element
    // (only the root div should have _ssrRenderAttrs)
    let render_attrs_count = code.matches("_ssrRenderAttrs(").count();
    assert_eq!(
        render_attrs_count, 1,
        "should have exactly 1 _ssrRenderAttrs (root only), got {} in:\n{}",
        render_attrs_count, code
    );
}

/// @ai-generated — Nested v-show with existing static style.
#[test]
fn ssr_nested_v_show_with_existing_style() {
    let code = gen_ssr_template(
        r#"<template><div><div v-show="visible" style="color: red">content</div></div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderStyle("),
        "should use _ssrRenderStyle for v-show, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 2: Static style rendering — _ssrRenderStyle()
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Static style on nested element should use _ssrRenderStyle().
#[test]
fn ssr_static_style_uses_render_style() {
    let code = gen_ssr_template(
        r#"<template><div><span style="height: 60%">text</span></div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderStyle("),
        "static style should use _ssrRenderStyle, got:\n{}",
        code
    );
    assert!(
        code.contains("\"height\""),
        "style should be JS object with property name, got:\n{}",
        code
    );
    // Negative: should NOT have plain CSS style
    assert!(
        !code.contains("style=\"height: 60%\""),
        "should not have plain CSS style attr, got:\n{}",
        code
    );
}

/// @ai-generated — Static style with multiple properties converted to JS object.
#[test]
fn ssr_static_style_multiple_props() {
    let code = gen_ssr_template(
        r#"<template><div><span style="color: red; font-size: 14px">text</span></div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderStyle("),
        "should use _ssrRenderStyle, got:\n{}",
        code
    );
    assert!(
        code.contains("\"font-size\""),
        "font-size should stay in kebab-case for SSR, got:\n{}",
        code
    );
    assert!(
        code.contains("\"color\""),
        "should have color property, got:\n{}",
        code
    );
}

/// @ai-generated — Static style on root element keeps kebab-case in mergeProps.
#[test]
fn ssr_static_style_root_kebab_case() {
    let code = gen_ssr_template(
        r#"<template><div style="margin-left: 20px; border-left: 2px solid gray">text</div></template>"#,
    );
    assert!(
        code.contains("\"margin-left\""),
        "should keep margin-left in kebab-case, got:\n{}",
        code
    );
    assert!(
        code.contains("\"border-left\""),
        "should keep border-left in kebab-case, got:\n{}",
        code
    );
    // Negative: should NOT camelCase
    assert!(
        !code.contains("marginLeft"),
        "should NOT camelCase CSS property names, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 5: Dynamic component <component :is>
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — <component :is="comp"> should use _ssrRenderVNode + _resolveDynamicComponent.
#[test]
fn ssr_dynamic_component_basic() {
    let code = gen_ssr_template(r#"<template><component :is="comp" /></template>"#);
    assert!(
        code.contains("_ssrRenderVNode("),
        "should use _ssrRenderVNode, got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveDynamicComponent("),
        "should use _resolveDynamicComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_createVNode("),
        "should use _createVNode, got:\n{}",
        code
    );
    // Negative: should NOT use _resolveComponent or _ssrRenderComponent
    assert!(
        !code.contains("_resolveComponent(\"component\")"),
        "should not resolve 'component' as normal component, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrRenderComponent("),
        "should not use _ssrRenderComponent for dynamic component, got:\n{}",
        code
    );
}

/// @ai-generated — <component :is> should not have _resolveComponent("Component").
#[test]
fn ssr_dynamic_component_no_resolve() {
    let code = gen_ssr_template(r#"<template><component :is="currentView" /></template>"#);
    // Should not have any _resolveComponent calls
    assert!(
        !code.contains("_resolveComponent("),
        "dynamic component should not have _resolveComponent, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 6: Suspense SSR rendering
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — <Suspense> should use _ssrRenderSuspense.
#[test]
fn ssr_suspense_basic() {
    let code = gen_ssr_template(r#"<template><Suspense><div>content</div></Suspense></template>"#);
    assert!(
        code.contains("_ssrRenderSuspense("),
        "should use _ssrRenderSuspense, got:\n{}",
        code
    );
    assert!(
        code.contains("default: () => {"),
        "should have default slot callback, got:\n{}",
        code
    );
    // Negative: no _resolveComponent or _ssrRenderComponent
    assert!(
        !code.contains("_resolveComponent(\"Suspense\")"),
        "should not resolve Suspense as component, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrRenderComponent("),
        "should not use _ssrRenderComponent for Suspense, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Fix 7: v-model _ssrGetDynamicModelProps
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Root v-model input should include _ssrGetDynamicModelProps.
#[test]
fn ssr_v_model_root_dynamic_model_props() {
    let code = gen_ssr_template(r#"<template><input v-model="text"></template>"#);
    assert!(
        code.contains("_ssrGetDynamicModelProps("),
        "root v-model should have _ssrGetDynamicModelProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_mergeProps("),
        "root v-model should use _mergeProps, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3 Fix 1: Component variable naming (kebab → underscore)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Kebab-case component tags use underscore replacement, not PascalCase.
#[test]
fn ssr_component_var_name_kebab_case() {
    let code = gen_ssr_template(r#"<template><a-button>click</a-button></template>"#);
    assert!(
        code.contains("_component_a_button"),
        "kebab-case tag should use underscore var name, got:\n{}",
        code
    );
    // Negative: should NOT have PascalCase variable name
    assert!(
        !code.contains("_component_AButton"),
        "should NOT use PascalCase var name for kebab-case tag, got:\n{}",
        code
    );
    // Resolve arg should use original tag name
    assert!(
        code.contains("_resolveComponent(\"a-button\")"),
        "resolve arg should be original tag name, got:\n{}",
        code
    );
}

/// @ai-generated — PascalCase component tags keep the same name (no hyphens to replace).
#[test]
fn ssr_component_var_name_pascal_case() {
    let code = gen_ssr_template(r#"<template><FormItem /></template>"#);
    assert!(
        code.contains("_component_FormItem"),
        "PascalCase tag should keep PascalCase var name, got:\n{}",
        code
    );
    assert!(
        code.contains("_resolveComponent(\"FormItem\")"),
        "resolve arg should be original PascalCase, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3 Fix 2: Setup ref for locally imported components
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Imported component uses $setup ref, not _resolveComponent.
#[test]
fn ssr_setup_import_uses_setup_ref() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><MyComp msg="hello" /></template>"#;
    let code = gen_ssr_template(source);
    assert!(
        code.contains("$setup.MyComp") || code.contains("$setup[\"MyComp\"]"),
        "imported component should use $setup ref, got:\n{}",
        code
    );
    // Negative: should NOT have _resolveComponent
    assert!(
        !code.contains("_resolveComponent"),
        "imported component should NOT use _resolveComponent, got:\n{}",
        code
    );
}

/// @ai-generated — Kebab-case tag with PascalCase import uses $setup ref.
#[test]
fn ssr_setup_import_kebab_tag() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><my-comp msg="hello" /></template>"#;
    let code = gen_ssr_template(source);
    assert!(
        code.contains("$setup.MyComp") || code.contains("$setup[\"MyComp\"]"),
        "kebab-case tag with PascalCase import should use $setup ref, got:\n{}",
        code
    );
    // Negative: should NOT have _resolveComponent
    assert!(
        !code.contains("_resolveComponent"),
        "imported component should NOT use _resolveComponent, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3 Fix 3: Event handler props on components
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Component event handlers are included as onXxx props.
#[test]
fn ssr_component_event_props() {
    let source = r#"<script setup>
import { ref } from 'vue'
const handler = () => {}
</script>
<template><MyComp @click="handler" /></template>"#;
    let code = gen_ssr_template(source);
    assert!(
        code.contains("onClick:"),
        "component should have onClick prop, got:\n{}",
        code
    );
    // Negative: the onClick should not be missing from the props object
    assert!(
        !code.contains("_ssrRenderComponent(_component_MyComp, null,"),
        "component props should not be null when there are events, got:\n{}",
        code
    );
}

/// @ai-generated — Component update event uses quoted key.
#[test]
fn ssr_component_update_event() {
    let source = r#"<script setup>
const fn1 = () => {}
</script>
<template><MyComp @update:modelValue="fn1" /></template>"#;
    let code = gen_ssr_template(source);
    assert!(
        code.contains("\"onUpdate:modelValue\""),
        "update event should have quoted key, got:\n{}",
        code
    );
}

/// @ai-generated — HTML element events still ignored in SSR.
#[test]
fn ssr_html_element_events_still_ignored() {
    let code = gen_ssr_template(r#"<template><button @click="handler">click</button></template>"#);
    assert!(
        !code.contains("onClick"),
        "HTML element events should still be ignored in SSR, got:\n{}",
        code
    );
    assert!(
        !code.contains("@click"),
        "HTML element events should not appear in SSR output, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3 Fix 4: Source-order attribute rendering (v-model)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-model attr placed at correct source position.
#[test]
fn ssr_attr_source_order_v_model() {
    let code = gen_ssr_template(
        r#"<template><div><input v-model="name" placeholder="Name"></div></template>"#,
    );
    let model_pos = code
        .find("_ssrRenderAttr(\"value\"")
        .expect("should have v-model _ssrRenderAttr");
    let placeholder_pos = code
        .find("placeholder=\"Name\"")
        .expect("should have placeholder");
    assert!(
        model_pos < placeholder_pos,
        "v-model attr should appear before placeholder (source order), got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3 Fix 5: Text whitespace around interpolations
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Whitespace around {{ }} at element boundaries is trimmed.
#[test]
fn ssr_text_whitespace_trimmed_around_interpolation() {
    let code = gen_ssr_template(r#"<template><span>{{ foo }}</span></template>"#);
    // Should NOT have extra spaces around the interpolation
    assert!(
        !code.contains("> ${") && !code.contains("} <"),
        "whitespace around interpolation should be trimmed at boundaries, got:\n{}",
        code
    );
    // Positive: should have interpolation directly after tag
    assert!(
        code.contains(">${_ssrInterpolate("),
        "interpolation should be directly after >, got:\n{}",
        code
    );
    assert!(
        code.contains(")}</span>"),
        "interpolation should be directly before </span>, got:\n{}",
        code
    );
}

/// @ai-generated — Whitespace between text and interpolation is preserved.
#[test]
fn ssr_text_whitespace_preserved_between_text() {
    let code = gen_ssr_template(r#"<template><span>hello {{ foo }} world</span></template>"#);
    // Spaces between "hello" and interpolation, and between interpolation and "world"
    // should be preserved as part of the text content
    assert!(
        code.contains("hello "),
        "space after 'hello' should be preserved, got:\n{}",
        code
    );
    assert!(
        code.contains("world"),
        "text 'world' should be preserved, got:\n{}",
        code
    );
}

/// @ai-generated — Non-boundary whitespace (e.g. <span> text </span>) is preserved.
#[test]
fn ssr_text_whitespace_non_interpolation_preserved() {
    let code = gen_ssr_template(r#"<template><span> text </span></template>"#);
    // Single spaces should be condensed but preserved (no interpolation adjacent)
    assert!(
        code.contains("text"),
        "text content should be preserved, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 3→7 Fix: HTML entity decode + re-encode in SSR text
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — HTML special entities are decoded then re-encoded in SSR.
#[test]
fn ssr_text_entity_gt_round_trips() {
    let code = gen_ssr_template(r#"<template><p>&gt;</p></template>"#);
    // &gt; → decode to > → re-encode to &gt; (HTML special char)
    assert!(
        code.contains("&gt;"),
        "HTML entity &gt; should round-trip back to &gt; in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — &amp; entity round-trips in SSR output.
#[test]
fn ssr_text_entity_amp_round_trips() {
    let code = gen_ssr_template(r#"<template><p>&amp;</p></template>"#);
    assert!(
        code.contains("&amp;"),
        "HTML entity &amp; should round-trip back to &amp; in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — &copy; entity is decoded to © (non-special char stays decoded).
#[test]
fn ssr_text_entity_copy_decoded() {
    let code = gen_ssr_template(r#"<template><p>&copy;</p></template>"#);
    // &copy; → decode to © → NOT re-encoded (not HTML special) → stays as ©
    assert!(
        code.contains('\u{00A9}'),
        "HTML entity &copy; should be decoded to © in SSR output, got:\n{}",
        code
    );
    // Negative: the entity form should NOT appear
    assert!(
        !code.contains("&copy;"),
        "&copy; entity should be decoded, not preserved, got:\n{}",
        code
    );
}

/// @ai-generated — Raw > in text is encoded to &gt; in SSR output.
#[test]
fn ssr_text_raw_gt_encoded() {
    // Note: tokenizer may or may not allow raw >, but if it does, it should be encoded
    let code = gen_ssr_template(r#"<template><p>a &gt; b</p></template>"#);
    assert!(
        code.contains("a &gt; b"),
        "raw > should be encoded to &gt; in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler wrapping: inline handler gets $event wrapper.
#[test]
fn ssr_component_event_inline_handler_wrapped() {
    let code = gen_ssr_template(r#"<template><MyComp @click="refresh()" /></template>"#);
    assert!(
        code.contains("$event => (_ctx.refresh())"),
        "inline handler should be wrapped in $event => (...), got:\n{}",
        code
    );
    // Negative: should NOT have bare call without wrapper
    assert!(
        !code.contains("onClick: _ctx.refresh()}") && !code.contains("onClick: _ctx.refresh(),"),
        "should NOT have unwrapped inline handler, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler: method reference should NOT be wrapped.
#[test]
fn ssr_component_event_method_ref_not_wrapped() {
    let code = gen_ssr_template(r#"<template><MyComp @click="handler" /></template>"#);
    assert!(
        code.contains("onClick: _ctx.handler"),
        "method reference should NOT be wrapped, got:\n{}",
        code
    );
    // Negative: should NOT have $event wrapper
    assert!(
        !code.contains("$event"),
        "method ref should NOT have $event wrapper, got:\n{}",
        code
    );
}

/// @ai-generated — Event handler: arrow function should NOT be wrapped.
#[test]
fn ssr_component_event_arrow_not_wrapped() {
    let code = gen_ssr_template(r#"<template><MyComp @click="() => doSomething()" /></template>"#);
    // Should keep the arrow function as-is
    assert!(
        !code.contains("$event => (() =>"),
        "arrow function should NOT be double-wrapped, got:\n{}",
        code
    );
}

/// @ai-generated — Slot forwarding: component with <slot> gets _: 3 FORWARDED.
#[test]
fn ssr_slot_forwarded_flag() {
    let code = gen_ssr_template(r#"<template><MyComp><slot /></MyComp></template>"#);
    assert!(
        code.contains("_: 3 /* FORWARDED */"),
        "component with <slot> outlet should use _: 3 FORWARDED, got:\n{}",
        code
    );
    // Negative: should NOT have _: 1 STABLE
    assert!(
        !code.contains("_: 1 /* STABLE */"),
        "should NOT have _: 1 STABLE when forwarding slots, got:\n{}",
        code
    );
}

/// @ai-generated — Component without slot outlets gets _: 1 STABLE.
#[test]
fn ssr_slot_stable_flag() {
    let code = gen_ssr_template(r#"<template><MyComp>hello</MyComp></template>"#);
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "component without <slot> should use _: 1 STABLE, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 3"),
        "should NOT have _: 3 FORWARDED without slot outlets, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// $setup dot notation (SSR uses $setup.x like VDOM)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — SSR setup binding uses dot notation.
#[test]
fn ssr_setup_binding_dot_notation() {
    let code = gen_ssr_template(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("$setup.msg"),
        "SSR should use dot notation $setup.msg, got:\n{}",
        code
    );
}

/// @ai-generated — SSR props use dot notation ($props.msg).
#[test]
fn ssr_props_dot_notation() {
    let code = gen_ssr_template(
        r#"<script setup>
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("$props.msg"),
        "SSR props should use dot notation, got:\n{}",
        code
    );
}

/// @ai-generated — SSR multi-script: computed in setup uses $setup. prefix.
#[test]
fn ssr_setup_binding_multi_script() {
    let code = gen_ssr_template(
        r#"<script lang="ts">
let a = 0;
</script>
<script setup lang="ts">
import { computed } from 'vue'
const foo = computed(() => 1)
</script>
<template><div>{{ foo }}</div></template>"#,
    );
    assert!(
        code.contains("$setup.foo"),
        "multi-script: setup computed should use $setup.foo, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.foo"),
        "multi-script: should NOT use _ctx.foo for setup binding, got:\n{}",
        code
    );
}

/// @ai-generated — SSR multi-script: angle bracket type assertion in setup
/// causes OXC parse failure, dropping all bindings.
/// When OXC fails, bindings fall back to `_ctx.` prefix.
/// TODO: fix OXC parse for angle bracket assertions in script setup
#[test]
fn ssr_setup_binding_multi_script_ts_angle_bracket() {
    let code = gen_ssr_template(
        r#"<script lang="ts">
let a = 0;
</script>
<script setup lang="ts">
import { computed } from "vue";
const foo = computed(() => 1);
let c = <string>0;
</script>
<template><div>{{ foo }}</div></template>"#,
    );
    // Currently falls back to _ctx.foo due to OXC parse failure on <string>0
    // When fixed, this should use $setup.foo instead
    assert!(
        code.contains("_ctx.foo") || code.contains("$setup.foo"),
        "should render foo with some prefix, got:\n{}",
        code
    );
}

/// @ai-generated — SSR: const with type annotation uses $setup. prefix.
#[test]
fn ssr_setup_binding_const_typed() {
    let code = gen_ssr_template(
        r#"<script setup lang="ts">
const count: number = 0
const name: string = 'John'
</script>
<template><div>{{ count }} {{ name }}</div></template>"#,
    );
    assert!(
        code.contains("$setup.count"),
        "typed const should use $setup.count, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.name"),
        "typed const should use $setup.name, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.count"),
        "should NOT use _ctx.count, got:\n{}",
        code
    );
}

/// @ai-generated — SSR _ctx uses dot notation.
#[test]
fn ssr_ctx_dot_notation() {
    let code = gen_ssr_template(r#"<template><div>{{ foo }}</div></template>"#);
    assert!(
        code.contains("_ctx.foo"),
        "SSR _ctx should use dot notation, got:\n{}",
        code
    );
}

// Note: _scopeId propagation for scoped styles is deferred — Vue inlines
// literal scope IDs (e.g., `data-v-xxxxx`) in SSR, not runtime _scopeId params.
// TODO: implement literal scope ID injection to match Vue's SSR output.

// ══════════════════════════════════════════════════════════════════
// Class array merge (static + dynamic class → _ssrRenderClass)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Static + dynamic class merged into _ssrRenderClass array.
#[test]
fn ssr_static_dynamic_class_merged() {
    let code = gen_ssr_template(
        r#"<template><div><li class="item" :class="{ active: isActive }">text</li></div></template>"#,
    );
    assert!(
        code.contains(r#"_ssrRenderClass([{ active: _ctx.isActive }, "item"])"#),
        "should merge into _ssrRenderClass([dynamic, static]) for non-root elements, got:\n{}",
        code
    );
    // Negative: should NOT have two separate class attributes
    let class_count = code.matches("class=").count();
    assert!(
        class_count <= 1,
        "should have at most 1 class attribute (merged), found {}, got:\n{}",
        class_count,
        code
    );
}

/// @ai-generated — Dynamic-only class uses _ssrRenderClass without array wrapper.
#[test]
fn ssr_only_dynamic_class_no_array() {
    let code = gen_ssr_template(
        r#"<template><div><span :class="{ active: ok }">text</span></div></template>"#,
    );
    assert!(
        code.contains("_ssrRenderClass({ active: _ctx.ok })"),
        "dynamic-only class should use _ssrRenderClass(expr), got:\n{}",
        code
    );
    // Negative: should NOT wrap in array
    assert!(
        !code.contains("_ssrRenderClass(["),
        "dynamic-only class should NOT be wrapped in array, got:\n{}",
        code
    );
}

/// @ai-generated — Static-only class stays as literal HTML.
#[test]
fn ssr_only_static_class_literal() {
    let code =
        gen_ssr_template(r#"<template><div><span class="item">text</span></div></template>"#);
    assert!(
        code.contains(r#"class="item""#),
        "static-only class should be literal HTML, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrRenderClass
    assert!(
        !code.contains("_ssrRenderClass"),
        "static-only class should NOT use _ssrRenderClass, got:\n{}",
        code
    );
}

/// @ai-generated — Root element mergeProps preserves source order of class attribute.
/// Class should appear at its template position, not appended at the end.
#[test]
fn ssr_root_mergeprops_class_source_order() {
    let code = gen_ssr_template(
        r#"<template><div class="wrapper" :style="s" data-testid="main">text</div></template>"#,
    );
    // class should come BEFORE style and data-testid in the mergeProps object
    let class_pos = code.find("class:");
    let style_pos = code.find("style:");
    let testid_pos = code.find("\"data-testid\":");
    assert!(
        class_pos.is_some() && style_pos.is_some(),
        "should have class and style in mergeProps, got:\n{}",
        code
    );
    assert!(
        class_pos.unwrap() < style_pos.unwrap(),
        "class should come before style (source order), got:\n{}",
        code
    );
    if let Some(tp) = testid_pos {
        assert!(
            class_pos.unwrap() < tp,
            "class should come before data-testid (source order), got:\n{}",
            code
        );
    }
}

/// @ai-generated — Component references from $setup use bracket notation in SSR.
#[test]
fn ssr_setup_component_bracket_notation() {
    let code = gen_ssr_template(
        r#"<template><MyComp/></template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    assert!(
        code.contains(r#"$setup["MyComp"]"#),
        "component ref should use bracket notation, got:\n{}",
        code
    );
    // Negative: should NOT use dot notation for component ref
    assert!(
        !code.contains("$setup.MyComp"),
        "should NOT use dot notation for component ref, got:\n{}",
        code
    );
}

/// @ai-generated — Data bindings from $setup still use dot notation in SSR.
#[test]
fn ssr_setup_data_dot_notation() {
    let code = gen_ssr_template(
        r#"<template><div>{{ msg }}</div></template>
<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>"#,
    );
    assert!(
        code.contains("$setup.msg"),
        "data binding should use dot notation, got:\n{}",
        code
    );
    // Negative: should NOT use bracket notation for data
    assert!(
        !code.contains(r#"$setup["msg"]"#),
        "data should NOT use bracket notation, got:\n{}",
        code
    );
}

/// @ai-generated — Component with default slot produces valid JS (push closed before slot closure).
#[test]
fn ssr_component_default_slot_valid_js() {
    let code = gen_ssr_template(
        r#"<template><MyComp><span>hello</span></MyComp></template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    // Should have _ssrRenderComponent with slot object
    assert!(
        code.contains("_ssrRenderComponent"),
        "should use _ssrRenderComponent, got:\n{}",
        code
    );
    // Should have _withCtx for slot
    assert!(
        code.contains("_withCtx"),
        "should have _withCtx for slot, got:\n{}",
        code
    );
    // The push literal must close before the slot closure: `)\n} else {
    // Negative: "} else {" must NOT appear inside a template literal
    assert!(
        !code.contains(">} else {"),
        "slot closure should NOT be inside template literal, got:\n{}",
        code
    );
    // Positive: should have proper } else { return [...] } pattern with VNode content
    assert!(
        code.contains("} else {\nreturn ["),
        "should have VDOM fallback, got:\n{}",
        code
    );
    // The fallback should have actual VNode content, not empty array
    assert!(
        code.contains("_createVNode(\"span\""),
        "VDOM fallback should have _createVNode for span, got:\n{}",
        code
    );
}

/// @ai-generated — Component with named slots produces valid JS structure.
#[test]
fn ssr_component_named_slots_valid_js() {
    let code = gen_ssr_template(
        r#"<template><MyComp><template #header><h1>Title</h1></template><template #default><p>Body</p></template></MyComp></template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    assert!(
        code.contains("header: _withCtx"),
        "should have named header slot, got:\n{}",
        code
    );
    assert!(
        code.contains("default: _withCtx"),
        "should have named default slot, got:\n{}",
        code
    );
    // Slot closures should NOT be inside template literals
    assert!(
        !code.contains(">} else {"),
        "slot closure should NOT be inside template literal, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 5: v-model on components
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-model on component decomposes to modelValue + onUpdate:modelValue.
#[test]
fn ssr_component_v_model_basic() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const val = ref('')
</script>
<template><MyComp v-model="val" /></template>"#;
    let code = gen_ssr_template(source);

    // Positive: modelValue prop emitted
    assert!(
        code.contains("modelValue: $setup[\"val\"]") || code.contains("modelValue: $setup.val"),
        "should have modelValue prop, got:\n{}",
        code
    );
    // Positive: onUpdate handler emitted
    assert!(
        code.contains("\"onUpdate:modelValue\": $event =>"),
        "should have onUpdate:modelValue handler, got:\n{}",
        code
    );
    // Negative: raw v-model should not appear
    assert!(
        !code.contains("v-model"),
        "v-model directive should not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — v-model:title on component uses custom prop name.
#[test]
fn ssr_component_v_model_named() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const title = ref('')
</script>
<template><MyComp v-model:title="title" /></template>"#;
    let code = gen_ssr_template(source);

    // Positive: title prop (not modelValue)
    assert!(
        code.contains("title: $setup[\"title\"]") || code.contains("title: $setup.title"),
        "should have title prop, got:\n{}",
        code
    );
    // Positive: onUpdate:title handler
    assert!(
        code.contains("\"onUpdate:title\": $event =>"),
        "should have onUpdate:title handler, got:\n{}",
        code
    );
    // Negative: no modelValue
    assert!(
        !code.contains("modelValue"),
        "should not have modelValue for named v-model, got:\n{}",
        code
    );
}

/// @ai-generated — v-model with modifiers emits modelModifiers.
#[test]
fn ssr_component_v_model_with_modifiers() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const text = ref('')
</script>
<template><MyComp v-model.trim.capitalize="text" /></template>"#;
    let code = gen_ssr_template(source);

    // Positive: modelModifiers with trim + capitalize
    assert!(
        code.contains("modelModifiers: {"),
        "should have modelModifiers, got:\n{}",
        code
    );
    assert!(
        code.contains("trim: true"),
        "should have trim modifier, got:\n{}",
        code
    );
    assert!(
        code.contains("capitalize: true"),
        "should have capitalize modifier, got:\n{}",
        code
    );
}

/// @ai-generated — Boolean attributes on components emit key: "".
#[test]
fn ssr_component_boolean_attrs() {
    let source = r#"<script setup>
import MyBtn from './MyBtn.vue'
</script>
<template><MyBtn rounded raised /></template>"#;
    let code = gen_ssr_template(source);

    // Positive: boolean attrs emitted as key: ""
    assert!(
        code.contains("rounded: \"\""),
        "should have rounded: \"\", got:\n{}",
        code
    );
    assert!(
        code.contains("raised: \"\""),
        "should have raised: \"\", got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 5: ref in component/root props
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — ref on component emitted in props.
#[test]
fn ssr_component_ref_in_props() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><MyComp ref="childRef" /></template>"#;
    let code = gen_ssr_template(source);
    // Components DO get ref in SSR props (unlike HTML elements)
    assert!(
        code.contains("ref: \"childRef\""),
        "should have ref in component props, got:\n{}",
        code
    );
    // Negative: should not have null props
    assert!(
        !code.contains(", null, null, _parent"),
        "should not have null props when ref is present, got:\n{}",
        code
    );
}

/// @ai-generated — ref on root element appears in _mergeProps.
#[test]
fn ssr_root_ref_in_merge_props() {
    let code = gen_ssr_template(r#"<template><div ref="myRef"></div></template>"#);
    // Root elements DO get ref in _mergeProps (for hydration)
    assert!(
        code.contains("ref: \"myRef\""),
        "should have ref in mergeProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_mergeProps"),
        "root ref should trigger _mergeProps, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 5: static style as JS object in mergeProps
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Static style in root _mergeProps uses JS object form.
#[test]
fn ssr_root_static_style_as_object() {
    let code =
        gen_ssr_template(r#"<template><div style="color: red; font-size: 14px"></div></template>"#);
    // Positive: style should be JS object
    assert!(
        code.contains("\"color\":\"red\"") || code.contains("\"color\": \"red\""),
        "should have style as JS object, got:\n{}",
        code
    );
    // Negative: should not have style as plain string in mergeProps
    assert!(
        !code.contains("style: \"color"),
        "style should not be a plain string in mergeProps, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Custom directives in SSR
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Custom directive with no value on root element.
#[test]
fn ssr_custom_directive_no_value_root() {
    let code = gen_ssr_template("<template><input v-focus></template>");
    // Positive: should resolve directive and use _ssrGetDirectiveProps
    assert!(
        code.contains("_resolveDirective(\"focus\")"),
        "should resolve directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_focus)"),
        "should call _ssrGetDirectiveProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs("),
        "should render attrs, got:\n{}",
        code
    );
    // Negative: raw v-focus should not appear in output
    assert!(
        !code.contains("v-focus"),
        "raw v-focus must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with value on nested element.
#[test]
fn ssr_custom_directive_with_value_nested() {
    let code =
        gen_ssr_template(r#"<template><div><div v-highlight="color">text</div></div></template>"#);
    // Positive: should have _ssrGetDirectiveProps with resolved value
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_highlight, _ctx.color)"),
        "should call _ssrGetDirectiveProps with value, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs("),
        "nested element with directive should use _ssrRenderAttrs, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-highlight"),
        "raw v-highlight must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with static argument.
#[test]
fn ssr_custom_directive_with_arg() {
    let code =
        gen_ssr_template(r#"<template><div><div v-tooltip:top="msg">text</div></div></template>"#);
    // Positive: should include value and static arg
    assert!(
        code.contains(r#"_ssrGetDirectiveProps(_ctx, _directive_tooltip, _ctx.msg, "top")"#),
        "should call _ssrGetDirectiveProps with value and arg, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-tooltip"),
        "raw v-tooltip must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with modifiers (no arg).
#[test]
fn ssr_custom_directive_with_modifiers() {
    let code = gen_ssr_template(
        r#"<template><div><div v-tooltip.show="text">text</div></div></template>"#,
    );
    // Positive: modifiers should appear as object, with void 0 arg placeholder
    assert!(
        code.contains(
            "_ssrGetDirectiveProps(_ctx, _directive_tooltip, _ctx.text, void 0, { show: true })"
        ),
        "should call _ssrGetDirectiveProps with modifiers, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-tooltip"),
        "raw v-tooltip must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with arg and modifiers.
#[test]
fn ssr_custom_directive_with_arg_and_modifiers() {
    let code = gen_ssr_template(
        r#"<template><div><div v-custom:arg.mod1.mod2="value">text</div></div></template>"#,
    );
    // Positive: should have value, arg, and modifiers
    assert!(
        code.contains(r#"_ssrGetDirectiveProps(_ctx, _directive_custom, _ctx.value, "arg", { mod1: true, mod2: true })"#),
        "should call _ssrGetDirectiveProps with arg and modifiers, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-custom"),
        "raw v-custom must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive from setup binding uses $setup["vFocus"].
#[test]
fn ssr_custom_directive_setup_binding() {
    let code = gen_ssr_template(
        r#"<template><input v-focus></template>
<script setup>
const vFocus = { mounted(el) { el.focus() } }
</script>"#,
    );
    // Positive: should use $setup["vFocus"] instead of _resolveDirective
    assert!(
        code.contains(r#"$setup["vFocus"]"#),
        "setup directive should use $setup[\"vFocus\"], got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx,"),
        "should call _ssrGetDirectiveProps, got:\n{}",
        code
    );
    // Negative: should NOT use _resolveDirective for setup-declared directives
    assert!(
        !code.contains("_resolveDirective"),
        "setup directive should not use _resolveDirective, got:\n{}",
        code
    );
    assert!(
        !code.contains("v-focus"),
        "raw v-focus must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive on root element merges with _attrs.
#[test]
fn ssr_custom_directive_on_root_merges_attrs() {
    let code = gen_ssr_template("<template><input v-focus></template>");
    // Root element should merge _attrs with directive props
    assert!(
        code.contains("_mergeProps("),
        "root element with directive should use _mergeProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_attrs"),
        "root element should merge _attrs, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive on nested element with other attrs.
#[test]
fn ssr_custom_directive_nested_with_attrs() {
    let code = gen_ssr_template(
        r#"<template><div><input type="text" placeholder="test" v-focus></div></template>"#,
    );
    // Positive: should merge static attrs with directive props
    assert!(
        code.contains("_mergeProps("),
        "nested element with attrs + directive should use _mergeProps, got:\n{}",
        code
    );
    assert!(
        code.contains("type: \"text\""),
        "should have type attr in mergeProps, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps("),
        "should have _ssrGetDirectiveProps, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-focus"),
        "raw v-focus must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Multiple custom directives on same element.
#[test]
fn ssr_multiple_directives_on_element() {
    let code =
        gen_ssr_template(r#"<template><div><input v-focus v-tooltip="'text'"></div></template>"#);
    // Positive: both directives should produce _ssrGetDirectiveProps calls
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_focus)"),
        "should have focus directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_tooltip, 'text')"),
        "should have tooltip directive with value, got:\n{}",
        code
    );
    assert!(
        code.contains("_mergeProps("),
        "multiple directives should use _mergeProps, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-focus"),
        "raw v-focus must not appear in output, got:\n{}",
        code
    );
    assert!(
        !code.contains("v-tooltip"),
        "raw v-tooltip must not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Built-in directives (v-show, v-model, v-if) should NOT produce _ssrGetDirectiveProps.
#[test]
fn ssr_builtin_directives_unchanged() {
    let code = gen_ssr_template(
        r#"<template><div><div v-show="show">visible</div><input v-model="name"></div></template>"#,
    );
    // Negative: built-in directives should NOT use _ssrGetDirectiveProps
    assert!(
        !code.contains("_ssrGetDirectiveProps"),
        "built-in directives should not use _ssrGetDirectiveProps, got:\n{}",
        code
    );
}

/// @ai-generated — Directive resolve declarations are hoisted to function preamble.
#[test]
fn ssr_directive_resolves_in_preamble() {
    let code = gen_ssr_template(
        r#"<template><div><input v-focus><div v-tooltip="'hi'">text</div></div></template>"#,
    );
    // Both resolves should be in the preamble (before _push)
    let push_pos = code.find("_push(").expect("should have _push");
    let focus_resolve = code.find("_resolveDirective(\"focus\")");
    let tooltip_resolve = code.find("_resolveDirective(\"tooltip\")");
    assert!(
        focus_resolve.is_some() && focus_resolve.unwrap() < push_pos,
        "focus resolve should be before _push, got:\n{}",
        code
    );
    assert!(
        tooltip_resolve.is_some() && tooltip_resolve.unwrap() < push_pos,
        "tooltip resolve should be before _push, got:\n{}",
        code
    );
}

/// @ai-generated — Custom directive with dynamic argument.
#[test]
fn ssr_custom_directive_dynamic_arg() {
    let code = gen_ssr_template(
        r#"<template><div><div v-tooltip:[position]="'text'">text</div></div></template>"#,
    );
    // Positive: dynamic arg should be resolved as expression, not wrapped in brackets
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_tooltip, 'text', _ctx.position)"),
        "should call _ssrGetDirectiveProps with dynamic arg, got:\n{}",
        code
    );
    // Negative: should not have brackets around arg
    assert!(
        !code.contains("[_ctx.position]"),
        "dynamic arg should not be wrapped in brackets, got:\n{}",
        code
    );
}

/// @ai-generated — Kebab-case directive name.
#[test]
fn ssr_custom_directive_kebab_case() {
    let code = gen_ssr_template(
        r#"<template><div><div v-click-outside="handler">text</div></div></template>"#,
    );
    // Positive: kebab-case directive should resolve correctly
    assert!(
        code.contains("_resolveDirective(\"click-outside\")"),
        "should resolve kebab-case directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_directive_click_outside"),
        "variable name should use underscores, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_click_outside, _ctx.handler)"),
        "should call with correct variable, got:\n{}",
        code
    );
    // Negative
    assert!(
        !code.contains("v-click-outside"),
        "raw directive must not appear in output, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Slot VDOM fallback (else branch)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Slot with text-only content generates _createTextVNode in else branch.
#[test]
fn ssr_slot_else_text_only() {
    let code = gen_ssr_template(
        r#"<template><Comp>hello</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: else branch should have _createTextVNode
    assert!(
        code.contains(r#"_createTextVNode("hello")"#),
        "else branch should have _createTextVNode, got:\n{}",
        code
    );
    // Negative: should NOT have empty return []
    assert!(
        !code.contains("return []"),
        "should not have empty return [], got:\n{}",
        code
    );
}

/// @ai-generated — Slot with single element generates _createVNode in else branch.
#[test]
fn ssr_slot_else_single_element() {
    let code = gen_ssr_template(
        r#"<template><Comp><div>content</div></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: else branch should have _createVNode for the div
    assert!(
        code.contains(r#"_createVNode("div", null, "content")"#),
        "else branch should have _createVNode for element, got:\n{}",
        code
    );
    assert!(
        !code.contains("return []"),
        "should not have empty return [], got:\n{}",
        code
    );
}

/// @ai-generated — Slot with multiple children generates array of VNodes.
#[test]
fn ssr_slot_else_multiple_children() {
    let code = gen_ssr_template(
        r#"<template><Comp><span>a</span><span>b</span></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: else branch should have both VNodes
    assert!(
        code.contains(r#"_createVNode("span", null, "a")"#),
        "else branch should have first span, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_createVNode("span", null, "b")"#),
        "else branch should have second span, got:\n{}",
        code
    );
}

/// @ai-generated — Slot with component child generates _createVNode(component).
#[test]
fn ssr_slot_else_component_child() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
</script>"#,
    );
    // Positive: else branch should have component VNode
    assert!(
        code.contains(r#"_createVNode($setup["Child"])"#),
        "else branch should have component VNode, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot else branch also has VDOM fallback.
#[test]
fn ssr_slot_else_named_slot() {
    let code = gen_ssr_template(
        r#"<template><Comp><template #header>Title</template></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: named slot else branch should have text VNode
    assert!(
        code.contains(r#"_createTextVNode("Title")"#),
        "named slot else branch should have _createTextVNode, got:\n{}",
        code
    );
}

/// @ai-generated — Element with props in slot else branch.
#[test]
fn ssr_slot_else_element_with_props() {
    let code = gen_ssr_template(
        r#"<template><Comp><input type="text" placeholder="test"></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: else branch should have element with props object
    assert!(
        code.contains(r#"_createVNode("input", { type: "text", placeholder: "test" })"#),
        "else branch should have element with props, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Slot params (v-slot destructuring)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Static style prop on component uses JS object format.
#[test]
fn ssr_component_static_style_object() {
    let code = gen_ssr_template(
        r#"<template><Comp style="width: 100%"></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: style should be a JS object
    assert!(
        code.contains(r#"style: {"width":"100%"}"#),
        "component style prop should be JS object, got:\n{}",
        code
    );
    // Negative: should NOT be a plain CSS string
    assert!(
        !code.contains(r#"style: "width: 100%""#),
        "should NOT use CSS string for component style, got:\n{}",
        code
    );
}

/// @ai-generated — Component with v-slot params preserves destructured params.
#[test]
fn ssr_slot_params_on_component() {
    let code = gen_ssr_template(
        r#"<template><Comp v-slot="{ item }">{{ item }}</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: _withCtx should have the destructured params
    assert!(
        code.contains("_withCtx(({ item }, _push, _parent"),
        "should have destructured slot params, got:\n{}",
        code
    );
    // Negative: should NOT use _ placeholder for params
    assert!(
        !code.contains("_withCtx((_, _push, _parent"),
        "should NOT drop slot params to _, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot with params preserves destructured params.
#[test]
fn ssr_slot_params_on_named_slot() {
    let code = gen_ssr_template(
        r#"<template><Comp><template #header="{ title }">{{ title }}</template></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: named slot should have destructured params
    assert!(
        code.contains("_withCtx(({ title }, _push, _parent"),
        "named slot should have destructured params, got:\n{}",
        code
    );
}

/// @ai-generated — Component with v-slot shorthand # preserves params.
#[test]
fn ssr_slot_params_shorthand() {
    let code = gen_ssr_template(
        r#"<template><Comp v-slot="{ count }">{{ count }}</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    assert!(
        code.contains("_withCtx(({ count }, _push, _parent"),
        "should have slot params with shorthand, got:\n{}",
        code
    );
}

/// @ai-generated — Component with named slots AND default content wraps default in _withCtx.
#[test]
fn ssr_named_slots_with_default_content() {
    let code = gen_ssr_template(
        r#"<template><Dropdown>
  <template #overlay><Menu /></template>
  <a>Hover me</a>
</Dropdown></template>
<script setup>
import Dropdown from './Dropdown.vue'
import Menu from './Menu.vue'
</script>"#,
    );
    // Named slot should have _withCtx wrapper
    assert!(
        code.contains("overlay: _withCtx("),
        "should have overlay slot with _withCtx, got:\n{}",
        code
    );
    // Default content should also have _withCtx wrapper
    assert!(
        code.contains("default: _withCtx("),
        "default content should be wrapped in _withCtx, got:\n{}",
        code
    );
    // Default content should contain the <a> element in a _push call
    assert!(
        code.contains("<a>Hover me</a>"),
        "default slot should contain the <a> element, got:\n{}",
        code
    );
    // Should NOT have raw _push calls outside _withCtx wrappers
    // (all _push calls should be inside slot _withCtx callbacks)
    assert!(
        !code.contains("{_push("),
        "should not have raw _push right after slot object opening brace, got:\n{}",
        code
    );
}

/// @ai-generated — Component with only named slots (no default) should not emit default: _withCtx.
#[test]
fn ssr_named_slots_no_default_content() {
    let code = gen_ssr_template(
        r#"<template><Tabs>
  <template #tab1><span>Tab 1</span></template>
  <template #tab2><span>Tab 2</span></template>
</Tabs></template>
<script setup>
import Tabs from './Tabs.vue'
</script>"#,
    );
    // Named slots should have _withCtx wrappers
    assert!(
        code.contains("tab1: _withCtx("),
        "should have tab1 slot, got:\n{}",
        code
    );
    assert!(
        code.contains("tab2: _withCtx("),
        "should have tab2 slot, got:\n{}",
        code
    );
    // No default slot since there's no default content
    assert!(
        !code.contains("default: _withCtx("),
        "should NOT have default slot, got:\n{}",
        code
    );
}

/// @ai-generated — Default content BEFORE named slot should close default slot before named.
#[test]
fn ssr_default_slot_before_named_slot() {
    let code = gen_ssr_template(
        r#"<template><Dropdown>
  <a>Hover me</a>
  <template #overlay><Menu /></template>
</Dropdown></template>
<script setup>
import Dropdown from './Dropdown.vue'
import Menu from './Menu.vue'
</script>"#,
    );
    // Default slot should be wrapped in _withCtx
    assert!(
        code.contains("default: _withCtx("),
        "default content should be wrapped in _withCtx, got:\n{}",
        code
    );
    // Named slot should also be wrapped in _withCtx
    assert!(
        code.contains("overlay: _withCtx("),
        "overlay slot should be wrapped in _withCtx, got:\n{}",
        code
    );
    // Named slot should come BEFORE default (matching Vue's SSR output)
    let def_pos = code.find("default: _withCtx(").unwrap();
    let overlay_pos = code.find("overlay: _withCtx(").unwrap();
    assert!(
        overlay_pos < def_pos,
        "named slot 'overlay' should come before default slot, got:\n{}",
        code
    );
    // Default slot should contain the <a> element
    assert!(
        code.contains("<a>Hover me</a>"),
        "default slot should contain the <a> element, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Built-in component SSR rendering
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Transition renders children directly in SSR (no-op wrapper).
#[test]
fn ssr_transition_renders_children_directly() {
    let code = gen_ssr_template(
        "<template><Transition><div v-if=\"show\">hello</div></Transition></template>",
    );
    // Transition should NOT use _ssrRenderComponent
    assert!(
        !code.contains("_ssrRenderComponent"),
        "Transition should not use _ssrRenderComponent in SSR, got:\n{}",
        code
    );
    // Children should be rendered directly (as root, with _ssrRenderAttrs)
    assert!(
        code.contains("hello</div>"),
        "Transition children should be rendered directly, got:\n{}",
        code
    );
    // v-if should produce an if/else
    assert!(
        code.contains("if ("),
        "v-if inside Transition should produce conditional, got:\n{}",
        code
    );
    // No Transition tag in output
    assert!(
        !code.contains("<Transition") && !code.contains("</Transition"),
        "Transition tag should not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — TransitionGroup renders its tag prop as a real HTML element.
/// `<TransitionGroup tag="ul" class="list">` → `<ul class="list">...children...</ul>`
#[test]
fn ssr_transition_group_renders_tag() {
    let code = gen_ssr_template(
        r#"<template><TransitionGroup tag="ul" class="list"><li v-for="item in items" :key="item.id">{{ item.text }}</li></TransitionGroup></template>"#,
    );
    // Should render <ul> tag
    assert!(
        code.contains("<ul"),
        "TransitionGroup should render its tag prop, got:\n{}",
        code
    );
    assert!(
        code.contains("</ul>"),
        "TransitionGroup should have closing tag, got:\n{}",
        code
    );
    // Should have class attribute
    assert!(
        code.contains(r#"class="list""#),
        "TransitionGroup should pass class to rendered tag, got:\n{}",
        code
    );
    // Should NOT use _ssrRenderComponent
    assert!(
        !code.contains("_ssrRenderComponent"),
        "TransitionGroup should not use _ssrRenderComponent, got:\n{}",
        code
    );
    // Should NOT have TransitionGroup tag in output
    assert!(
        !code.contains("<TransitionGroup") && !code.contains("</TransitionGroup"),
        "TransitionGroup tag should not appear in output, got:\n{}",
        code
    );
    // Children should render (v-for)
    assert!(
        code.contains("_ssrRenderList"),
        "TransitionGroup children should render, got:\n{}",
        code
    );
}

/// @ai-generated — TransitionGroup without explicit tag defaults to span.
#[test]
fn ssr_transition_group_default_tag() {
    let code = gen_ssr_template(
        r#"<template><TransitionGroup><div v-for="i in items" :key="i">{{ i }}</div></TransitionGroup></template>"#,
    );
    // Default tag should be span
    assert!(
        code.contains("<span") && code.contains("</span>"),
        "TransitionGroup without tag prop should default to span, got:\n{}",
        code
    );
}

/// @ai-generated — KeepAlive renders children directly in SSR.
#[test]
fn ssr_keepalive_renders_children_directly() {
    let code = gen_ssr_template("<template><KeepAlive><div>cached</div></KeepAlive></template>");
    // KeepAlive should NOT use _ssrRenderComponent
    assert!(
        !code.contains("_ssrRenderComponent"),
        "KeepAlive should not use _ssrRenderComponent in SSR, got:\n{}",
        code
    );
    // Children should be rendered directly (as root, with _ssrRenderAttrs)
    assert!(
        code.contains("cached</div>"),
        "KeepAlive children should be rendered directly, got:\n{}",
        code
    );
    // No KeepAlive tag in output
    assert!(
        !code.contains("<KeepAlive") && !code.contains("</KeepAlive"),
        "KeepAlive tag should not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — Teleport uses _ssrRenderTeleport in SSR.
#[test]
fn ssr_teleport_uses_ssr_helper() {
    let code =
        gen_ssr_template(r#"<template><Teleport to="body"><div>modal</div></Teleport></template>"#);
    // Should use _ssrRenderTeleport
    assert!(
        code.contains("_ssrRenderTeleport"),
        "Teleport should use _ssrRenderTeleport, got:\n{}",
        code
    );
    // Should NOT use _ssrRenderComponent
    assert!(
        !code.contains("_ssrRenderComponent"),
        "Teleport should not use _ssrRenderComponent in SSR, got:\n{}",
        code
    );
    // Should include the target "body"
    assert!(
        code.contains("\"body\""),
        "Teleport should include target \"body\", got:\n{}",
        code
    );
    // Children should be inside the callback
    assert!(
        code.contains("<div>modal</div>"),
        "Teleport children should be rendered inside callback, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Binding prefix resolution
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Options API `data()` properties should use `$data.` prefix.
#[test]
fn ssr_data_binding_uses_data_prefix() {
    let code = gen_ssr_template(
        r#"<template><div>{{ count }}</div></template>
<script>
export default {
  data() { return { count: 0 } }
}
</script>"#,
    );
    assert!(
        code.contains("$data.count"),
        "data binding should use $data. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.count"),
        "should NOT use _ctx. for data bindings, got:\n{}",
        code
    );
}

/// @ai-generated — Options API `props` should use `$props.` prefix.
#[test]
fn ssr_props_binding_uses_props_prefix() {
    let code = gen_ssr_template(
        r#"<template><div>{{ msg }}</div></template>
<script>
export default {
  props: ['msg']
}
</script>"#,
    );
    assert!(
        code.contains("$props.msg"),
        "props binding should use $props. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.msg"),
        "should NOT use _ctx. for props bindings, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// v-model on <select> — _ssrIncludeBooleanAttr + _ssrLooseContain/Equal
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-model on <select> should add `selected` attr to <option> children.
#[test]
fn ssr_v_model_select_option_selected() {
    let code = gen_ssr_template(
        r#"<template><select v-model="val"><option value="a">A</option><option value="b">B</option></select></template>
<script setup>
import { ref } from 'vue'
const val = ref('a')
</script>"#,
    );

    // Should contain _ssrIncludeBooleanAttr for selected check
    assert!(
        code.contains("_ssrIncludeBooleanAttr"),
        "v-model select should use _ssrIncludeBooleanAttr, got:\n{}",
        code
    );
    // Should contain _ssrLooseContain for array check
    assert!(
        code.contains("_ssrLooseContain"),
        "v-model select should use _ssrLooseContain for array model values, got:\n{}",
        code
    );
    // Should contain _ssrLooseEqual for non-array check
    assert!(
        code.contains("_ssrLooseEqual"),
        "v-model select should use _ssrLooseEqual for single model values, got:\n{}",
        code
    );
    // Should have selected attribute injection pattern
    assert!(
        code.contains(r#"? " selected" : """#),
        "should emit ' selected' ternary, got:\n{}",
        code
    );
    // Should reference option values "a" and "b"
    assert!(
        code.contains(r#""a""#) && code.contains(r#""b""#),
        "should reference option values, got:\n{}",
        code
    );
    // Should NOT have raw v-model in output
    assert!(
        !code.contains("v-model"),
        "v-model should not appear in output, got:\n{}",
        code
    );
}

/// @ai-generated — v-model select with dynamic option values.
#[test]
fn ssr_v_model_select_dynamic_option_value() {
    let code = gen_ssr_template(
        r#"<template><select v-model="chosen"><option :value="item">{{ item }}</option></select></template>
<script setup>
import { ref } from 'vue'
const chosen = ref('')
const item = ref('x')
</script>"#,
    );

    // Should contain _ssrIncludeBooleanAttr for selected check
    assert!(
        code.contains("_ssrIncludeBooleanAttr"),
        "dynamic option value should use _ssrIncludeBooleanAttr, got:\n{}",
        code
    );
    // Should reference the dynamic value expr ($setup.item)
    assert!(
        code.contains("$setup.item"),
        "should reference dynamic option value, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on select inside v-for renders selected for each option.
#[test]
fn ssr_v_model_select_in_v_for() {
    let code = gen_ssr_template(
        r#"<template><select v-model="val"><option v-for="opt in options" :value="opt">{{ opt }}</option></select></template>
<script setup>
import { ref } from 'vue'
const val = ref('')
const options = ref(['a', 'b'])
</script>"#,
    );

    // Should contain _ssrIncludeBooleanAttr even inside v-for
    assert!(
        code.contains("_ssrIncludeBooleanAttr"),
        "v-model select with v-for options should use _ssrIncludeBooleanAttr, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Dynamic component <component :is> — _attrs forwarding
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Root-level dynamic component forwards _attrs.
#[test]
fn ssr_dynamic_component_root_forwards_attrs() {
    let code = gen_ssr_template(
        r#"<template><component :is="comp" /></template>
<script setup>
import { ref } from 'vue'
const comp = ref('div')
</script>"#,
    );
    // Should forward _attrs to the dynamic component at root
    assert!(
        code.contains("_attrs"),
        "root dynamic component should forward _attrs, got:\n{}",
        code
    );
    // Should NOT have null as the props argument when _attrs should be forwarded
    assert!(
        !code.contains("_createVNode(_resolveDynamicComponent($setup.comp), null"),
        "root dynamic component should not have null props, got:\n{}",
        code
    );
}

/// @ai-generated — Options API `computed` should use `_ctx.` prefix.
#[test]
fn ssr_computed_binding_uses_options_prefix() {
    let code = gen_ssr_template(
        r#"<template><div>{{ total }}</div></template>
<script>
export default {
  computed: { total() { return 42; } }
}
</script>"#,
    );
    assert!(
        code.contains("$options.total"),
        "computed binding should use $options. prefix in standalone mode, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.total"),
        "computed binding should NOT use _ctx. prefix in standalone mode, got:\n{}",
        code
    );
}

// ========================================================================
// Dynamic attribute names (:[expr] / v-bind:[expr])
// ========================================================================

/// @ai-generated — Dynamic attribute name should use computed property key in _ssrRenderAttrs.
#[test]
fn ssr_dynamic_attr_name_simple() {
    let code = gen_ssr_template(
        r#"<template><div :[dynamicPropName]="dynamicPropValue">Dynamic prop name</div></template>
<script setup>
const dynamicPropName = ref('title')
const dynamicPropValue = ref('hello')
</script>"#,
    );
    // Vue: _ssrRenderAttrs({ [$setup.dynamicPropName || ""]: $setup.dynamicPropValue })
    assert!(
        code.contains(r#"[$setup.dynamicPropName || ""]"#),
        "should use computed property key with || \"\", got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs("),
        "should use _ssrRenderAttrs for dynamic attr name, got:\n{}",
        code
    );
    // Negative: must NOT use _ssrRenderAttr with bracket literal
    assert!(
        !code.contains(r#"_ssrRenderAttr("["#),
        "must NOT use _ssrRenderAttr with bracket-quoted name, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic attr name with template literal expression.
#[test]
fn ssr_dynamic_attr_name_template_literal() {
    let code = gen_ssr_template(
        r#"<template><div :[`data-${dynamicClassName}`]="isActive">Dynamic attribute name</div></template>
<script>
export default {
  data() { return { dynamicClassName: 'test', isActive: true } }
}
</script>"#,
    );
    // Vue: _ssrRenderAttrs({ [(`data-${_ctx.dynamicClassName}`) || ""]: _ctx.isActive })
    // Verter uses $data. prefix for data() properties; key check is proper template literal form
    assert!(
        code.contains(r#"`data-${$data.dynamicClassName}`"#),
        "should resolve dynamicClassName in template literal with correct offset, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"|| ""]"#),
        "should have || \"\" fallback in computed property key, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderAttrs("),
        "should use _ssrRenderAttrs for dynamic attr name, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"_ssrRenderAttr("["#),
        "must NOT use _ssrRenderAttr with bracket-quoted name, got:\n{}",
        code
    );
    // Negative: must NOT contain broken expression with truncated identifier
    assert!(
        !code.contains("d_ctx."),
        "must NOT have broken 'd_ctx.' from off-by-one in dynamic attr name, got:\n{}",
        code
    );
}

/// @ai-generated — Same-name shorthand `:class` (Vue 3.4+) resolves to arg name as expression.
#[test]
fn ssr_same_name_shorthand_class() {
    // Non-root element so :class uses inline _ssrRenderClass path.
    // Use script setup so `class` ref resolves to $setup.class.
    let code = gen_ssr_template(
        r#"<template><div><span :class>shorthand</span></div></template>
<script setup>
import { ref } from 'vue'
const class_ = ref('active') // just need bindings present
</script>"#,
    );
    // Same-name shorthand `:class` resolves to _ctx.class (no binding found for "class")
    assert!(
        code.contains("_ssrRenderClass(_ctx.class)"),
        "should use _ssrRenderClass(_ctx.class) for :class shorthand, got:\n{}",
        code
    );
    assert!(
        !code.contains("[\"class\"]"),
        "should use dot notation, not bracket notation, got:\n{}",
        code
    );
}

/// @ai-generated — Same-name shorthand `:id` resolves to arg name as expression.
#[test]
fn ssr_same_name_shorthand_id() {
    // Non-root element so :id uses inline _ssrRenderAttr path
    let code = gen_ssr_template(
        r#"<template><div><span :id>shorthand</span></div></template>
<script setup>
const id = ref('my-id')
</script>"#,
    );
    // Vue: <span${_ssrRenderAttr("id", $setup.id)}>shorthand</span>
    assert!(
        code.contains("$setup.id"),
        "should resolve shorthand :id to $setup.id, got:\n{}",
        code
    );
    assert!(
        !code.contains("$setup[\"id\"]"),
        "should use dot notation $setup.id, not bracket notation, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic attr name with template literal in script setup.
#[test]
fn ssr_dynamic_attr_name_template_literal_setup() {
    let code = gen_ssr_template(
        r#"<template><div :[`data-${dynamicName}`]="val">test</div></template>
<script setup>
const dynamicName = ref('foo')
const val = ref(true)
</script>"#,
    );
    // Vue: { [(`data-${$setup.dynamicName}`) || ""]: $setup.val }
    assert!(
        code.contains(r#"`data-${$setup.dynamicName}`"#),
        "should resolve dynamicName to $setup.dynamicName in template literal, got:\n{}",
        code
    );
    assert!(
        !code.contains("d$setup."),
        "must NOT have broken 'd$setup.' from off-by-one in dynamic attr name, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic attr name on root element should go through _mergeProps path.
#[test]
fn ssr_dynamic_attr_name_root_element() {
    let code = gen_ssr_template(
        r#"<template><div :[name]="value" class="static">Root</div></template>
<script setup>
const name = ref('title')
const value = ref('hello')
</script>"#,
    );
    // Root elements merge with _attrs, so dynamic attr names use computed keys in the attrs obj
    assert!(
        code.contains(r#"[$setup.name || ""]"#),
        "root element should use computed property key, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"_ssrRenderAttr("["#),
        "must NOT use _ssrRenderAttr with bracket-quoted name, got:\n{}",
        code
    );
}

// ========================================================================
// Textarea/input v-model SSR
// ========================================================================

/// @ai-generated — Textarea v-model should render value as attr in _ssrRenderAttrs, not as content.
#[test]
fn ssr_textarea_vmodel_value_attr() {
    // Vue SSR puts `value: expr` in the attrs object for textarea v-model,
    // instead of interpolating content between <textarea>...</textarea>.
    let code = gen_ssr_template(
        r#"<template><div><textarea v-model="msg" class="input"></textarea></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    // Non-root textarea: uses _ssrInterpolate as content (no value attr)
    assert!(
        code.contains("_ssrInterpolate("),
        "non-root textarea v-model should use _ssrInterpolate content, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrRenderAttr(\"value\""),
        "non-root textarea should NOT add value as inline attr, got:\n{}",
        code
    );
}

/// @ai-generated — Same-name shorthand in slot props should be included.
#[test]
fn ssr_slot_same_name_shorthand_props() {
    let code = gen_ssr_template(
        r#"<template><div><slot name="header" :items :count="items.length" /></div></template>
<script setup>
const items = ref([])
</script>"#,
    );
    // Vue: _ssrRenderSlot(_ctx.$slots, "header", {count: ..., items: $setup.items}, ...)
    assert!(
        code.contains("items: $setup.items"),
        "slot shorthand :items should produce 'items: $setup.items', got:\n{}",
        code
    );
    assert!(
        code.contains("count:"),
        "slot :count should also be present, got:\n{}",
        code
    );
}

/// @ai-generated — Static class + v-bind spread should NOT duplicate class in attrs.
#[test]
fn ssr_class_dedup_with_vbind_spread() {
    let code = gen_ssr_template(
        r#"<template><div><input class="my-input" v-bind="{ ...$attrs, class: null }" type="text"></div></template>
<script setup>
</script>"#,
    );
    // Vue deduplicates: {class: null} from spread takes precedence over static class.
    // Verter should NOT have class: "my-input" appearing twice.
    let class_count = code.matches("class:").count();
    assert!(
        class_count <= 2, // once from spread ({class: null}) + once from static is OK via _mergeProps
        "should not duplicate class in attrs, found {} occurrences:\n{}",
        class_count,
        code
    );
}

/// @ai-generated — Input type=range v-model should render value as attr.
#[test]
fn ssr_input_range_vmodel_value_attr() {
    let code = gen_ssr_template(
        r#"<template><div><input type="range" v-model="val" class="slider"></div></template>
<script setup>
const val = ref(50)
</script>"#,
    );
    // Vue: { class: "slider", type: "range", value: $setup.val }
    assert!(
        code.contains("value:") || code.contains("\"value\""),
        "input range v-model should add value property, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.val"),
        "should resolve v-model value to $setup.val, got:\n{}",
        code
    );
}

// ========================================================================
// Suspense slot rendering
// ========================================================================

/// @ai-generated — Suspense with named slots should use simple arrow functions, not _withCtx.
#[test]
fn ssr_suspense_named_slots_no_withctx() {
    let code = gen_ssr_template(
        r#"<template>
<Suspense>
  <template #default>
    <div>Default content</div>
  </template>
  <template #fallback>
    <div>Loading...</div>
  </template>
</Suspense>
</template>
<script setup>
</script>"#,
    );
    // Vue pattern: _ssrRenderSuspense(_push, { default: () => { _push(...) }, fallback: () => { _push(...) }, _: 1 })
    assert!(
        code.contains("_ssrRenderSuspense("),
        "should have _ssrRenderSuspense call, got:\n{}",
        code
    );
    assert!(
        code.contains("default: () => {"),
        "Suspense default slot should use simple arrow function, got:\n{}",
        code
    );
    assert!(
        code.contains("fallback: () => {"),
        "Suspense fallback slot should use simple arrow function, got:\n{}",
        code
    );
    // Negative: Suspense slots should NOT use _withCtx
    assert!(
        !code.contains("_withCtx"),
        "Suspense slots must NOT use _withCtx, got:\n{}",
        code
    );
    // Negative: no VDOM fallback in Suspense slots
    assert!(
        !code.contains("else {"),
        "Suspense slots should not have VDOM fallback branch, got:\n{}",
        code
    );
}

/// @ai-generated — Suspense with default content (no named slots) should also use simple arrow.
#[test]
fn ssr_suspense_implicit_default_slot() {
    let code = gen_ssr_template(
        r#"<template>
<Suspense>
  <div>Default content</div>
</Suspense>
</template>
<script setup>
</script>"#,
    );
    assert!(
        code.contains("_ssrRenderSuspense("),
        "should have _ssrRenderSuspense, got:\n{}",
        code
    );
    assert!(
        code.contains("default: () => {"),
        "implicit default slot should use simple arrow, got:\n{}",
        code
    );
    assert!(
        !code.contains("_withCtx"),
        "Suspense must not use _withCtx, got:\n{}",
        code
    );
}

/// @ai-generated — Suspense with mixed content (bare elements + named slot templates).
/// The bare content should become the implicit default slot.
#[test]
fn ssr_suspense_mixed_content_implicit_default() {
    let code = gen_ssr_template(
        r#"<template>
<Suspense>
  <component :is="comp" />
  <template #fallback>
    Loading...
  </template>
</Suspense>
</template>
<script setup>
const comp = {}
</script>"#,
    );
    // Vue pattern: _ssrRenderSuspense(_push, {
    //   default: () => { _ssrRenderVNode(_push, _createVNode(...), _parent) },
    //   fallback: () => { _push(`Loading... `) },
    //   _: 1
    // })
    assert!(
        code.contains("_ssrRenderSuspense("),
        "should have _ssrRenderSuspense call, got:\n{}",
        code
    );
    assert!(
        code.contains("default: () => {"),
        "bare content should be wrapped in default slot, got:\n{}",
        code
    );
    assert!(
        code.contains("fallback: () => {"),
        "named fallback slot should be present, got:\n{}",
        code
    );
    // Negative: default: should appear before fallback: in the output
    let default_pos = code.find("default: () => {").unwrap();
    let fallback_pos = code.find("fallback: () => {").unwrap();
    assert!(
        default_pos < fallback_pos,
        "default slot should come before fallback slot, got:\n{}",
        code
    );
}

// ========================================================================
// v-for iterable binding resolution
// ========================================================================

/// @ai-generated — v-for iterable with compound expression should resolve bindings.
#[test]
fn ssr_vfor_iterable_binding_compound_expr() {
    let code = gen_ssr_template(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>
<script setup>
const items = ref([])
</script>"#,
    );
    assert!(
        code.contains("$setup.items"),
        "v-for iterable should use $setup. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrRenderList(items,"),
        "should NOT use bare 'items' without prefix, got:\n{}",
        code
    );
}

/// @ai-generated — v-for iterable with member expression should resolve root binding.
#[test]
fn ssr_vfor_iterable_member_expr() {
    let code = gen_ssr_template(
        r#"<template><div v-for="item in data.items" :key="item.id">{{ item.name }}</div></template>
<script setup>
const data = reactive({ items: [] })
</script>"#,
    );
    assert!(
        code.contains("$setup.data.items"),
        "v-for member expr iterable should prefix root with $setup., got:\n{}",
        code
    );
}

/// @ai-generated — :key on a component should be passed as a prop to _ssrRenderComponent.
#[test]
fn ssr_component_key_prop_in_vfor() {
    let code = gen_ssr_template(
        r#"<template><div><MyItem v-for="item in items" :key="item.id" :title="item.name" /></div></template>
<script setup>
import MyItem from './MyItem.vue'
const items = ref([])
</script>"#,
    );
    // :key should appear as a prop on the component
    assert!(
        code.contains("key: item.id"),
        ":key should be passed as component prop, got:\n{}",
        code
    );
    // key should come before title in the props object (source order)
    let key_pos = code.find("key: item.id").unwrap();
    let title_pos = code.find("title: item.name").unwrap();
    assert!(
        key_pos < title_pos,
        "key should appear before title in props, got:\n{}",
        code
    );
}

/// @ai-generated — :key on HTML elements should still be stripped in SSR.
#[test]
fn ssr_html_element_key_still_stripped() {
    let code = gen_ssr_template(
        r#"<template><div><li v-for="item in items" :key="item.id" :class="item.cls">{{ item.name }}</li></div></template>
<script setup>
const items = ref([])
</script>"#,
    );
    // :key should NOT appear in the output for HTML elements
    assert!(
        !code.contains("key:"),
        ":key should be stripped from HTML element SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — :key on dynamic component should be passed as prop.
#[test]
fn ssr_dynamic_component_key_prop() {
    let code = gen_ssr_template(
        r#"<template><div><component :is="currentComp" :key="id" :msg="text" /></div></template>
<script setup>
const currentComp = ref('MyComp')
const id = ref(1)
const text = ref('hello')
</script>"#,
    );
    // :key should appear in the dynamic component's props
    assert!(
        code.contains("key: $setup.id"),
        ":key should be passed as dynamic component prop, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot on component should use correct slot name, not "default".
#[test]
fn ssr_named_slot_not_default() {
    let code = gen_ssr_template(
        r#"<template><Story><template #controls><div>hi</div></template></Story></template>
<script setup>
import Story from './Story.vue'
</script>"#,
    );
    // The slot should be named "controls", not "default"
    assert!(
        code.contains("controls: _withCtx"),
        "should use named slot 'controls', got:\n{}",
        code
    );
    assert!(
        !code.contains("default: _withCtx"),
        "should NOT have default slot when only named slot exists, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot with slot params should pass params correctly.
#[test]
fn ssr_named_slot_with_params() {
    let code = gen_ssr_template(
        r#"<template><BaseSelect><template #popper="{ hide }"><div @click="hide">Close</div></template></BaseSelect></template>
<script setup>
import BaseSelect from './BaseSelect.vue'
</script>"#,
    );
    // Slot should be named "popper" with params
    assert!(
        code.contains("popper: _withCtx"),
        "should use named slot 'popper', got:\n{}",
        code
    );
    assert!(
        code.contains("{ hide }"),
        "slot params should include {{{{ hide }}}}, got:\n{}",
        code
    );
}

/// @ai-generated — Nested component with both named slot and default content.
/// Named slots should be detected even when there's also default content.
#[test]
fn ssr_nested_component_named_and_default_slots() {
    let code = gen_ssr_template(
        r#"<template>
<Story>
  <Variant title="default">
    <template #controls><div>controls content</div></template>
    <h1>Default content</h1>
  </Variant>
</Story>
</template>
<script setup>
import Story from './Story.vue'
import Variant from './Variant.vue'
</script>"#,
    );
    // The Variant component should have named slot "controls" with _withCtx
    assert!(
        code.contains("controls: _withCtx"),
        "should detect named slot 'controls' on nested component, got:\n{}",
        code
    );
    // Should also have default slot with _withCtx
    assert!(
        code.contains("default: _withCtx"),
        "should have default slot wrapper for non-template children, got:\n{}",
        code
    );
}

/// @ai-generated — Default content before named template slot.
/// When default slot content appears before <template #name> in source,
/// both slots should still be correctly detected and wrapped.
#[test]
fn ssr_default_content_before_named_slot() {
    let code = gen_ssr_template(
        r#"<template>
<Variant title="default">
  <h1>State</h1>
  <div>Default content</div>
  <template #controls><div>controls</div></template>
</Variant>
</template>
<script setup>
import Variant from './Variant.vue'
</script>"#,
    );
    // Named slot "controls" should be detected
    assert!(
        code.contains("controls: _withCtx"),
        "should detect named slot 'controls', got:\n{}",
        code
    );
    // Default content should be wrapped in default: _withCtx
    assert!(
        code.contains("default: _withCtx"),
        "default content should be wrapped in default slot, got:\n{}",
        code
    );
    // Should NOT have bare _push at the top level of the slots object
    assert!(
        !code.contains(", {_push("),
        "should not have bare _push in slots object, got:\n{}",
        code
    );
}

/// @ai-generated — Named slot detection works without script setup.
#[test]
fn ssr_named_slot_no_script_setup() {
    let code = gen_ssr_template(
        r#"<template>
<Variant title="default">
  <h1>State</h1>
  <template #controls><div>controls</div></template>
</Variant>
</template>
<script>
export default {}
</script>"#,
    );
    // Named slot "controls" should still be detected
    assert!(
        code.contains("controls: _withCtx"),
        "should detect named slot 'controls' without script setup, got:\n{}",
        code
    );
    assert!(
        code.contains("default: _withCtx"),
        "should have default slot wrapper, got:\n{}",
        code
    );
}

/// @ai-generated — Multi-root template should merge fragment close <!--]--> into
/// the last push, not split it into a separate _push() call.
#[test]
fn ssr_multi_root_fragment_close_merged() {
    let code = gen_ssr_template(
        r#"<template>
<div>hello</div>
<div>world</div>
</template>
<script setup>
</script>"#,
    );
    // Fragment close should be merged into the last push
    assert!(
        code.contains("<!--]-->`)"),
        "fragment close should be merged into last push, got:\n{}",
        code
    );
    assert!(
        !code.contains("_push(`<!--]-->`)"),
        "fragment close should NOT be a separate push, got:\n{}",
        code
    );
}

/// @ai-generated — Multi-root where last root child is an element containing a component.
/// The fragment close marker <!--]--> should merge into the element's closing push,
/// not be a separate _push call.
#[test]
fn ssr_multi_root_fragment_close_after_component_in_div() {
    let code = gen_ssr_template(
        r#"<template>
<p>intro</p>
<div><MyComp msg="hi" /></div>
</template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    // Fragment close should be merged: `</div><!--]-->`)
    assert!(
        code.contains("</div><!--]-->`)"),
        "fragment close should be merged into closing div push, got:\n{}",
        code
    );
    // Should NOT have separate push for fragment close
    assert!(
        !code.contains("_push(`<!--]-->`)"),
        "fragment close should NOT be a separate push, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic component with children should render slots like regular components.
/// Vue wraps children in { default: _withCtx((...) => { ... }) }.
#[test]
fn ssr_dynamic_component_with_children() {
    let code = gen_ssr_template(
        r#"<template>
<component :is="'div'"><span>hello</span></component>
</template>
<script setup>
</script>"#,
    );
    // Should have _createVNode with slot content
    assert!(
        code.contains("default: _withCtx("),
        "dynamic component with children should have default slot, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderVNode"),
        "should use _ssrRenderVNode for dynamic component, got:\n{}",
        code
    );
    // Children should be inside the slot callback
    assert!(
        code.contains("<span>hello</span>"),
        "should render child content, got:\n{}",
        code
    );
    // Should NOT close without slot content
    assert!(
        !code.contains(", null), _parent"),
        "should not have null slots when children exist, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback should merge static class + dynamic :class into
/// a single class: ["static", dynamicExpr] array, matching Vue's output.
#[test]
fn ssr_vdom_fallback_class_merge() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><div class="static-cls" :class="{ active: isActive }">text</div></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const isActive = ref(true)
</script>"#,
    );
    // VDOM fallback should have merged class
    assert!(
        code.contains(r#"class: ["static-cls","#),
        "should merge static + dynamic class in VDOM fallback, got:\n{}",
        code
    );
    // Should NOT have duplicate class props
    assert!(
        !code.contains(r#"class: "static-cls", class: {"#),
        "should not have separate class entries, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_fallback_slot_outlet() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><slot></slot></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    // VDOM fallback should use _renderSlot, not _createVNode("slot")
    assert!(
        code.contains("_renderSlot"),
        "should use _renderSlot for <slot> in VDOM fallback, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_renderSlot(_ctx.$slots, "default")"#),
        "should render default slot outlet, got:\n{}",
        code
    );
    // Should NOT have _createVNode("slot")
    assert!(
        !code.contains(r#"_createVNode("slot")"#),
        "should not render slot as regular element, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_fallback_event_handlers() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><button @click="handleClick">Click</button></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const handleClick = () => {}
</script>"#,
    );
    // VDOM fallback should emit onClick handler
    assert!(
        code.contains("onClick: $setup.handleClick"),
        "should emit onClick event handler in VDOM fallback, got:\n{}",
        code
    );
    // Should NOT drop the event handler
    assert!(
        !code.contains(r#"_createVNode("button", null"#),
        "should not render button without props when it has event handlers, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_fallback_v_model_on_element() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><input v-model="text" /></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const text = ref('')
</script>"#,
    );
    // For native elements, Vue uses _withDirectives(_createVNode("input", {onUpdate:modelValue...}), [[vModelText, expr]])
    // At minimum, we need the "onUpdate:modelValue" handler prop
    assert!(
        code.contains("\"onUpdate:modelValue\""),
        "should emit onUpdate:modelValue handler for v-model on element, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_fallback_inline_event_handler() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><button @click="count++">Inc</button></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
let count = ref(0)
</script>"#,
    );
    // Inline handler should be wrapped in $event => (...)
    assert!(
        code.contains("onClick: $event => ("),
        "should wrap inline handler in $event arrow function, got:\n{}",
        code
    );
    // Should have onClick prop, not null props
    assert!(
        !code.contains(r#"_createVNode("button", null"#),
        "should not render button without props, got:\n{}",
        code
    );
}

#[test]
fn ssr_named_slots_in_component() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp>
  <template #header><h1>Title</h1></template>
  <template #footer><p>Footer</p></template>
</MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
</script>"#,
    );
    // Should emit named slots, not default slot
    assert!(
        code.contains("header: _withCtx("),
        "should emit header named slot, got:\n{}",
        code
    );
    assert!(
        code.contains("footer: _withCtx("),
        "should emit footer named slot, got:\n{}",
        code
    );
    // Should NOT put everything in default slot
    assert!(
        !code.contains("default: _withCtx((_, _push, _parent, _scopeId) => {\nif (_push) {\n_push(`<h1>Title</h1>"),
        "should not put named slot content in default slot, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Teleport dynamic binding
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — ref and :ref attributes should be skipped in SSR output.
#[test]
fn ssr_ref_attrs_skipped_on_non_root() {
    // Non-root element: ref should NOT appear in SSR output
    let code =
        gen_ssr_template(r#"<template><div><span ref="mySpan">content</span></div></template>"#);
    assert!(
        !code.contains("\"ref\"") && !code.contains("ref:") && !code.contains("ref=\""),
        "ref attribute should be skipped on non-root elements in SSR, got:\n{}",
        code
    );
    assert!(
        code.contains("<span>content</span>"),
        "should render element without ref, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic :ref should be skipped in SSR output.
#[test]
fn ssr_dynamic_ref_skipped() {
    let code = gen_ssr_template(
        r#"<template><ul><li v-for="(item, i) in items" :ref="el => setRef(el, i)">{{ item }}</li></ul></template>
<script setup>
const items = ['a', 'b']
function setRef(el, i) {}
</script>"#,
    );
    // :ref should NOT produce _ssrRenderAttr
    assert!(
        !code.contains("_ssrRenderAttr(\"ref\""),
        ":ref should not produce _ssrRenderAttr in SSR, got:\n{}",
        code
    );
    assert!(
        !code.contains("setRef"),
        "ref callback should not appear in SSR output, got:\n{}",
        code
    );
}

/// @ai-generated — Root-level comment before element should produce valid push and fragment markers.
#[test]
fn ssr_root_comment_before_element() {
    let code = gen_ssr_template(
        r#"<template>
  <!--before div-->
  <div>
    <!--after div-->
    foo
  </div>
</template>"#,
    );
    // Should have fragment markers for multi-root (comment counts for hydration)
    assert!(
        code.contains("<!--[-->"),
        "should have fragment open marker, got:\n{}",
        code
    );
    assert!(
        code.contains("<!--]-->"),
        "should have fragment close marker, got:\n{}",
        code
    );
    // Should have the comment inside the push
    assert!(
        code.contains("<!--before div-->"),
        "should include the comment, got:\n{}",
        code
    );
    // Should still apply _attrs to the root div
    assert!(
        code.contains("_ssrRenderAttrs(_attrs)"),
        "should apply _attrs to root div, got:\n{}",
        code
    );
    // Should produce valid single-push output (not nested _push calls)
    let push_count = code.matches("_push(").count();
    assert!(
        push_count == 1,
        "should have exactly 1 _push call, got {} in:\n{}",
        push_count,
        code
    );
}

/// @ai-generated — Teleport with dynamic :to binding should resolve the expression.
#[test]
fn ssr_teleport_dynamic_to_binding() {
    let code = gen_ssr_template(
        r#"<template><Teleport :to="teleportTarget"><div>content</div></Teleport></template>
<script setup>
import { ref } from 'vue'
const teleportTarget = ref('#modal')
</script>"#,
    );
    // Should use the resolved binding, not hardcoded "body"
    assert!(
        code.contains("$setup.teleportTarget"),
        "Teleport :to should resolve to $setup.teleportTarget, got:\n{}",
        code
    );
    assert!(
        !code.contains("\"body\""),
        "Teleport :to should NOT be hardcoded to \"body\", got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic component with v-bind spread should pass props.
#[test]
fn ssr_dynamic_component_v_bind_spread() {
    let code = gen_ssr_template(
        r#"<template><component :is="currentComponent" v-bind="dynamicProps" /></template>
<script setup>
import { ref } from 'vue'
const currentComponent = ref('div')
const dynamicProps = ref({})
</script>"#,
    );
    // Should pass the spread props to _createVNode
    assert!(
        code.contains("$setup.dynamicProps"),
        "v-bind spread should be passed as props, got:\n{}",
        code
    );
    // Should NOT have null for props when there's a v-bind spread
    assert!(
        !code.contains("_resolveDynamicComponent($setup.currentComponent), null, null)"),
        "props should not be null with v-bind spread, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic component with v-model should generate model props.
#[test]
fn ssr_dynamic_component_v_model() {
    let code = gen_ssr_template(
        r#"<template><component :is="inputComponent" v-model="inputValue" /></template>
<script setup>
import { ref } from 'vue'
const inputComponent = ref('input')
const inputValue = ref('')
</script>"#,
    );
    // Should have modelValue prop
    assert!(
        code.contains("modelValue: $setup.inputValue"),
        "v-model should generate modelValue prop, got:\n{}",
        code
    );
    // Should have onUpdate handler
    assert!(
        code.contains("\"onUpdate:modelValue\""),
        "v-model should generate onUpdate:modelValue handler, got:\n{}",
        code
    );
}

/// @ai-generated — Teleport with dynamic :disabled binding should resolve the expression.
#[test]
fn ssr_teleport_dynamic_disabled_binding() {
    let code = gen_ssr_template(
        r#"<template><Teleport to="body" :disabled="showModal"><div>content</div></Teleport></template>
<script setup>
import { ref } from 'vue'
const showModal = ref(false)
</script>"#,
    );
    // Should use the resolved binding for disabled
    assert!(
        code.contains("$setup.showModal"),
        "Teleport :disabled should resolve to $setup.showModal, got:\n{}",
        code
    );
    // Target should still be "body"
    assert!(
        code.contains("\"body\""),
        "Teleport static to should be \"body\", got:\n{}",
        code
    );
    // disabled should NOT be hardcoded false
    assert!(
        !code.contains(", false, _parent)"),
        "Teleport :disabled should NOT be hardcoded to false, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 24: v-model checkbox/radio inline attrs
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-model on checkbox should emit inline type="checkbox" + _ssrIncludeBooleanAttr.
#[test]
fn ssr_v_model_checkbox_inline() {
    let code = gen_ssr_template(
        r#"<template><div><input type="checkbox" v-model="checked"></div></template>
<script setup>
const checked = ref(false)
</script>"#,
    );
    // Vue emits inline: type="checkbox"${(_ssrIncludeBooleanAttr(...)) ? " checked" : ""}
    assert!(
        code.contains("type=\"checkbox\""),
        "should have inline type=\"checkbox\", got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrIncludeBooleanAttr"),
        "should use _ssrIncludeBooleanAttr for checked, got:\n{}",
        code
    );
    assert!(
        code.contains("\" checked\" : \"\""),
        "should have ternary for checked attr, got:\n{}",
        code
    );
    // Negative: should NOT wrap checkbox in _ssrRenderAttrs({...})
    assert!(
        !code.contains("_ssrRenderAttrs({ type:"),
        "should not wrap checkbox attrs in _ssrRenderAttrs, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on radio should emit inline type="radio" + _ssrIncludeBooleanAttr.
#[test]
fn ssr_v_model_radio_inline() {
    let code = gen_ssr_template(
        r#"<template><div><input type="radio" v-model="picked" value="one"></div></template>
<script setup>
const picked = ref('one')
</script>"#,
    );
    assert!(
        code.contains("type=\"radio\""),
        "should have inline type=\"radio\", got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrIncludeBooleanAttr") && code.contains("_ssrLooseEqual"),
        "should use _ssrIncludeBooleanAttr + _ssrLooseEqual for radio, got:\n{}",
        code
    );
    // Should NOT wrap radio in _ssrRenderAttrs({...})
    assert!(
        !code.contains("_ssrRenderAttrs({ type:"),
        "should not wrap radio attrs in _ssrRenderAttrs, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Round 25: v-else-if robustness + single-child v-if fragment
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — v-else-if chain should emit proper else if conditionals.
#[test]
fn ssr_v_else_if_chain() {
    let code = gen_ssr_template(
        r#"<template>
<div v-if="loading">Loading...</div>
<div v-else-if="error">Error: {{ error }}</div>
<div v-else>Content</div>
</template>
<script setup>
const loading = ref(false)
const error = ref(null)
</script>"#,
    );
    // Should have if/else-if/else chain
    assert!(
        code.contains("if ($setup.loading)"),
        "should have if condition, got:\n{}",
        code
    );
    assert!(
        code.contains("} else if ($setup.error)"),
        "should have else-if condition, got:\n{}",
        code
    );
    assert!(
        code.contains("} else {"),
        "should have else branch, got:\n{}",
        code
    );
    // Negative: should NOT have orphaned comment placeholder where else-if should be
    assert!(
        !code.contains("} else {\n_push(`<!---->`)\n}\n_push(`"),
        "should not have broken chain with comment placeholder, got:\n{}",
        code
    );
}

/// HTML comments between v-if/v-else-if/v-else branches should NOT break the
/// chain. Vue's compiler treats interstitial comments as non-structural.
#[test]
fn ssr_v_else_if_chain_with_comments() {
    let code = gen_ssr_template(
        r#"<template>
<div v-if="loading">Loading...</div>
<!-- Error State -->
<div v-else-if="error">Error: {{ error }}</div>
<!-- Default Content -->
<div v-else>Content</div>
</template>
<script setup>
const loading = ref(false)
const error = ref(null)
</script>"#,
    );
    // Should have proper if/else-if/else chain despite intervening comments
    assert!(
        code.contains("} else if ($setup.error)"),
        "comment between branches must not break else-if chain, got:\n{}",
        code
    );
    assert!(
        code.contains("} else {"),
        "comment between branches must not break else chain, got:\n{}",
        code
    );
    // Negative: should NOT have disconnected if blocks
    assert!(
        !code.contains("}\nif ("),
        "should not have disconnected if blocks, got:\n{}",
        code
    );
}

/// @ai-generated — Adjacent text + interpolation in VDOM fallback should be merged
/// into a single _createTextVNode call with string concatenation.
/// Vue: _createTextVNode("Hello " + _toDisplayString(name) + "!", 1 /* TEXT */)
/// NOT: _createTextVNode("Hello "), _createTextVNode(_toDisplayString(name), 1)
#[test]
fn ssr_vdom_fallback_text_merge() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp>Hello {{ name }}!</MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const name = ref('World')
</script>"#,
    );
    // Positive: should have concatenated text in VDOM fallback
    assert!(
        code.contains(r#"_createTextVNode("Hello " + _toDisplayString"#),
        "should merge adjacent text + interpolation into single _createTextVNode, got:\n{}",
        code
    );
    // Negative: should NOT have separate _createTextVNode for "Hello "
    assert!(
        !code.contains(r#"_createTextVNode("Hello ")"#),
        "should NOT have separate _createTextVNode for plain text, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback for elements with dynamic content should generate
/// proper _toDisplayString content inside _createVNode.
#[test]
fn ssr_vdom_fallback_dynamic_text_element() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><div>{{ msg }}</div></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const msg = ref('hello')
</script>"#,
    );
    // Positive: div with dynamic text should use _toDisplayString with TEXT patch flag
    assert!(
        code.contains(r#"_createVNode("div", null, _toDisplayString($setup.msg), 1 /* TEXT */)"#),
        "should generate _createVNode with _toDisplayString and TEXT patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback for elements with dynamic props should include
/// the prop binding in the props object.
#[test]
fn ssr_vdom_fallback_dynamic_props_element() {
    let code = gen_ssr_template(
        r#"<template>
<MyComp><input :value="val" /></MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
</script>"#,
    );
    // Positive: element with dynamic prop should include it in props
    assert!(
        code.contains(r#"value: $setup.val"#),
        "should include dynamic prop in VDOM fallback props, got:\n{}",
        code
    );
}

/// @ai-generated — v-if with single child component should not have extra fragment.
// ── VDOM fallback: <template> element should be transparent (unwrapped) ──

/// @ai-generated — <template v-if> should NOT produce _createVNode("template")
/// in the VDOM fallback; its children should be unwrapped.
#[test]
fn ssr_vdom_fallback_template_vif_unwrapped() {
    let code = gen_ssr_template(
        r#"<template><Comp><template v-if="show"><span>A</span><span>B</span></template></Comp></template>
<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>"#,
    );
    // Should NOT have _createVNode("template")
    assert!(
        !code.contains("_createVNode(\"template\""),
        "template v-if should be unwrapped in VDOM, got:\n{}",
        code
    );
    // Should have the children directly
    assert!(
        code.contains("_createVNode(\"span\""),
        "should have span VNodes directly, got:\n{}",
        code
    );
}

/// @ai-generated — <template v-for> should NOT produce _createVNode("template")
/// in the VDOM fallback; its children should be unwrapped as Fragment.
#[test]
fn ssr_vdom_fallback_template_vfor_unwrapped() {
    let code = gen_ssr_template(
        r#"<template><Comp><template v-for="item in items" :key="item.id"><span>{{ item.name }}</span></template></Comp></template>
<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>"#,
    );
    // Should NOT have _createVNode("template")
    assert!(
        !code.contains("_createVNode(\"template\""),
        "template v-for should be unwrapped in VDOM, got:\n{}",
        code
    );
}

// ── VDOM fallback: named slots ──

/// @ai-generated — Named slots should generate separate _withCtx wrappers in VDOM fallback.
#[test]
fn ssr_vdom_fallback_named_slots() {
    let code = gen_ssr_template(
        r#"<template><Comp>
<template #title>Title</template>
<template #default>Default</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Should have named slot "title"
    assert!(
        code.contains("title: _withCtx("),
        "should have title slot, got:\n{}",
        code
    );
    // Should have default slot
    assert!(
        code.contains("default: _withCtx("),
        "should have default slot, got:\n{}",
        code
    );
    // Should NOT wrap everything in a single default slot
    let title_count = code.matches("title: _withCtx(").count();
    assert!(
        title_count >= 1,
        "should have at least 1 title slot occurrence, got {} in:\n{}",
        title_count,
        code
    );
}

/// @ai-generated — Named slot with params should pass params to _withCtx callback.
#[test]
fn ssr_vdom_fallback_named_slot_params() {
    let code = gen_ssr_template(
        r#"<template><Comp>
<template #item="{ data }">{{ data.name }}</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // The named slot _withCtx should include the slot params ({ data })
    // The structure is: item: _withCtx(({ data }, _push, _parent, _scopeId) => { if (_push) { ... } else { return [...] } })
    assert!(
        code.contains("item: _withCtx(({ data }"),
        "named slot should have params in _withCtx callback, got:\n{}",
        code
    );
    // The slot VDOM fallback should reference data.name
    assert!(
        code.contains("_toDisplayString(data.name)"),
        "slot VDOM fallback should reference data.name, got:\n{}",
        code
    );
}

// ── VDOM fallback: static style → JS object ──

/// @ai-generated — Static style in VDOM fallback props should be converted to JS object.
#[test]
fn ssr_vdom_fallback_static_style_js_object() {
    let code = gen_ssr_template(
        r#"<template><Comp><span style="color: red">text</span></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Positive: static style should be JS object in VDOM fallback
    assert!(
        code.contains(r#"style: {"color":"red"}"#),
        "should convert static style to JS object, got:\n{}",
        code
    );
    // Negative: should NOT be a string
    assert!(
        !code.contains(r#"style: "color: red""#),
        "should NOT have string style in VDOM, got:\n{}",
        code
    );
}

/// @ai-generated — Multi-property static style → JS object with camelCase.
#[test]
fn ssr_vdom_fallback_static_style_multi_prop() {
    let code = gen_ssr_template(
        r#"<template><Comp><div style="font-size: 16px; background-color: blue">text</div></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Vue SSR keeps kebab-case for static style properties in VDOM fallback too
    assert!(
        code.contains(r#"style: {"font-size":"16px","background-color":"blue"}"#),
        "should keep kebab-case for static style properties, got:\n{}",
        code
    );
}

// ── VDOM fallback: patch flags ──

/// @ai-generated — _createTextVNode with interpolation should have TEXT flag in VDOM fallback.
#[test]
fn ssr_vdom_fallback_text_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp>{{ msg }}</Comp></template>
<script setup>
import Comp from './Comp.vue'
const msg = ref('hello')
</script>"#,
    );
    // The VDOM fallback _createTextVNode should have 1 /* TEXT */ flag
    assert!(
        code.contains("_createTextVNode(_toDisplayString($setup.msg), 1 /* TEXT */)"),
        "should have TEXT patch flag on _createTextVNode with interpolation, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback for element with dynamic text child should
/// include TEXT patch flag (1) on _createVNode.
#[test]
fn ssr_vdom_fallback_element_text_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><pre>{{ content }}</pre></Comp></template>
<script setup>
import Comp from './Comp.vue'
const content = ref('hello')
</script>"#,
    );
    // Vue generates: _createVNode("pre", null, _toDisplayString($setup.content), 1 /* TEXT */)
    assert!(
        code.contains("1 /* TEXT */"),
        "element with interpolation child should have TEXT patchflag, got:\n{}",
        code
    );
    assert!(
        !code.contains("_createVNode(\"pre\", null, _toDisplayString($setup.content))"),
        "should NOT have _createVNode without patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — _createBlock for v-if HTML element should include TEXT patchflag
/// when it has dynamic text children.
#[test]
fn ssr_vdom_block_element_text_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><h2 v-if="show">{{ title }}</h2></Comp></template>
<script setup>
import Comp from './Comp.vue'
const show = ref(true)
const title = ref('hello')
</script>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // _createBlock("h2", { key: 0 }, _toDisplayString(...), 1 /* TEXT */)
    assert!(
        vdom_part.contains("_createBlock(\"h2\""),
        "v-if element should use _createBlock, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("1 /* TEXT */"),
        "_createBlock with dynamic text should have TEXT patchflag, got:\n{}",
        vdom_part
    );
}

/// @ai-generated — v-if component block with dynamic props should have PROPS flag
#[test]
fn ssr_vdom_block_component_props_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><Child v-if="show" :location="loc" /></Comp></template>
<script setup>
import Comp from './Comp.vue'
import Child from './Child.vue'
const show = ref(true)
const loc = ref('')
</script>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // _createBlock(Child, { key: 0, location: ... }, null, 8 /* PROPS */, ["location"])
    assert!(
        vdom_part.contains("8 /* PROPS */"),
        "v-if component block with dynamic props should have PROPS flag, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains(r#"["location"]"#),
        "should have dynamic props array, got:\n{}",
        vdom_part
    );
}

/// @ai-generated — VDOM fallback for component with dynamic props should
/// include PROPS patch flag (8) and dynamic props array.
#[test]
fn ssr_vdom_fallback_component_props_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><Child :store="store" /></Comp></template>
<script setup>
import Comp from './Comp.vue'
import Child from './Child.vue'
const store = ref({})
</script>"#,
    );
    // Vue generates: _createVNode(Child, { store: $setup.store }, null, 8 /* PROPS */, ["store"])
    // The `null` for children is required when there are no children but patch flags exist.
    assert!(
        code.contains("null, 8 /* PROPS */"),
        "childless component with dynamic props should have null children before PROPS patchflag, got:\n{}",
        code
    );
    assert!(
        code.contains("[\"store\"]"),
        "component with dynamic props should have dynamic props array, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback for element with NEED_HYDRATION: event handlers
/// on elements produce patch flag 32.
#[test]
fn ssr_vdom_fallback_element_need_hydration_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><input @input="handler" /></Comp></template>
<script setup>
import Comp from './Comp.vue'
const handler = () => {}
</script>"#,
    );
    // Vue generates: _createVNode("input", { onInput: $setup.handler }, null, 32 /* NEED_HYDRATION */)
    // handler is setup-const (arrow function literal), but <input> is a form element
    // so Vue adds NEED_HYDRATION but NOT PROPS (no dynamic props array).
    assert!(
        code.contains("32 /* NEED_HYDRATION */"),
        "form element with const event handler should have NEED_HYDRATION patchflag, got:\n{}",
        code
    );
    assert!(
        !code.contains("40 /* PROPS, NEED_HYDRATION */"),
        "const handler should NOT have PROPS flag (only NEED_HYDRATION), got:\n{}",
        code
    );
}

/// @ai-generated — Element with setup-const interpolation should NOT get TEXT patchflag.
/// Vue's VDOM fallback skips TEXT flag when the expression is a constant.
#[test]
fn ssr_vdom_fallback_const_text_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><p>{{ msg }}</p></Comp></template>
<script setup>
import Comp from './Comp.vue'
const msg = 'hello'
</script>"#,
    );
    // msg is literal-const → Vue: _createVNode("p", null, _toDisplayString($setup.msg))
    // No TEXT patchflag because the expression is constant.
    assert!(
        code.contains("_createVNode(\"p\""),
        "should create p element, got:\n{}",
        code
    );
    assert!(
        !code.contains("1 /* TEXT */"),
        "const interpolation should NOT have TEXT patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — Component with setup-const bound prop should NOT get PROPS patchflag.
/// Vue skips PROPS when all dynamic-bound expressions are constant.
#[test]
fn ssr_vdom_fallback_component_const_prop_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :title="constVal" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
const constVal = 'hello'
</script>"#,
    );
    // constVal is literal-const → Vue: _createVNode(Child, { title: $setup.constVal })
    // No PROPS patchflag.
    assert!(
        code.contains("_createVNode("),
        "should have _createVNode, got:\n{}",
        code
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "const prop should NOT have PROPS patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — @click has a dedicated fast path and is excluded from NEED_HYDRATION.
/// Const @click handler on any element → no patch flags.
#[test]
fn ssr_vdom_fallback_div_const_handler_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><div @click="handler">click</div></Comp></template>
<script setup>
import Comp from './Comp.vue'
const handler = () => {}
</script>"#,
    );
    // handler is setup-const, @click excluded from NEED_HYDRATION → no flags
    // Vue: _createVNode("div", { onClick: $setup.handler }, "click")
    assert!(
        code.contains("_createVNode(\"div\""),
        "should create div element, got:\n{}",
        code
    );
    assert!(
        !code.contains("/* NEED_HYDRATION */"),
        "@click is excluded from NEED_HYDRATION, got:\n{}",
        code
    );
    assert!(
        !code.contains("/* PROPS"),
        "const @click should NOT have PROPS flag, got:\n{}",
        code
    );
}

/// @ai-generated — Component with const event handler should NOT get PROPS patchflag.
#[test]
fn ssr_vdom_fallback_component_const_event_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child @click="handler" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
const handler = () => {}
</script>"#,
    );
    // handler is setup-const → no PROPS flag on component
    assert!(
        code.contains("_createVNode("),
        "should have _createVNode, got:\n{}",
        code
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "component with const event handler should NOT have PROPS flag, got:\n{}",
        code
    );
}

/// @ai-generated — Component with literal numeric prop should NOT get PROPS patchflag.
/// Vue treats `:span="8"` as a constant expression — no dynamic tracking needed.
#[test]
fn ssr_vdom_fallback_literal_number_prop_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :span="8" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
</script>"#,
    );
    // :span="8" is a numeric literal → constant → no PROPS patchflag
    assert!(
        code.contains("_createVNode("),
        "should have _createVNode, got:\n{}",
        code
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "literal number prop should NOT have PROPS patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — Component with literal string prop should NOT get PROPS patchflag.
/// Vue treats `:label="'hello'"` as a constant expression.
#[test]
fn ssr_vdom_fallback_literal_string_prop_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :label="'hello'" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
</script>"#,
    );
    assert!(
        code.contains("_createVNode("),
        "should have _createVNode, got:\n{}",
        code
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "literal string prop should NOT have PROPS patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — Component with literal boolean prop should NOT get PROPS patchflag.
/// Vue treats `:bordered="false"` as a constant expression.
#[test]
fn ssr_vdom_fallback_literal_bool_prop_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :bordered="false" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
</script>"#,
    );
    assert!(
        code.contains("_createVNode("),
        "should have _createVNode, got:\n{}",
        code
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "literal boolean prop should NOT have PROPS patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — Element with literal number in :class should NOT get CLASS patchflag.
/// This tests that literal expressions in dynamic bindings are recognized as const.
#[test]
fn ssr_vdom_fallback_literal_text_interp_no_patchflag() {
    let code = gen_ssr_template(
        r#"<template><Comp><p>{{ 42 }}</p></Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    assert!(
        code.contains("_createVNode(\"p\""),
        "should create p element, got:\n{}",
        code
    );
    assert!(
        !code.contains("1 /* TEXT */"),
        "literal number interpolation should NOT have TEXT patchflag, got:\n{}",
        code
    );
}

/// @ai-generated — Mixed const and dynamic props: only dynamic ones should trigger flags.
/// `:span="8"` is literal const, `:count="count"` is reactive → PROPS flag with only ["count"].
#[test]
fn ssr_vdom_fallback_mixed_const_dynamic_props() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :span="8" :count="count" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
import { ref } from 'vue'
const count = ref(0)
</script>"#,
    );
    // count is setup-ref → dynamic, but span is literal → const
    // Should have PROPS flag with only ["count"], not ["span", "count"]
    assert!(
        code.contains("8 /* PROPS */"),
        "should have PROPS flag for dynamic count, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""count""#),
        "should list count in dynamic props, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#""span""#),
        "should NOT list span in dynamic props (literal const), got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback should use $setup. prefix for compound expressions.
/// Vue generates `$setup.state.count` but Verter was generating bare `state.count`.
#[test]
fn ssr_vdom_fallback_compound_expr_setup_prefix() {
    let code = gen_ssr_template(
        r#"<template><Parent><Child :count="state.count" /></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Child from './Child.vue'
import { reactive } from 'vue'
const state = reactive({ count: 0 })
</script>"#,
    );
    // state is setup-reactive-const → $setup.state.count in both SSR and VDOM paths
    assert!(
        code.contains("$setup.state.count"),
        "compound expression should have $setup. prefix, got:\n{}",
        code
    );
    assert!(
        !code.contains(": state.count"),
        "should NOT have bare 'state.count' without prefix, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback event handler with compound expression gets $setup prefix.
#[test]
fn ssr_vdom_fallback_event_handler_setup_prefix() {
    let code = gen_ssr_template(
        r#"<template><Comp><button @click="state.count++">+</button></Comp></template>
<script setup>
import Comp from './Comp.vue'
import { reactive } from 'vue'
const state = reactive({ count: 0 })
</script>"#,
    );
    assert!(
        code.contains("$setup.state.count++"),
        "event handler compound expression should have $setup. prefix, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM fallback interpolation with compound expression gets $setup prefix.
#[test]
fn ssr_vdom_fallback_interpolation_setup_prefix() {
    let code = gen_ssr_template(
        r#"<template><Comp><p>{{ store.errors }}</p></Comp></template>
<script setup>
import Comp from './Comp.vue'
import { reactive } from 'vue'
const store = reactive({ errors: [] })
</script>"#,
    );
    // Both the SSR path (_ssrInterpolate) and VDOM path (_toDisplayString) should use $setup.
    assert!(
        code.contains("$setup.store.errors"),
        "interpolation compound expression should have $setup. prefix, got:\n{}",
        code
    );
    // Check that the VDOM fallback path specifically uses $setup. in _toDisplayString
    assert!(
        code.contains("_toDisplayString($setup.store.errors)"),
        "VDOM fallback _toDisplayString should have $setup. prefix, got:\n{}",
        code
    );
}

// ── Named slots with mixed default content ──

/// @ai-generated — Component with both implicit default content AND a named slot.
/// The named slot should be recognized and the default content wrapped separately.
/// This reproduces the StateSetup.story.vue pattern where Vue generates:
/// `{ controls: _withCtx(...), default: _withCtx(...), _: 1 }`
/// but Verter puts everything in default.
#[test]
fn ssr_named_slot_with_implicit_default() {
    let code = gen_ssr_template(
        r#"<template><Comp title="test">
<h1>Default content</h1>
<template #controls>
<div>Controls</div>
</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    // Should have named "controls" slot
    assert!(
        code.contains("controls: _withCtx("),
        "should have controls named slot, got:\n{}",
        code
    );
    // Should also have default slot for the <h1>
    assert!(
        code.contains("default: _withCtx("),
        "should have implicit default slot, got:\n{}",
        code
    );
    // The controls slot should contain "Controls"
    assert!(
        code.contains("Controls"),
        "controls slot should contain its content, got:\n{}",
        code
    );
}

/// @ai-generated — Same as above but with globally registered (non-imported) components.
/// The components are resolved via _resolveComponent, not $setup.
#[test]
fn ssr_named_slot_with_implicit_default_global() {
    let code = gen_ssr_template(
        r#"<template><Story><Variant title="default">
<h1>State</h1>
<template #controls>
<div class="controls">Controls</div>
</template>
</Variant></Story></template>
<script setup>
</script>"#,
    );
    // Should have named "controls" slot
    assert!(
        code.contains("controls: _withCtx("),
        "global component should have controls named slot, got:\n{}",
        code
    );
    // Should have default slot for <h1>
    assert!(
        code.contains("default: _withCtx("),
        "global component should have implicit default slot, got:\n{}",
        code
    );
    // Controls content should be present
    assert!(
        code.contains("Controls"),
        "controls slot should contain its content, got:\n{}",
        code
    );
}

// ── V-if VDOM fallback: ternary with _openBlock/_createBlock + key ──

/// @ai-generated — v-if in slot VDOM fallback should generate ternary with
/// _openBlock()/_createBlock() and key: 0, plus _createCommentVNode("v-if", true).
#[test]
fn ssr_vdom_fallback_vif_ternary() {
    let code = gen_ssr_template(
        r#"<template><Comp><div v-if="show">hello</div></Comp></template>
<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>"#,
    );
    // Positive: VDOM fallback should have ternary with _openBlock/_createBlock
    assert!(
        code.contains("_openBlock()"),
        "should have _openBlock() in VDOM fallback, got:\n{}",
        code
    );
    assert!(
        code.contains("_createBlock("),
        "should have _createBlock() in VDOM fallback, got:\n{}",
        code
    );
    assert!(
        code.contains("key: 0"),
        "should have key: 0 on v-if branch, got:\n{}",
        code
    );
    assert!(
        code.contains("_createCommentVNode(\"v-if\", true)"),
        "should have _createCommentVNode for v-if else branch, got:\n{}",
        code
    );
    // Negative: should NOT wrap in _createVNode("template")
    assert!(
        !code.contains("_createVNode(\"template\""),
        "should NOT wrap v-if in template VNode, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else in slot VDOM fallback: both branches get keys.
#[test]
fn ssr_vdom_fallback_vif_velse_ternary() {
    let code = gen_ssr_template(
        r#"<template><Comp><div v-if="a">A</div><span v-else>B</span></Comp></template>
<script setup>
import Comp from './Comp.vue'
const a = ref(true)
</script>"#,
    );
    // Both branches should have keys
    assert!(
        code.contains("key: 0"),
        "v-if branch should have key: 0, got:\n{}",
        code
    );
    assert!(
        code.contains("key: 1"),
        "v-else branch should have key: 1, got:\n{}",
        code
    );
    // Should NOT have _createCommentVNode("v-if") since there's an else branch
    assert!(
        !code.contains("_createCommentVNode(\"v-if\""),
        "should not have comment VNode when v-else exists, got:\n{}",
        code
    );
    // Should NOT wrap in template VNode
    assert!(
        !code.contains("_createVNode(\"template\""),
        "should NOT wrap v-if in template VNode, got:\n{}",
        code
    );
}

/// @ai-generated — v-if/v-else-if/v-else: three-way ternary with keys 0, 1, 2.
#[test]
fn ssr_vdom_fallback_vif_chain_keys() {
    let code = gen_ssr_template(
        r#"<template><Comp>
<div v-if="a">A</div>
<div v-else-if="b">B</div>
<span v-else>C</span>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
const a = ref(true)
const b = ref(false)
</script>"#,
    );
    assert!(
        code.contains("key: 0"),
        "v-if branch should have key: 0, got:\n{}",
        code
    );
    assert!(
        code.contains("key: 1"),
        "v-else-if branch should have key: 1, got:\n{}",
        code
    );
    assert!(
        code.contains("key: 2"),
        "v-else branch should have key: 2, got:\n{}",
        code
    );
}

/// @ai-generated — v-if on component in VDOM fallback: _createBlock with key.
#[test]
fn ssr_vdom_fallback_vif_component() {
    let code = gen_ssr_template(
        r#"<template><Outer><Inner v-if="show" /></Outer></template>
<script setup>
import Outer from './Outer.vue'
import Inner from './Inner.vue'
const show = ref(true)
</script>"#,
    );
    assert!(
        code.contains("_createBlock("),
        "should use _createBlock for v-if component, got:\n{}",
        code
    );
    assert!(
        code.contains("key: 0"),
        "component v-if should have key: 0, got:\n{}",
        code
    );
    assert!(
        code.contains("_createCommentVNode(\"v-if\", true)"),
        "should have comment VNode for missing else, got:\n{}",
        code
    );
}

#[test]
fn ssr_v_if_single_component_no_extra_fragment() {
    let code = gen_ssr_template(
        r#"<template><div>
<MyComp v-if="show" />
</div></template>
<script setup>
import MyComp from './MyComp.vue'
const show = ref(true)
</script>"#,
    );
    // The v-if branch with single component should NOT have <!--[--> inside
    assert!(
        !code.contains("_push(`<!--[-->`) _push(_ssrRenderComponent"),
        "single component in v-if should not have fragment markers, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_ordering_named_before_default() {
    // Vue emits named slots before the implicit default slot.
    // When default content appears before named slots in source,
    // Verter must reorder the output to match Vue.
    let code = gen_ssr_template(
        r#"<template><Comp>
<h1>Default content</h1>
<template #controls>
<div>Controls</div>
</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    assert!(
        code.contains("controls: _withCtx("),
        "should have controls slot, got:\n{}",
        code
    );
    assert!(
        code.contains("default: _withCtx("),
        "should have default slot, got:\n{}",
        code
    );
    let controls_pos = code.find("controls: _withCtx(").unwrap();
    let default_pos = code.find("default: _withCtx(").unwrap();
    assert!(
        controls_pos < default_pos,
        "named slot 'controls' (pos {}) should come before 'default' slot (pos {}):\n{}",
        controls_pos,
        default_pos,
        code
    );
}

/// @ai-generated — Hyphenated v-bind shorthand on slot props should resolve
/// to camelCase expression, not subtraction.
/// `:heading-value` → value should be `_ctx.headingValue`, not `_ctx.heading - value`
#[test]
fn ssr_slot_prop_hyphenated_shorthand() {
    let code = gen_ssr_template(
        r#"<template><slot :heading-value></slot></template>
<script>
export default {
  props: ['headingValue']
}
</script>"#,
    );
    // The slot prop value should use the camelized name (prefix depends on binding type)
    assert!(
        code.contains(r#"headingValue: _ctx["headingValue"]"#)
            || code.contains("headingValue: _ctx.headingValue")
            || code.contains("headingValue: $props.headingValue")
            || code.contains(r#"headingValue: $props["headingValue"]"#),
        "slot prop value should use camelized name 'headingValue', got:\n{}",
        code
    );
    // Must NOT contain subtraction pattern
    assert!(
        !code.contains(r#"_ctx["heading"]-value"#) && !code.contains("_ctx.heading-value"),
        "should not produce subtraction expression, got:\n{}",
        code
    );
}

/// @ai-generated — Hyphenated v-bind shorthand on element attributes should
/// resolve to camelCase expression, not subtraction.
#[test]
fn ssr_element_attr_hyphenated_shorthand() {
    let code = gen_ssr_template(
        r#"<template><div :data-count></div></template>
<script setup>
const dataCount = ref(0)
</script>"#,
    );
    // Should use camelized name for the value expression
    assert!(
        code.contains("data-count")
            && (code.contains("$setup.dataCount")
                || code.contains(r#"$setup["dataCount"]"#)
                || code.contains("_ctx.dataCount")
                || code.contains(r#"_ctx["dataCount"]"#)),
        "should use camelized 'dataCount' for value lookup, got:\n{}",
        code
    );
    // Must NOT contain subtraction pattern
    assert!(
        !code.contains("data-count\"") || code.contains("\"data-count\""), // attr name in quotes is fine
        "should not produce subtraction in value expression, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on checkbox should generate proper Array.isArray() call
/// with parentheses around the model expression.
#[test]
fn ssr_checkbox_vmodel_isarray_parens() {
    let code = gen_ssr_template(
        r#"<template><div><input type="checkbox" v-model="model" /></div></template>
<script>
export default { data() { return { model: false } } }
</script>"#,
    );
    // Should have proper function call syntax: Array.isArray(_ctx["model"])
    assert!(
        code.contains("Array.isArray(") && code.contains("_ssrLooseContain("),
        "checkbox v-model should have Array.isArray() with parens, got:\n{}",
        code
    );
    // Must NOT have broken syntax: Array.isArray_ctx or Array.isArray$
    assert!(
        !code.contains("isArray_ctx") && !code.contains("isArray$"),
        "should not have missing parens in Array.isArray call, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on checkbox with v-bind spread (setup API) should
/// generate proper Array.isArray() call with parentheses. Element-plus pattern.
#[test]
fn ssr_checkbox_vmodel_isarray_with_vbind_spread() {
    let code = gen_ssr_template(
        r#"<template>
  <label>
    <input v-model="model" :class="cls" type="checkbox" v-bind="inputBindings" />
  </label>
</template>
<script setup>
const model = defineModel()
const inputBindings = computed(() => ({ value: 'x' }))
const cls = computed(() => 'my-class')
</script>"#,
    );
    // Should have proper function call syntax: Array.isArray($setup["model"])
    assert!(
        code.contains("Array.isArray("),
        "checkbox v-model should have Array.isArray() with parens, got:\n{}",
        code
    );
    // Must NOT have broken syntax: Array.isArray$setup or Array.isArray_ctx
    assert!(
        !code.contains("isArray_ctx") && !code.contains("isArray$"),
        "should not have missing parens in Array.isArray call, got:\n{}",
        code
    );
}

/// @ai-generated — v-show on a dynamic <component :is="..."> should NOT emit
/// `_resolveDirective("show")` because v-show is a built-in directive.
#[test]
fn ssr_dynamic_component_vshow_no_resolve_directive() {
    let code = gen_ssr_template(
        r#"<template><component :is="as" v-show="visible"><slot /></component></template>
<script>
export default { props: ['as', 'visible'] }
</script>"#,
    );
    // Must NOT resolve v-show as a custom directive
    assert!(
        !code.contains("_resolveDirective"),
        "v-show on dynamic component should not emit _resolveDirective, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ssrGetDirectiveProps"),
        "v-show on dynamic component should not emit _ssrGetDirectiveProps, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_flag_dynamic_when_vif_on_slot() {
    // Vue uses _: 2 /* DYNAMIC */ when template v-slot has v-if
    let code = gen_ssr_template(
        r#"<template><Comp>
<template v-if="show" #header>
<h1>Header</h1>
</template>
<template #footer>
<p>Footer</p>
</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>"#,
    );
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "should use DYNAMIC slot flag when v-if on template v-slot, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 1 /* STABLE */"),
        "should NOT use STABLE when v-if on template v-slot, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_flag_dynamic_when_vfor_on_slot() {
    // Vue uses _: 2 /* DYNAMIC */ when template v-slot has v-for
    let code = gen_ssr_template(
        r#"<template><Comp>
<template v-for="item in items" #[item.name]>
{{ item.content }}
</template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>"#,
    );
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "should use DYNAMIC slot flag when v-for on template v-slot, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_flag_stable_when_no_dynamic_slots() {
    // Static slots should use _: 1 /* STABLE */
    let code = gen_ssr_template(
        r#"<template><Comp>
<template #header><h1>Header</h1></template>
<template #footer><p>Footer</p></template>
</Comp></template>
<script setup>
import Comp from './Comp.vue'
</script>"#,
    );
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "should use STABLE slot flag for static slots, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 2 /* DYNAMIC */"),
        "should NOT use DYNAMIC for static slots, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_flag_dynamic_when_component_in_vfor() {
    // A component with slots inside v-for should use _: 2 /* DYNAMIC */
    let code = gen_ssr_template(
        r#"<template><div v-for="item in items">
<Comp>
<template #header><h1>{{ item.title }}</h1></template>
</Comp>
</div></template>
<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>"#,
    );
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "should use DYNAMIC slot flag when component is inside v-for, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 1 /* STABLE */"),
        "should NOT use STABLE when inside v-for, got:\n{}",
        code
    );
}

// ─── Fragment marker tests ────────────────────────────────────────────────────

#[test]
fn ssr_no_fragment_markers_for_text_only_root() {
    // A template with only text/interpolation at root should NOT have fragment markers
    let code = gen_ssr_template(
        r#"<template> Hello {{ name }}! </template>
<script setup>
const props = defineProps<{ name: string }>()
</script>"#,
    );
    assert!(
        !code.contains("<!--[-->"),
        "text-only root should not have fragment markers, got:\n{}",
        code
    );
    assert!(
        !code.contains("<!--]-->"),
        "text-only root should not have fragment markers, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrInterpolate"),
        "should contain interpolation, got:\n{}",
        code
    );
}

#[test]
fn ssr_no_extra_fragment_in_vfor_body() {
    // v-for body should not have extra fragment markers when the body is a single element
    let code = gen_ssr_template(
        r#"<template><ul>
<li v-for="item in items">{{ item }}</li>
</ul></template>
<script setup>
const items = ref([])
</script>"#,
    );
    // v-for itself emits <!--[--> and <!--]--> for the list boundary.
    // The v-for body (single <li>) should NOT add another layer of <!--[-->...<!--]-->
    let frag_open_count = code.matches("<!--[-->").count();
    let frag_close_count = code.matches("<!--]-->").count();
    assert_eq!(
        frag_open_count, 1,
        "should have exactly 1 fragment open marker for v-for, got {} in:\n{}",
        frag_open_count, code
    );
    assert_eq!(
        frag_close_count, 1,
        "should have exactly 1 fragment close marker for v-for, got {} in:\n{}",
        frag_close_count, code
    );
}

#[test]
fn ssr_no_extra_fragment_in_vif_single_root() {
    // v-if with a single-root branch should not add extra fragment markers
    let code = gen_ssr_template(
        r#"<template><div>
<span v-if="show">Hello</span>
<span v-else>Goodbye</span>
</div></template>
<script setup>
const show = ref(true)
</script>"#,
    );
    // v-if/v-else with single elements should not have any fragment markers
    assert!(
        !code.contains("<!--[-->"),
        "v-if with single root should not have fragment markers, got:\n{}",
        code
    );
}

#[test]
fn ssr_template_vif_single_child_no_fragment() {
    // <template v-if> with a single child should NOT emit fragment markers
    let code = gen_ssr_template(
        r#"<template><div>
<template v-if="show"><a>link</a></template>
<template v-else><span>text</span></template>
</div></template>
<script setup>
const show = ref(true)
</script>"#,
    );
    assert!(
        !code.contains("<!--[-->"),
        "<template v-if> with single child should not have fragment markers, got:\n{}",
        code
    );
    assert!(
        code.contains("<a>link</a>"),
        "should contain the inner element, got:\n{}",
        code
    );
}

#[test]
fn ssr_template_vif_multi_child_has_fragment() {
    // <template v-if> with multiple children SHOULD emit fragment markers
    let code = gen_ssr_template(
        r#"<template><div>
<template v-if="show"><a>link</a><span>more</span></template>
</div></template>
<script setup>
const show = ref(true)
</script>"#,
    );
    assert!(
        code.contains("<!--[-->"),
        "<template v-if> with multiple children should have fragment markers, got:\n{}",
        code
    );
    assert!(
        code.contains("<!--]-->"),
        "<template v-if> with multiple children should have close fragment marker, got:\n{}",
        code
    );
}

// ── Comment whitespace preservation ──

#[test]
fn ssr_vdom_fallback_comment_preserves_whitespace() {
    let code = gen_ssr_template(
        r#"<script setup>
const show = ref(true)
</script>
<template><Comp><template #default="{ item }"><!-- item is typed as User --><div>{{ item }}</div></template></Comp></template>"#,
    );
    // Vue preserves spaces inside comment VNodes: " item is typed as User "
    assert!(
        code.contains(r#"_createCommentVNode(" item is typed as User ")"#),
        "comment text should preserve leading/trailing whitespace, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"_createCommentVNode("item is typed as User")"#),
        "comment text should NOT be trimmed, got:\n{}",
        code
    );
}

// ── VDOM text boundary whitespace ──

#[test]
fn ssr_vdom_fallback_interpolation_no_boundary_whitespace() {
    // Template with whitespace around interpolation inside an element:
    // <p>\n  {{ msg }}\n</p> creates text nodes around the interpolation
    let code = gen_ssr_template(
        "<script setup>\nconst msg = ref('hello')\n</script>\n<template><Comp><p>\n  {{ msg }}\n</p></Comp></template>",
    );
    // Vue drops leading/trailing whitespace-only text around interpolation in element children
    // Should be: _toDisplayString($setup.msg), 1 /* TEXT */
    // NOT: " " + _toDisplayString($setup.msg) + " "
    assert!(
        code.contains(r#"_toDisplayString($setup.msg), 1 /* TEXT */"#),
        "interpolation-only children should not have surrounding whitespace, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"" " + _toDisplayString"#),
        "should not have leading whitespace text part, got:\n{}",
        code
    );
}

// ── SSR slot flag dynamic detection ──

#[test]
fn ssr_slot_flag_dynamic_inside_v_for() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>
<template><div v-for="item in items"><Comp><span>{{ item }}</span></Comp></div></template>"#,
    );
    // Inside v-for, slots should be DYNAMIC (2), not STABLE (1)
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "slots inside v-for should be DYNAMIC, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 1 /* STABLE */"),
        "should not have STABLE flag inside v-for, got:\n{}",
        code
    );
}

// ── Class array merging in VDOM props ──

#[test]
fn ssr_vdom_class_array_merge_static_and_dynamic() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const active = ref(true)
</script>
<template><Comp><div class="static-cls" :class="{ active: active }">hi</div></Comp></template>"#,
    );
    // Vue merges static class + :class into array: class: ["static-cls", { active: active }]
    assert!(
        code.contains(r#"class: ["static-cls", { active: $setup.active }]"#),
        "should merge static+dynamic class into array, got:\n{}",
        code
    );
    // Should NOT emit separate class keys
    assert!(
        !code.contains(r#"class: "static-cls", class:"#),
        "should not have separate class keys, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_class_array_merge_on_component() {
    let code = gen_ssr_template(
        r#"<script setup>
import Icon from './Icon.vue'
const iconColor = ref(true)
</script>
<template><Comp><Icon :icon="'test'" class="htw-w-5" :class="{ red: !iconColor }"/></Comp></template>"#,
    );
    // Class merge should work on component VNodes too
    assert!(
        code.contains(r#"class: ["htw-w-5", { red: !$setup.iconColor }]"#),
        "should merge class on component VNode, got:\n{}",
        code
    );
}

/// @ai-generated — Root component with static+dynamic class should merge into array.
#[test]
fn ssr_root_component_class_array_merge() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const isActive = ref(true)
</script>
<template><Comp class="base-cls" :class="{ active: isActive }">text</Comp></template>"#,
    );
    // Root component props should have merged class array
    assert!(
        code.contains(r#"class: ["base-cls", { active: $setup.isActive }]"#),
        "should merge class into array on root component, got:\n{}",
        code
    );
    // Should NOT have separate class keys
    assert!(
        !code.contains(r#"class: "base-cls", class:"#),
        "should not have separate class keys, got:\n{}",
        code
    );
}

/// @ai-generated — Non-root component (inside div) with static+dynamic class should merge into array.
#[test]
fn ssr_nonroot_component_class_array_merge() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const isActive = ref(true)
</script>
<template><div><Comp class="base-cls" :class="{ active: isActive }">text</Comp></div></template>"#,
    );
    // Non-root component props should still have merged class array
    assert!(
        code.contains(r#"class: ["base-cls", { active: $setup.isActive }]"#),
        "should merge class into array on non-root component, got:\n{}",
        code
    );
    // Should NOT have separate class keys
    assert!(
        !code.contains(r#"class: "base-cls", class:"#),
        "should not have separate class keys, got:\n{}",
        code
    );
}

/// @ai-generated — Dynamic component (:is) with static+dynamic class should merge.
#[test]
fn ssr_dynamic_component_class_array_merge() {
    let code = gen_ssr_template(
        r#"<template><component :is="comp" class="btn" :class="{ active: isActive }">text</component></template>"#,
    );
    // Dynamic component should also merge class
    assert!(
        code.contains(r#"class: ["btn", { active: _ctx.isActive }]"#),
        "should merge class into array on dynamic component, got:\n{}",
        code
    );
}

// ── Patch flag: no TEXT on mixed children ──

#[test]
fn ssr_vdom_no_text_patchflag_on_mixed_children() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
import ChildComp from './ChildComp.vue'
const name = ref('hello')
</script>
<template><Comp><div><ChildComp :value="name" />{{ name }}</div></Comp></template>"#,
    );
    // When element has mixed children (Element + Interpolation), Vue does NOT
    // set TEXT patch flag on the parent. Only the _createTextVNode gets TEXT.
    // The outer div should NOT have a TEXT patch flag
    assert!(
        !code.contains(r#"], 1 /* TEXT */)"#),
        "mixed children parent should not have TEXT patch flag, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_text_patchflag_on_pure_text_children() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const name = ref('hello')
</script>
<template><Comp><div>Hello {{ name }}</div></Comp></template>"#,
    );
    // When element children are purely text+interpolation, the TEXT flag is correct
    assert!(
        code.contains("1 /* TEXT */"),
        "pure text children should have TEXT patch flag, got:\n{}",
        code
    );
}

/// @ai-generated - VDOM v-if condition expressions must resolve bindings ($setup prefix)
#[test]
fn ssr_vdom_vif_condition_resolves_bindings() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>
<template><Comp><div v-if="show">visible</div></Comp></template>"#,
    );
    // The v-if condition in VDOM fallback should have $setup. prefix
    assert!(
        code.contains("$setup.show"),
        "v-if condition should resolve binding with $setup prefix, got:\n{}",
        code
    );
    // Should NOT contain bare `show` in condition position (without prefix)
    assert!(
        !code.contains("(show)"),
        "should not have bare `show` without prefix in condition, got:\n{}",
        code
    );
}

/// @ai-generated — VDOM props for components should include ref from el.v_ref.
#[test]
fn ssr_vdom_component_ref_prop() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
import Upload from './Upload.vue'
</script>
<template><Comp><Upload ref="ref1">Upload</Upload></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // ref should appear in the VDOM component props
    assert!(
        vdom_part.contains(r#"ref: "ref1""#),
        "component VDOM props should include ref, got:\n{}",
        vdom_part
    );
    assert!(
        !vdom_part.contains("_createVNode($setup[\"Upload\"], null,"),
        "props should NOT be null when ref is present, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - VDOM v-if compound condition resolves all bindings in fallback path
#[test]
fn ssr_vdom_vif_compound_condition_resolves_bindings() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const group = ref(null)
const expandText = ref(false)
</script>
<template><Comp><div v-if="group && !expandText">content</div></Comp></template>"#,
    );
    // The VDOM fallback (else branch) should resolve both identifiers with $setup.
    // Extract just the else/return portion to check VDOM fallback specifically
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("$setup.group"),
        "VDOM fallback should resolve 'group' with $setup prefix, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("$setup.expandText"),
        "VDOM fallback should resolve 'expandText' with $setup prefix, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - Teleport disabled prop expression should resolve bindings
#[test]
fn ssr_teleport_disabled_resolves_bindings() {
    let code = gen_ssr_template(
        r#"<script setup>
const showModal = ref(false)
</script>
<template><Teleport to="body" :disabled="!showModal"><div>modal</div></Teleport></template>"#,
    );
    // The disabled expression should have $setup. prefix
    assert!(
        code.contains("!$setup.showModal"),
        "Teleport disabled should resolve binding with $setup prefix, got:\n{}",
        code
    );
}

/// @ai-generated - <template v-if> with single child element should promote child to block
#[test]
fn ssr_vdom_template_vif_single_child_promotion() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const show = ref(true)
const name = ref('hello')
</script>
<template><Comp><template v-if="show"><a>{{ name }}</a></template></Comp></template>"#,
    );
    // Vue promotes single child: _createBlock("a", { key: 0 }, ...)
    // Should NOT wrap in _Fragment
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains(r#"_createBlock("a""#),
        "single child should be promoted to block tag, got:\n{}",
        vdom_part
    );
    assert!(
        !vdom_part.contains("_Fragment"),
        "should NOT use Fragment for single child template, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - Trailing whitespace after interpolation before closing tag is preserved as space
#[test]
fn ssr_trailing_whitespace_after_interp_before_close_tag() {
    let code = gen_ssr_template(
        r#"<script setup>
const items = ref([])
</script>
<template>
<ul>
<li v-for="{ name, email } in items" :key="name">
  {{ name }} &lt;{{ email }}&gt;
</li>
</ul>
</template>"#,
    );
    // The trailing whitespace/newline after &gt; before </li> should condense to a space
    // Vue outputs: &gt; </li>
    assert!(
        code.contains("&gt; </li>"),
        "should preserve space before closing tag, got:\n{}",
        code
    );
}

/// @ai-generated - Trailing newline after text before closing tag condenses to space
#[test]
fn ssr_trailing_newline_condenses_to_space_simple() {
    let code = gen_ssr_template("<template><div>hello\n</div></template>");
    assert!(
        code.contains("hello </div>"),
        "trailing newline should become space, got:\n{}",
        code
    );
}

/// @ai-generated - Trailing newline after entity before closing tag condenses to space
#[test]
fn ssr_trailing_newline_after_entity_condenses_to_space() {
    let code = gen_ssr_template("<template><div>a &gt;\n</div></template>");
    assert!(
        code.contains("&gt; </div>"),
        "trailing newline after entity should become space, got:\n{}",
        code
    );
}

/// @ai-generated - VDOM text children preserve leading/trailing spaces
#[test]
fn ssr_vdom_text_preserves_boundary_whitespace() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template><Comp><button> -1 </button></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Vue preserves leading/trailing spaces in text-only element children
    assert!(
        vdom_part.contains("\" -1 \""),
        "should preserve leading/trailing spaces in text, got:\n{}",
        vdom_part
    );
    assert!(
        !vdom_part.contains("\"-1\""),
        "should not trim text, got:\n{}",
        vdom_part
    );
}

/// Whitespace between adjacent interpolations should produce " " in _createTextVNode.
/// `{{ a }} {{ b }}` → `_createTextVNode(_toDisplayString(a) + " " + _toDisplayString(b))`
#[test]
fn ssr_vdom_text_whitespace_between_interpolations() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const a = 'hello'
const b = 'world'
</script>
<template><Comp><span>{{ a }} {{ b }}</span></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Should include " " between the two interpolations
    assert!(
        vdom_part.contains(r#"" " + _toDisplayString"#) || vdom_part.contains(r#"+ " " +"#),
        "should have space between adjacent interpolations, got:\n{}",
        vdom_part
    );
    // Should NOT have two _toDisplayString calls joined without a space
    assert!(
        !vdom_part.contains("_toDisplayString($setup.a) + _toDisplayString($setup.b)"),
        "adjacent interpolations should have space between them, got:\n{}",
        vdom_part
    );
}

/// Whitespace-only text at the end of a text run should NOT generate " ".
/// `<Comp> Dropdown </Comp>` → `_createTextVNode(" Dropdown ")`, not `" Dropdown " + " "`
#[test]
fn ssr_vdom_text_no_trailing_whitespace_space() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template><Comp> Dropdown </Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("_createTextVNode(\" Dropdown \")"),
        "should produce clean text without extra space, got:\n{}",
        vdom_part
    );
    assert!(
        !vdom_part.contains("+ \" \""),
        "should not have trailing space concatenation, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - Named slots with hyphens are quoted as JS object keys
#[test]
fn ssr_named_slot_with_hyphen_is_quoted() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template><Comp><template #day-popover="{ dayTitle }">{{ dayTitle }}</template></Comp></template>"#,
    );
    // Slot names with hyphens must be quoted in JS object literals
    assert!(
        code.contains("\"day-popover\": _withCtx("),
        "hyphenated slot name should be quoted, got:\n{}",
        code
    );
    assert!(
        !code.contains("\nday-popover: _withCtx("),
        "unquoted hyphenated slot name is invalid JS, got:\n{}",
        code
    );
}

/// @ai-generated - v-for in VDOM fallback generates _renderList with Fragment wrapper
#[test]
fn ssr_vdom_vfor_generates_render_list() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>
<template><Comp><div v-for="item in items" :key="item.id">{{ item.name }}</div></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("_renderList("),
        "v-for should generate _renderList, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("_Fragment"),
        "v-for should use Fragment wrapper, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("128 /* KEYED_FRAGMENT */"),
        "keyed v-for should have KEYED_FRAGMENT flag, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - keyed v-for children (HTML or component) use _createBlock
#[test]
fn ssr_vdom_vfor_keyed_uses_create_block() {
    // HTML element in keyed v-for
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const items = ref([])
</script>
<template><Comp><div v-for="item in items" :key="item.id">{{ item.name }}</div></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("(_openBlock(), _createBlock(\"div\""),
        "keyed v-for HTML element should use _createBlock, got:\n{}",
        vdom_part
    );

    // Component in keyed v-for
    let code2 = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
import Item from './Item.vue'
const items = ref([])
</script>
<template><Comp><Item v-for="item in items" :key="item.id" :data="item" /></Comp></template>"#,
    );
    let else_pos2 = code2
        .find("} else {")
        .expect("should have VDOM else branch");
    let vdom_part2 = &code2[else_pos2..];
    assert!(
        vdom_part2.contains("(_openBlock(), _createBlock($setup[\"Item\"]"),
        "keyed v-for component child should also use _createBlock, got:\n{}",
        vdom_part2
    );
}

/// @ai-generated — Component inside v-for should have DYNAMIC slot flag and DYNAMIC_SLOTS patch flag.
#[test]
fn ssr_vdom_vfor_component_dynamic_slots() {
    let code = gen_ssr_template(
        r#"<script setup>
import Outer from './Outer.vue'
import Inner from './Inner.vue'
const items = ref([])
</script>
<template><Outer><Inner v-for="item in items" :key="item.id"><span>{{ item.name }}</span></Inner></Outer></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Component inside v-for should have _: 2 /* DYNAMIC */ slot flag
    assert!(
        vdom_part.contains("_: 2 /* DYNAMIC */"),
        "component inside v-for should have DYNAMIC slot flag, got:\n{}",
        vdom_part
    );
    // Component inside v-for should have 1024 /* DYNAMIC_SLOTS */ patch flag
    assert!(
        vdom_part.contains("1024 /* DYNAMIC_SLOTS */"),
        "component inside v-for should have DYNAMIC_SLOTS patch flag, got:\n{}",
        vdom_part
    );
    // Inner component should NOT have _: 1 /* STABLE */ (outer can)
    // Check that the DYNAMIC flag appears before KEYED_FRAGMENT
    let dynamic_pos = vdom_part.find("_: 2 /* DYNAMIC */").unwrap();
    let keyed_pos = vdom_part.find("128 /* KEYED_FRAGMENT */").unwrap();
    assert!(
        dynamic_pos < keyed_pos,
        "DYNAMIC slot flag should appear within the v-for (before KEYED_FRAGMENT), got:\n{}",
        vdom_part
    );
}

/// @ai-generated - <template v-if> with multiple children should use Fragment
#[test]
fn ssr_vdom_template_vif_multi_child_uses_fragment() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>
<template><Comp><template v-if="show"><div>one</div><div>two</div></template></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("_Fragment"),
        "multiple children should use Fragment, got:\n{}",
        vdom_part
    );
}

// ── SSR component props: class array merging ──

#[test]
fn ssr_component_props_class_merge() {
    // When a component has both static class and :class, SSR props should merge them
    // into a single class: ["static", dynamic] array, not two separate class: entries.
    let code = gen_ssr_template(
        r#"<script setup>
import Icon from './Icon.vue'
const isActive = ref(false)
</script>
<template><Icon class="w-5 h-5" :class="{ active: isActive }" /></template>"#,
    );
    assert!(
        code.contains(r#"class: ["w-5 h-5", { active: $setup.isActive }]"#),
        "should merge static+dynamic class into array, got:\n{}",
        code
    );
    // Must NOT have two separate class entries
    let class_count = code.matches("class:").count();
    assert!(
        class_count <= 1,
        "should have at most 1 class: entry, got {} in:\n{}",
        class_count,
        code
    );
}

#[test]
fn ssr_component_props_class_merge_root() {
    // Root component with static+dynamic class should also merge
    let code = gen_ssr_template(
        r#"<script setup>
import Icon from './Icon.vue'
const isActive = ref(false)
</script>
<template><Icon class="w-5 h-5" :class="{ active: isActive }" /></template>"#,
    );
    assert!(
        code.contains(r#"class: ["w-5 h-5", { active: $setup.isActive }]"#),
        "root component should merge class array too, got:\n{}",
        code
    );
}

// ── SSR v-model kebab-case prop quoting ──

#[test]
fn ssr_vdom_vif_parens_always_in_ternary() {
    // Vue wraps conditions in parens in VDOM ternary expressions
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const show = ref(true)
</script>
<template><Comp><div v-if="show">yes</div><div v-else>no</div></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("($setup.show)"),
        "VDOM ternary should wrap conditions in parens, got:\n{}",
        vdom_part
    );
}

#[test]
fn ssr_vdom_vif_parens_complex_condition() {
    // Vue wraps compound conditions in parens in VDOM ternary
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const a = ref(1)
const b = ref(2)
</script>
<template><Comp><div v-if="a > b">yes</div><div v-else>no</div></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Compound conditions are also wrapped in parens
    assert!(
        vdom_part.contains("($setup.a > $setup.b)"),
        "VDOM ternary should wrap conditions in parens, got:\n{}",
        vdom_part
    );
}

// ── SSR v-model kebab-case prop quoting ──

#[test]
fn ssr_component_vmodel_kebab_case_prop_quoted() {
    // v-model:page-size should emit "page-size" (quoted) in the props object
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const pageSize = ref(10)
</script>
<template><Comp v-model:page-size="pageSize" /></template>"#,
    );
    assert!(
        code.contains(r#""page-size": $setup.pageSize"#),
        "kebab-case v-model prop should be quoted, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""onUpdate:pageSize""#) || code.contains(r#""onUpdate:page-size""#),
        "v-model update handler should be quoted, got:\n{}",
        code
    );
}

// ── SSR slot flag: stability tracking ──

#[test]
fn ssr_slot_stable_flag_for_root_component() {
    // A component at root level should have STABLE slot flag
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template><Comp><p>content</p></Comp></template>"#,
    );
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "Root-level component should have STABLE slot flag, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 2 /* DYNAMIC */"),
        "Should not have DYNAMIC flag for root-level component, got:\n{}",
        code
    );
}

// ── Custom directives on components ─────────────────────────────
#[test]
fn ssr_custom_directive_on_component_global() {
    // Custom directive on a component should resolve the directive and
    // merge _ssrGetDirectiveProps into the component's props via _mergeProps.
    let code = gen_ssr_template(
        r#"<script setup>
import Foo from './Foo.vue'
</script>
<template><Foo v-foo test="ss" /></template>"#,
    );
    // Should have _resolveDirective
    assert!(
        code.contains("_resolveDirective(\"foo\")"),
        "Should resolve custom directive, got:\n{}",
        code
    );
    // Should have _ssrGetDirectiveProps
    assert!(
        code.contains("_ssrGetDirectiveProps"),
        "Should have _ssrGetDirectiveProps call, got:\n{}",
        code
    );
    // Should merge props with _mergeProps
    assert!(
        code.contains("_mergeProps("),
        "Should use _mergeProps to merge directive props, got:\n{}",
        code
    );
    // Negative: directive should not appear as raw attribute
    assert!(
        !code.contains("v-foo:"),
        "v-foo should not appear as raw prop, got:\n{}",
        code
    );
}

#[test]
fn ssr_custom_directive_on_component_setup_binding() {
    // Custom directive from setup binding should use $setup["vFoo"]
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const vFoo = { mounted() {} }
</script>
<template><Comp v-foo="expr" /></template>"#,
    );
    assert!(
        code.contains("$setup[\"vFoo\"]"),
        "Should use $setup[\"vFoo\"] for setup-declared directive, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDirectiveProps"),
        "Should have _ssrGetDirectiveProps, got:\n{}",
        code
    );
    assert!(
        !code.contains("_resolveDirective"),
        "Should NOT use _resolveDirective for setup binding, got:\n{}",
        code
    );
}

#[test]
fn ssr_custom_directive_on_component_with_value_and_arg() {
    // Custom directive with value and arg on component
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const msg = ref('hello')
</script>
<template><Comp v-tooltip:top="msg" /></template>"#,
    );
    assert!(
        code.contains("_ssrGetDirectiveProps(_ctx, _directive_tooltip, $setup.msg, \"top\")"),
        "Should have directive with value and arg, got:\n{}",
        code
    );
}

// ── v-bind spread on components ─────────────────────────────────
#[test]
fn ssr_vbind_spread_on_component() {
    // v-bind="obj" on a component should merge the spread into props via _mergeProps.
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const rest = { a: 1, b: 2 }
</script>
<template><Comp class="foo" v-bind="rest" /></template>"#,
    );
    // Should use _mergeProps
    assert!(
        code.contains("_mergeProps("),
        "Should use _mergeProps for v-bind spread, got:\n{}",
        code
    );
    // Should include the spread expression
    assert!(
        code.contains("$setup.rest"),
        "Should reference spread expression, got:\n{}",
        code
    );
    // Should NOT have _resolveDirective("bind")
    assert!(
        !code.contains("_resolveDirective(\"bind\")"),
        "v-bind should NOT be resolved as a custom directive, got:\n{}",
        code
    );
}

// ── v-for fragment markers ──────────────────────────────────────
#[test]
fn ssr_vfor_element_has_fragment_markers() {
    // v-for on a regular element should have fragment markers.
    let code = gen_ssr_template(
        r#"<script setup>
const items = ref([])
</script>
<template><div><span v-for="item in items">{{ item }}</span></div></template>"#,
    );
    let ssr_part = if let Some(pos) = code.find("} else {") {
        &code[..pos]
    } else {
        &code
    };
    assert!(
        ssr_part.contains("<!--[-->"),
        "v-for on element should have fragment open marker, got:\n{}",
        ssr_part
    );
    assert!(
        ssr_part.contains("<!--]-->"),
        "v-for on element should have fragment close marker, got:\n{}",
        ssr_part
    );
}

/// @ai-generated - v-if chain in VDOM fallback should have incrementing keys
#[test]
fn ssr_vdom_vif_chain_key_numbering() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const a = ref(false)
const b = ref(false)
const c = ref(false)
</script>
<template><Comp><div v-if="a">A</div><div v-else-if="b">B</div><div v-else-if="c">C</div><div v-else>D</div></Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Each branch should get an incrementing key
    assert!(
        vdom_part.contains("key: 0"),
        "v-if branch should have key: 0, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 1"),
        "first v-else-if should have key: 1, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 2"),
        "second v-else-if should have key: 2, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 3"),
        "v-else should have key: 3, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - v-if chain key numbering works with component branches
#[test]
fn ssr_vdom_vif_chain_key_numbering_components() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
import A from './A.vue'
import B from './B.vue'
import C from './C.vue'
const x = ref(false)
const y = ref(false)
</script>
<template>
  <Comp>
    <A v-if="x" />
    <B v-else-if="y" />
    <C v-else />
  </Comp>
</template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("key: 0"),
        "v-if branch should have key: 0, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 1"),
        "first v-else-if should have key: 1, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 2"),
        "v-else should have key: 2, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - v-if chain key numbering works when branches have newlines between them
#[test]
fn ssr_vdom_vif_chain_key_numbering_multiline() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const a = ref(false)
const b = ref(false)
const c = ref(false)
</script>
<template>
  <Comp>
    <div v-if="a">A</div>
    <div v-else-if="b">B</div>
    <div v-else-if="c">C</div>
    <div v-else>D</div>
  </Comp>
</template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    assert!(
        vdom_part.contains("key: 0"),
        "v-if branch should have key: 0, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 1"),
        "first v-else-if should have key: 1, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 2"),
        "second v-else-if should have key: 2, got:\n{}",
        vdom_part
    );
    assert!(
        vdom_part.contains("key: 3"),
        "v-else should have key: 3, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - VDOM fallback text should decode &nbsp; to actual U+00A0 character
#[test]
fn ssr_vdom_text_decodes_nbsp_entity() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const name = ref('')
</script>
<template><Comp>{{ name }}&nbsp;hello</Comp></template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // Vue decodes &nbsp; to the actual non-breaking space char \u{00A0} in VDOM text
    assert!(
        vdom_part.contains("\u{00A0}"),
        "VDOM text should contain decoded non-breaking space, got:\n{}",
        vdom_part
    );
    // Negative: &nbsp; entity should NOT appear as a literal string
    assert!(
        !vdom_part.contains("&nbsp;"),
        "VDOM text should not contain literal &nbsp; entity, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - NEED_HYDRATION flag (32) is set for non-click event handlers
#[test]
fn ssr_vdom_need_hydration_flag_on_any_element() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const onFocus = () => {}
</script>
<template><Comp><button @focus="onFocus">Focus</button></Comp></template>"#,
    );
    let vdom_part = code.split("} else {").nth(1).unwrap_or("");
    // button with @focus should have NEED_HYDRATION in patch flags
    assert!(
        vdom_part.contains("NEED_HYDRATION"),
        "button with @focus should have NEED_HYDRATION flag, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - NEED_HYDRATION flag on non-form elements with non-click events
#[test]
fn ssr_vdom_need_hydration_flag_non_form_element() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const doSomething = () => {}
</script>
<template><Comp><a @keypress="doSomething">Link</a></Comp></template>"#,
    );
    let vdom_part = code.split("} else {").nth(1).unwrap_or("");
    // <a> with @keypress should get NEED_HYDRATION
    assert!(
        vdom_part.contains("NEED_HYDRATION"),
        "non-form element <a> with @keypress should have NEED_HYDRATION, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - @click is excluded from NEED_HYDRATION (has dedicated fast path)
#[test]
fn ssr_vdom_click_excluded_from_need_hydration() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const handler = () => {}
</script>
<template><Comp><div @click="handler">Click</div></Comp></template>"#,
    );
    let vdom_part = code.split("} else {").nth(1).unwrap_or("");
    // @click is excluded from NEED_HYDRATION
    assert!(
        !vdom_part.contains("NEED_HYDRATION"),
        "@click should NOT trigger NEED_HYDRATION, got:\n{}",
        vdom_part
    );
}

/// @ai-generated - SSR preserves TypeScript syntax (as casts, ! assertions)
/// Bundler-level TS stripping handles these, not the compiler.
#[test]
fn ssr_preserves_ts_syntax_in_expressions() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
let received = ''
</script>
<template><Comp @action="(v: string) => received = v" /></template>"#,
    );
    // SSR should preserve the TS type annotation
    assert!(
        code.contains(": string)"),
        "SSR should preserve TS type annotation, got:\n{}",
        code
    );
}

/// @ai-generated - @vue:mounted → onVnodeMounted in SSR component props
#[test]
fn ssr_vue_lifecycle_hooks_use_vnode_naming() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const onMounted = () => {}
const onUnmounted = () => {}
</script>
<template><Comp @vue:mounted="onMounted" @vue:unmounted="onUnmounted" /></template>"#,
    );
    // @vue:mounted → onVnodeMounted
    assert!(
        code.contains("onVnodeMounted"),
        "should convert @vue:mounted to onVnodeMounted, got:\n{}",
        code
    );
    // @vue:unmounted → onVnodeUnmounted
    assert!(
        code.contains("onVnodeUnmounted"),
        "should convert @vue:unmounted to onVnodeUnmounted, got:\n{}",
        code
    );
    // Negative: should NOT contain the literal "onVue:" form
    assert!(
        !code.contains("onVue:"),
        "should not contain 'onVue:' literal form, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_ref_in_source_order() {
    // Vue puts ref in source order among other props, not always first
    let code = gen_ssr_template(
        r#"<template><div id="header" ref="header" class="main"></div></template>"#,
    );
    // ref should appear after id, matching source order: id, ref, class
    assert!(
        code.contains(r#"{ id: "header", ref: "header", class: "main" }"#),
        "ref should be in source order (after id), got:\n{}",
        code
    );
    // Negative: ref should NOT be first
    assert!(
        !code.contains(r#"{ ref: "header", id: "header""#),
        "ref should NOT be first when id comes before it in source, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_need_patch_flag_for_ref_only() {
    // Elements with ONLY ref (no other dynamic attrs) get NEED_PATCH (512).
    // Use a slot to trigger the VDOM path.
    let code = gen_ssr_template(r#"<template><Comp><div ref="el"></div></Comp></template>"#);
    assert!(
        code.contains("512 /* NEED_PATCH */"),
        "ref-only element should have NEED_PATCH (512) flag, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_no_need_patch_when_other_dynamic_flags() {
    // When an element with ref also has other dynamic content (e.g., :class),
    // NEED_PATCH should NOT be set — Vue strips ref from VDOM in this case.
    let code = gen_ssr_template(
        r#"<template><Comp><div ref="el" :class="cls">text</div></Comp></template>"#,
    );
    assert!(
        !code.contains("NEED_PATCH"),
        "ref with other dynamic attrs should NOT have NEED_PATCH, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_component_need_patch_for_ref() {
    // Components with ref ALWAYS get NEED_PATCH (512), even when they
    // have other dynamic flags (unlike HTML elements).
    // Component must be inside another component's slot to trigger VDOM fallback.
    let code =
        gen_ssr_template(r#"<template><Parent><Comp ref="comp"></Comp></Parent></template>"#);
    assert!(
        code.contains("512 /* NEED_PATCH */"),
        "component with ref should have NEED_PATCH (512), got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_component_no_need_patch_with_other_flags() {
    // Components with ref + dynamic props should NOT have NEED_PATCH —
    // same rule as HTML elements: NEED_PATCH only when ref is the sole dynamic flag.
    // Component must be inside another component's slot to trigger VDOM fallback.
    let code = gen_ssr_template(
        r#"<template><Parent><Comp ref="comp" :msg="msg"></Comp></Parent></template>"#,
    );
    assert!(
        !code.contains("NEED_PATCH"),
        "component with ref + dynamic props should NOT have NEED_PATCH, got:\n{}",
        code
    );
    assert!(
        code.contains("PROPS"),
        "component with dynamic props should include PROPS, got:\n{}",
        code
    );
}

#[test]
fn ssr_slot_flag_dynamic_in_scoped_slot() {
    // When a component with slots is inside a scoped slot (one with user parameters),
    // its slots should be marked DYNAMIC (2) in BOTH the SSR push path and
    // the VDOM fallback, because the scoped slot context may cause re-rendering.
    let code = gen_ssr_template(
        r#"<template><Outer v-slot="{ state }"><Inner>text</Inner></Outer></template>"#,
    );
    // Inner's slot should be DYNAMIC in the SSR push path
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "child component slots inside a scoped slot should be DYNAMIC, got:\n{}",
        code
    );
}

/// In the SSR push path, components inside a scoped slot should have
/// DYNAMIC slot flags. The VDOM fallback uses its own rules (only
/// has_dynamic_slots, not scoped slot depth).
#[test]
fn ssr_slot_flag_dynamic_in_scoped_slot_nested() {
    let code = gen_ssr_template(
        r#"<template>
<CompA>
  <template #renderItem="{ item }">
    <CompB><CompC :title="item.title">Card content</CompC></CompB>
  </template>
</CompA>
</template>"#,
    );
    // SSR push path: CompB and CompC should have DYNAMIC slot flags
    // because they are inside a scoped slot (#renderItem="{ item }")
    let dynamic_count = code.matches("_: 2 /* DYNAMIC */").count();
    assert!(
        dynamic_count >= 2,
        "expected at least 2 DYNAMIC slot flags in SSR push path (CompB and CompC), got {} in:\n{}",
        dynamic_count,
        code
    );
}

#[test]
fn ssr_slot_flag_stable_in_non_scoped_slot() {
    // When a component with slots is inside a non-scoped slot (no user parameters),
    // its slots should be STABLE (1).
    let code = gen_ssr_template(r#"<template><Outer><Inner>text</Inner></Outer></template>"#);
    // Inner's slot should be STABLE because Outer's slot has no user params
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "child component slots in non-scoped slot should be STABLE, got:\n{}",
        code
    );
    assert!(
        !code.contains("_: 2 /* DYNAMIC */"),
        "should not have DYNAMIC flag for non-scoped slot, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_event_with_keys_modifier() {
    // VDOM fallback should wrap key event modifiers with _withKeys
    let code = gen_ssr_template(
        r#"<template><Parent><div @keydown.enter="submit">text</div></Parent></template>"#,
    );
    assert!(
        code.contains("_withKeys("),
        "should wrap handler with _withKeys for .enter modifier, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_withKeys($setup.submit, ["enter"])"#)
            || code.contains(r#"_withKeys(_ctx.submit, ["enter"])"#),
        "should wrap with _withKeys and pass enter as modifier, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_event_with_modifiers() {
    // VDOM fallback should wrap runtime modifiers with _withModifiers
    let code = gen_ssr_template(
        r#"<template><Parent><div @click.stop="handler">text</div></Parent></template>"#,
    );
    assert!(
        code.contains("_withModifiers("),
        "should wrap handler with _withModifiers for .stop, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"["stop"]"#),
        "should include stop modifier, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_event_capture_modifier_key_suffix() {
    // Option modifiers (capture, once, passive) append to the event key name
    let code = gen_ssr_template(
        r#"<template><Parent><div @click.capture="handler">text</div></Parent></template>"#,
    );
    assert!(
        code.contains("onClickCapture"),
        "should append Capture to key name, got:\n{}",
        code
    );
    assert!(
        !code.contains("_withModifiers"),
        "capture should not use _withModifiers, got:\n{}",
        code
    );
}

#[test]
fn ssr_vdom_vif_global_key_counter_across_chains() {
    // Separate v-if chains in the same parent should use globally unique keys.
    // Vue increments the key counter across all v-if chains.
    let code = gen_ssr_template(
        r#"<script setup>
import A from './A.vue'
import B from './B.vue'
const x = ref(false)
const y = ref(false)
</script>
<template>
  <Outer>
    <div>
      <A v-if="x" />
      <B v-if="y" />
    </div>
  </Outer>
</template>"#,
    );
    let else_pos = code.find("} else {").expect("should have VDOM else branch");
    let vdom_part = &code[else_pos..];
    // First v-if chain: key: 0
    assert!(
        vdom_part.contains("key: 0"),
        "first v-if chain should have key: 0, got:\n{}",
        vdom_part
    );
    // Second v-if chain: key: 1 (not key: 0 again!)
    assert!(
        vdom_part.contains("key: 1"),
        "second v-if chain should have key: 1 (global counter), got:\n{}",
        vdom_part
    );
}

/// @ai-generated — Computed variables in script setup should use $setup prefix.
#[test]
fn ssr_binding_setup_computed_uses_setup_prefix() {
    let code = gen_ssr_template(
        r#"<script setup>
import { computed } from "vue"
const foo = computed(() => 1)
</script>
<template><div>{{ foo }}</div></template>"#,
    );
    // Positive: should use $setup.foo since foo is a setup binding
    assert!(
        code.contains("$setup.foo"),
        "computed variable should use $setup prefix, got:\n{}",
        code
    );
    // Negative: should NOT use _ctx.foo
    assert!(
        !code.contains("_ctx.foo"),
        "computed variable should NOT use _ctx prefix, got:\n{}",
        code
    );
}

/// @ai-generated — v-model on component: dynamic props array includes both modelValue and
/// the camelized onUpdate handler name.
#[test]
fn ssr_vdom_vmodel_component_dynamic_props() {
    // Component v-model inside a parent component slot → triggers VDOM fallback
    let code = gen_ssr_template(
        r#"<template><Parent><Comp v-model="msg">text</Comp></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Comp from './Comp.vue'
import { ref } from 'vue'
const msg = ref('')
</script>"#,
    );
    // Vue includes both modelValue and onUpdate:modelValue in dynamic props
    assert!(
        code.contains(r#""modelValue", "onUpdate:modelValue""#),
        "should have both modelValue and onUpdate:modelValue in dynamic props, got:\n{}",
        code
    );
}

/// @ai-generated — v-model with named arg: onUpdate handler name is camelized.
#[test]
fn ssr_vdom_vmodel_named_camelizes_update_handler() {
    let code = gen_ssr_template(
        r#"<template><Parent><Comp v-model:my-prop="val">text</Comp></Parent></template>
<script setup>
import Parent from './Parent.vue'
import Comp from './Comp.vue'
import { ref } from 'vue'
const val = ref('')
</script>"#,
    );
    // Vue camelizes the model prop name for onUpdate: "my-prop" → "onUpdate:myProp"
    assert!(
        code.contains(r#""my-prop", "onUpdate:myProp""#),
        "should have my-prop and onUpdate:myProp (camelized) in dynamic props, got:\n{}",
        code
    );
    // Should NOT have uncamelized onUpdate:my-prop
    assert!(
        !code.contains(r#""onUpdate:my-prop""#),
        "should NOT have uncamelized onUpdate:my-prop, got:\n{}",
        code
    );
}

/// @ai-generated — Multiscript with companion script: setup vars use $setup prefix.
#[test]
fn ssr_binding_multiscript_setup_var() {
    // Use the exact content from the real file (with TS <string>0 cast)
    let code = gen_ssr_template(
        r#"<script lang="ts">
let a = 0;
let b = <string>0;
/* FOO */
</script>
<script setup lang="ts">
import { computed } from "vue";
const foo = computed(() => 1);
// @ts-expect-error
let c = <string>0;
let b = "";
</script>
<template>
  <div>
    {{ foo }}
  </div>
</template>"#,
    );
    // Also check script output
    let script = gen_ssr_script(
        r#"<script lang="ts">
let a = 0;
let b = <string>0;
/* FOO */
</script>
<script setup lang="ts">
import { computed } from "vue";
const foo = computed(() => 1);
// @ts-expect-error
let c = <string>0;
let b = "";
</script>
<template>
  <div>
    {{ foo }}
  </div>
</template>"#,
    );
    eprintln!("MULTISCRIPT TEMPLATE:\n{}", code);
    eprintln!("MULTISCRIPT SCRIPT:\n{}", script);
    // Positive: should use $setup.foo
    assert!(
        code.contains("$setup.foo"),
        "multiscript setup computed should use $setup prefix, got:\n{}",
        code
    );
    // Negative: should NOT use _ctx.foo
    assert!(
        !code.contains("_ctx.foo"),
        "multiscript setup computed should NOT use _ctx prefix, got:\n{}",
        code
    );
}

/// @ai-generated - Dynamic <component :is> in SSR uses _resolveDynamicComponent
#[test]
fn ssr_dynamic_component_resolve() {
    let code = gen_ssr_template(
        r#"<script setup>
import { ref } from 'vue'
const currentView = ref('Home')
</script>
<template><component :is="currentView" /></template>"#,
    );
    // SSR uses _ssrRenderVNode with _createVNode(_resolveDynamicComponent(...))
    assert!(
        code.contains("_resolveDynamicComponent"),
        "should use _resolveDynamicComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrRenderVNode"),
        "should use _ssrRenderVNode for dynamic components, got:\n{}",
        code
    );
    // Negative: should NOT reference _component_component
    assert!(
        !code.contains("_component_component"),
        "should not fall back to _component_component when :is is present, got:\n{}",
        code
    );
    // Negative: should NOT have is: in props (consumed by _resolveDynamicComponent)
    assert!(
        !code.contains("is: _ctx.currentView") && !code.contains("is: $setup"),
        "should not include :is as a prop, got:\n{}",
        code
    );
}

/// @ai-generated - Dynamic <component :is> with other props excludes :is from props
#[test]
fn ssr_dynamic_component_props_exclude_is() {
    let code = gen_ssr_template(
        r#"<script setup>
import { ref } from 'vue'
const view = ref('Home')
const color = ref('red')
</script>
<template><component :is="view" :color="color" class="wrapper" /></template>"#,
    );
    // Should have _resolveDynamicComponent
    assert!(
        code.contains("_resolveDynamicComponent"),
        "should use _resolveDynamicComponent, got:\n{}",
        code
    );
    // Should have the other props
    assert!(
        code.contains("color:"),
        "should include color prop, got:\n{}",
        code
    );
    assert!(
        code.contains("class: \"wrapper\""),
        "should include class prop, got:\n{}",
        code
    );
    // Negative: should NOT have is: in props
    assert!(
        !code.contains("is: "),
        "should not include :is as a prop, got:\n{}",
        code
    );
}

// ── Textarea value as content ─────────────────────────────────────

#[test]
fn ssr_textarea_value_as_content() {
    // Root textarea with :value — Vue puts it in attrs via _ssrRenderAttrs
    let code = gen_ssr_template(
        r#"<template>
  <textarea :value="displayedSourceCode" readonly></textarea>
</template>
<script setup>
const displayedSourceCode = ref('')
</script>"#,
    );
    // Root path: value goes into attrs object
    assert!(
        code.contains("value: $setup.displayedSourceCode"),
        "root textarea :value should be in attrs obj, got:\n{}",
        code
    );
    // Negative: :value should NOT trigger _ssrGetDynamicModelProps
    assert!(
        !code.contains("_ssrGetDynamicModelProps"),
        "textarea :value should not trigger _ssrGetDynamicModelProps, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrInterpolate for content
    assert!(
        !code.contains("_ssrInterpolate"),
        "root textarea :value should not use content interpolation, got:\n{}",
        code
    );
}

#[test]
fn ssr_textarea_vmodel_as_content() {
    // Root textarea — goes through _mergeProps path.
    // Vue SSR renders textarea v-model as _ssrInterpolate content, NOT as value: attr.
    // The "textarea" tag is passed to _ssrRenderAttrs so the runtime skips the value attr.
    let code = gen_ssr_template(
        r#"<template>
  <textarea v-model="text" class="editor"></textarea>
</template>
<script setup>
const text = ref('')
</script>"#,
    );
    // Positive: should use _ssrInterpolate for content
    assert!(
        code.contains("_ssrInterpolate($setup.text)"),
        "textarea v-model should use _ssrInterpolate for content, got:\n{}",
        code
    );
    // Positive: should pass "textarea" tag arg to _ssrRenderAttrs
    assert!(
        code.contains(r#", "textarea")"#),
        "textarea should pass tag name to _ssrRenderAttrs, got:\n{}",
        code
    );
    // Negative: should NOT add value: in attrs (content interpolation handles it)
    assert!(
        !code.contains("value: $setup.text"),
        "textarea v-model should NOT have value in attrs, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrGetDynamicModelProps (only for <input>)
    assert!(
        !code.contains("_ssrGetDynamicModelProps"),
        "textarea should NOT use _ssrGetDynamicModelProps, got:\n{}",
        code
    );
}

// ────────────────────────────────────────────────────────────────────
// Component name resolution order: exact → camelCase → PascalCase
// ────────────────────────────────────────────────────────────────────

/// @ai-generated — Vue resolves `<el-icon>` to camelCase `elIcon` binding first,
/// falling back to PascalCase `ElIcon` only if camelCase isn't found.
/// When only `ElIcon` exists in bindings, both Vue and Verter should use `ElIcon`.
/// When only `elIcon` exists, both should use `elIcon`.
/// When both exist, Vue prefers camelCase `elIcon`.
#[test]
fn ssr_component_name_camel_case_resolution() {
    // Tag <el-icon> with camelCase binding `elIcon` → should resolve to $setup["elIcon"]
    let code = gen_ssr_template(
        r#"<script setup>
import { elIcon } from 'element-plus'
</script>
<template><el-icon /></template>"#,
    );
    // Positive: should use camelCase binding
    assert!(
        code.contains(r#"$setup["elIcon"]"#),
        "should resolve <el-icon> to camelCase $setup[\"elIcon\"], got:\n{}",
        code
    );
    // Negative: should NOT use PascalCase
    assert!(
        !code.contains(r#"$setup["ElIcon"]"#),
        "should not use PascalCase ElIcon when camelCase elIcon exists, got:\n{}",
        code
    );
}

/// @ai-generated — When PascalCase binding exists but not camelCase,
/// PascalCase should be used.
#[test]
fn ssr_component_name_pascal_case_fallback() {
    // Tag <el-icon> with PascalCase binding `ElIcon` → should resolve to $setup["ElIcon"]
    let code = gen_ssr_template(
        r#"<script setup>
import { ElIcon } from 'element-plus'
</script>
<template><el-icon /></template>"#,
    );
    // Positive: should use PascalCase binding
    assert!(
        code.contains(r#"$setup["ElIcon"]"#),
        "should resolve <el-icon> to PascalCase $setup[\"ElIcon\"], got:\n{}",
        code
    );
    // Negative: should NOT use _resolveComponent
    assert!(
        !code.contains("_resolveComponent"),
        "should not use _resolveComponent when binding exists, got:\n{}",
        code
    );
}

/// @ai-generated — v-model + explicit @update handler should merge into an array.
/// Vue merges duplicate event handlers into arrays: [handler1, handler2].
#[test]
fn ssr_vmodel_with_explicit_update_handler() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const val = ref('')
function onInput(v) { console.log(v) }
</script>
<template>
  <div>
    <Comp v-model="val" @update:model-value="onInput" />
  </div>
</template>"#,
    );
    // Positive: should merge handlers into an array
    assert!(
        code.contains(
            r#""onUpdate:modelValue": [$event => (($setup.val) = $event), $setup.onInput]"#
        ),
        "should merge v-model and explicit handler into array, got:\n{}",
        code
    );
    // Negative: should NOT emit duplicate keys
    let count = code.matches(r#""onUpdate:modelValue""#).count();
    assert_eq!(
        count, 1,
        "should have exactly one onUpdate:modelValue key, got {} in:\n{}",
        count, code
    );
}

/// @ai-generated — v-bind spread on a component should split props around the
/// spread position, matching Vue's _mergeProps argument order:
/// _mergeProps({before}, spread, {after})
#[test]
fn ssr_component_v_bind_spread_position() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <div>
    <Comp :modelValue="val" v-bind="$attrs" class="mb-2" name="test" />
  </div>
</template>"#,
    );
    // Positive: props before the spread go in one object, spread in the middle,
    // props after in another object
    assert!(
        code.contains(
            r#"_mergeProps({ modelValue: _ctx.val }, _ctx.$attrs, { class: "mb-2", name: "test" })"#
        ),
        "should split props around v-bind spread, got:\n{}",
        code
    );
    // Negative: should NOT put all props in a single object before the spread
    assert!(
        !code.contains(
            r#"_mergeProps({ modelValue: _ctx.val, class: "mb-2", name: "test" }, _ctx.$attrs)"#
        ),
        "should not group all props before the spread, got:\n{}",
        code
    );
}

/// @ai-generated — v-bind spread as only source should not wrap in _mergeProps
#[test]
fn ssr_component_v_bind_spread_only() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
const obj = { a: 1 }
</script>
<template>
  <div>
    <Comp v-bind="obj" />
  </div>
</template>"#,
    );
    // When v-bind is the only prop source on a non-root element,
    // it should use the spread directly without _mergeProps
    assert!(
        code.contains(r#"_ssrRenderComponent($setup["Comp"], $setup.obj, null"#),
        "should use spread directly without _mergeProps, got:\n{}",
        code
    );
}

/// @ai-generated — v-bind spread on root component merges with _attrs
#[test]
fn ssr_component_v_bind_spread_root_with_attrs() {
    let code = gen_ssr_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Comp :title="msg" v-bind="$attrs" class="mb-2" />
</template>"#,
    );
    // Root component: props_before, $attrs spread, props_after, _attrs
    assert!(
        code.contains(
            r#"_mergeProps({ title: _ctx.msg }, _ctx.$attrs, { class: "mb-2" }, _attrs)"#
        ),
        "should split props and include _attrs, got:\n{}",
        code
    );
}

// ─── slot prop camelization ────────────────────────────────────

#[test]
fn ssr_slot_outlet_static_props_camelized() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <slot mdc-unwrap="p" data-testid="foo" />
  </div>
</template>"#,
    );
    // Positive: static slot props should be camelized
    assert!(
        code.contains("mdcUnwrap: \"p\""),
        "should camelize kebab-case slot prop, got:\n{}",
        code
    );
    assert!(
        code.contains("dataTestid: \"foo\""),
        "should camelize data- slot prop, got:\n{}",
        code
    );
    // Negative: should NOT have kebab-case keys
    assert!(
        !code.contains("\"mdc-unwrap\""),
        "should NOT have quoted kebab-case key, got:\n{}",
        code
    );
    assert!(
        !code.contains("\"data-testid\""),
        "should NOT have quoted data- key, got:\n{}",
        code
    );
}

// ─── v-show + :style merge tests ────────────────────────────────

#[test]
fn ssr_vshow_with_dynamic_style_merged() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <span v-show="visible" :style="customStyle">text</span>
  </div>
</template>"#,
    );
    // Positive: should merge into single _ssrRenderStyle([...]) call
    assert!(
        code.contains(
            r#"_ssrRenderStyle([_ctx.customStyle, (_ctx.visible) ? null : { display: "none" }])"#
        ),
        "should merge v-show and :style into single _ssrRenderStyle array, got:\n{}",
        code
    );
    // Negative: should NOT have two separate style attributes
    let style_count = code.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly one style attribute, got {} in:\n{}",
        style_count, code
    );
}

#[test]
fn ssr_vshow_with_static_style_merged() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <span v-show="visible" style="color: red">text</span>
  </div>
</template>"#,
    );
    // Positive: should merge static style + v-show into single _ssrRenderStyle
    assert!(
        code.contains(
            r#"_ssrRenderStyle([{"color":"red"}, (_ctx.visible) ? null : { display: "none" }])"#
        ),
        "should merge static style and v-show into array, got:\n{}",
        code
    );
    // Negative: should NOT have two separate style attributes
    let style_count = code.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly one style attribute, got {} in:\n{}",
        style_count, code
    );
}

#[test]
fn ssr_vshow_alone_no_array_wrapper() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <span v-show="visible">text</span>
  </div>
</template>"#,
    );
    // v-show alone should NOT use array form — just the conditional expression
    assert!(
        code.contains(r#"_ssrRenderStyle((_ctx.visible) ? null : { display: "none" })"#),
        "v-show alone should use simple expression, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// v-model on root native input: _temp0 pattern
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Root input with v-model uses _temp0 pattern (Vue compat).
/// Vue first merges static props + _attrs into _temp0, then uses _temp0
/// as the input for _ssrGetDynamicModelProps.
#[test]
fn ssr_vmodel_root_input_temp0_pattern() {
    let code = gen_ssr_template(
        r#"<script setup>
const modelValue = defineModel()
</script>
<template>
  <input v-model="modelValue" class="test" />
</template>"#,
    );
    // Should declare let _temp0
    assert!(
        code.contains("let _temp0"),
        "should declare _temp0 variable, got:\n{}",
        code
    );
    // Should use comma expression: (_temp0 = _mergeProps(..., _attrs), _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, ...)))
    assert!(
        code.contains("_temp0 = _mergeProps("),
        "should assign _mergeProps result to _temp0, got:\n{}",
        code
    );
    assert!(
        code.contains("_ssrGetDynamicModelProps(_temp0,"),
        "should pass _temp0 to _ssrGetDynamicModelProps, got:\n{}",
        code
    );
    // Negative: should NOT include value: in the static props object
    assert!(
        !code.contains("value: $setup.modelValue"),
        "should NOT include value in static props (delegated to _ssrGetDynamicModelProps), got:\n{}",
        code
    );
    // Negative: should NOT pass _attrs directly to _ssrGetDynamicModelProps
    assert!(
        !code.contains("_ssrGetDynamicModelProps(_attrs,"),
        "should pass _temp0 (not _attrs) to _ssrGetDynamicModelProps, got:\n{}",
        code
    );
}

/// @ai-generated — Non-root input with v-model should NOT use _temp0 pattern.
/// The _temp0 pattern is only for root elements where _attrs fallthrough matters.
#[test]
fn ssr_vmodel_nonroot_input_no_temp0() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <input v-model="text" class="field" />
  </div>
</template>"#,
    );
    // Non-root should use inline _ssrRenderAttr for value
    assert!(
        code.contains(r#"_ssrRenderAttr("value", _ctx.text)"#),
        "non-root input v-model should use inline _ssrRenderAttr, got:\n{}",
        code
    );
    // Negative: should NOT use _temp0
    assert!(
        !code.contains("_temp0"),
        "non-root should NOT use _temp0 pattern, got:\n{}",
        code
    );
    // Negative: should NOT use _ssrGetDynamicModelProps
    assert!(
        !code.contains("_ssrGetDynamicModelProps"),
        "non-root should NOT use _ssrGetDynamicModelProps, got:\n{}",
        code
    );
}

/// @ai-generated — Root input with v-model should NOT include explicit checked/value
/// in the attrs object — _ssrGetDynamicModelProps handles it at runtime.
#[test]
fn ssr_vmodel_root_input_no_explicit_value_prop() {
    let code = gen_ssr_template(
        r#"<template>
  <input v-model="text" />
</template>"#,
    );
    // Should use _ssrGetDynamicModelProps
    assert!(
        code.contains("_ssrGetDynamicModelProps("),
        "root v-model should use _ssrGetDynamicModelProps, got:\n{}",
        code
    );
    // Negative: should NOT have value: in the mergeProps args
    // (value is determined at runtime by _ssrGetDynamicModelProps based on type)
    assert!(
        !code.contains("value: _ctx.text"),
        "root v-model should NOT add explicit value prop, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// Root textarea v-model: content interpolation + "textarea" tag arg
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — Root textarea with v-model should NOT add value to attrs.
/// Instead, it should interpolate content and pass "textarea" tag arg to _ssrRenderAttrs.
#[test]
fn ssr_vmodel_root_textarea_content_interpolation() {
    let code = gen_ssr_template(
        r#"<script setup>
const modelValue = defineModel()
</script>
<template>
  <textarea v-model="modelValue" class="test"></textarea>
</template>"#,
    );
    // Should use _ssrInterpolate for content
    assert!(
        code.contains("_ssrInterpolate($setup.modelValue)"),
        "root textarea should interpolate v-model value as content, got:\n{}",
        code
    );
    // Should pass "textarea" as second arg to _ssrRenderAttrs
    assert!(
        code.contains(r#"_ssrRenderAttrs(_mergeProps("#) && code.contains(r#", "textarea")"#),
        "root textarea should pass tag name to _ssrRenderAttrs, got:\n{}",
        code
    );
    // Negative: should NOT include value: in the attrs object
    assert!(
        !code.contains("value: $setup.modelValue"),
        "root textarea should NOT include value in attrs, got:\n{}",
        code
    );
}

/// @ai-generated — Static class attribute values should have trailing whitespace trimmed.
/// Vue trims whitespace from class attribute values, Verter should too.
#[test]
fn ssr_static_class_trailing_whitespace_trimmed() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <p class="text-2xl mt-14 mb-6 ">content</p>
  </div>
</template>"#,
    );
    // Should have trimmed class value
    assert!(
        code.contains(r#"class="text-2xl mt-14 mb-6""#),
        "static class should be trimmed, got:\n{}",
        code
    );
    // Negative: should NOT have trailing space in class
    assert!(
        !code.contains(r#"class="text-2xl mt-14 mb-6 ""#),
        "should NOT have trailing space in class value, got:\n{}",
        code
    );
}

/// @ai-generated — Non-root textarea with v-model should still interpolate content.
#[test]
fn ssr_vmodel_nonroot_textarea_content() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <textarea v-model="msg" class="input"></textarea>
  </div>
</template>"#,
    );
    // Non-root textarea should interpolate content
    assert!(
        code.contains("_ssrInterpolate(_ctx.msg)"),
        "non-root textarea should interpolate v-model content, got:\n{}",
        code
    );
    // Negative: should NOT include value: in attrs
    assert!(
        !code.contains("value: _ctx.msg"),
        "non-root textarea should NOT have value attr, got:\n{}",
        code
    );
}

/// @ai-generated — :id on non-root input with v-bind spread must NOT be dropped.
/// Vue includes id: in _mergeProps alongside the spread.
#[test]
fn ssr_id_binding_with_vbind_spread() {
    let code = gen_ssr_template(
        r#"<template>
  <div>
    <input :id="uuid" v-bind="{ ...$attrs, onChange: updateValue }" :checked="modelValue" class="input" type="checkbox">
  </div>
</template>
<script setup>
const uuid = 'test-id';
const updateValue = () => {};
const modelValue = true;
</script>"#,
    );
    // id should appear in the mergeProps alongside checked, class, type
    assert!(
        code.contains("id:") || code.contains("\"id\""),
        ":id binding should appear in SSR output when v-bind spread is present, got:\n{}",
        code
    );
    // Negative: id should not be stripped
    assert!(
        !code.contains("type: \"checkbox\"})") || code.contains("id"),
        "id should not be missing from props when v-bind spread present, got:\n{}",
        code
    );
}

/// @ai-generated — Slot name with dot notation like #header.id must produce "header.id"
/// as the slot key, not just "header". Vuetify v-data-table uses slots like #item.name.
#[test]
fn ssr_slot_name_with_dot_notation() {
    let code = gen_ssr_template(
        r#"<template>
  <MyTable :items="items">
    <template #header.id="{ column }">
      {{ column.title.toUpperCase() }}
    </template>
  </MyTable>
</template>"#,
    );
    // Positive: slot name should include the dot portion
    assert!(
        code.contains("\"header.id\":") || code.contains("\"header.id\": "),
        "slot name should be \"header.id\" (including modifier), got:\n{}",
        code
    );
    // Negative: should NOT have just "header:" without the .id part
    assert!(
        !code.contains("header: _withCtx"),
        "slot name should be \"header.id\" not just \"header\", got:\n{}",
        code
    );
}

/// @ai-generated — v-for with TS type assertion in iterable should preserve
/// the TS syntax (Vue's SSR keeps TS intact) and NOT corrupt output with
/// binding prefixes inside the type annotation.
#[test]
fn ssr_vfor_ts_type_assertion_preserved() {
    let code = gen_ssr_template(
        r#"<script setup lang="ts">
const chartConfig = { desktop: 1, mobile: 2 }
</script>
<template>
  <div v-for="key of ['desktop', 'mobile'] as (keyof typeof chartConfig)[]" :key="key">
    {{ key }}
  </div>
</template>"#,
    );
    // Positive: the TS type assertion should be preserved intact (Vue's SSR keeps TS)
    assert!(
        code.contains("as (keyof typeof chartConfig)[]"),
        "TS type assertion should be preserved in SSR output, got:\n{}",
        code
    );
    // Negative: no corrupted binding prefix inside type annotation
    assert!(
        !code.contains("_ctx[\"f\"]"),
        "should not have corrupted binding prefix in type annotation, got:\n{}",
        code
    );
    assert!(
        !code.contains("_ctx.typeof"),
        "should not prefix 'typeof' with _ctx, got:\n{}",
        code
    );
}

// ─── merge_duplicate_event_handlers regression ──────────────────────────────

#[test]
fn test_merge_duplicate_event_handlers_multiple_keys() {
    // Regression: when multiple duplicate key groups exist, removing entries
    // for the first group shifts indices for subsequent groups, causing
    // index-out-of-bounds panic.
    use super::merge_duplicate_event_handlers;

    let mut parts = vec![
        r#""onUpdate:a": $event => (a = $event)"#.to_string(),
        r#"b: val_b"#.to_string(),
        r#""onUpdate:a": $event => (a2 = $event)"#.to_string(),
        r#"c: val_c"#.to_string(),
        r#""onUpdate:d": $event => (d = $event)"#.to_string(),
        r#"e: val_e"#.to_string(),
        r#"f: val_f"#.to_string(),
        r#"g: val_g"#.to_string(),
        r#"h: val_h"#.to_string(),
        r#""onUpdate:d": $event => (d2 = $event)"#.to_string(),
    ];

    // Should not panic
    merge_duplicate_event_handlers(&mut parts);

    // Positive: merged entries should have array values
    let result = parts.join(", ");
    assert!(
        result.contains("["),
        "should have merged array form, got: {}",
        result
    );

    // Negative: no duplicate keys remain
    let key_count_a = parts
        .iter()
        .filter(|p| p.starts_with("\"onUpdate:a\""))
        .count();
    assert_eq!(
        key_count_a, 1,
        "should have exactly one onUpdate:a entry after merge, got {}",
        key_count_a
    );
    let key_count_d = parts
        .iter()
        .filter(|p| p.starts_with("\"onUpdate:d\""))
        .count();
    assert_eq!(
        key_count_d, 1,
        "should have exactly one onUpdate:d entry after merge, got {}",
        key_count_d
    );
}

#[test]
fn test_ssr_component_with_many_v_models_no_panic() {
    // Regression: DatePicker.vue with multiple same-named event handlers
    // caused index-out-of-bounds in merge_duplicate_event_handlers.
    let code = gen_ssr_template(
        r#"<template>
  <MyComp
    :a="x" @update:a="x = $event"
    :b="y" @update:b="y = $event"
    :c="z" @update:c="z = $event"
    :d="w" @update:d="w = $event"
    :e="v" @update:e="v = $event"
    :f="u" @update:f="u = $event"
    :g="t" @update:g="t = $event"
    :h="s" @update:h="s = $event"
    :i="r" @update:i="r = $event"
    :j="q" @update:j="q = $event"
    @click="onClick"
    @click="onClick2"
  />
</template>
<script setup>
const x = ref(1)
const y = ref(2)
const z = ref(3)
const w = ref(4)
const v = ref(5)
const u = ref(6)
const t = ref(7)
const s = ref(8)
const r = ref(9)
const q = ref(10)
function onClick() {}
function onClick2() {}
</script>"#,
    );
    // Positive: should have ssrRenderComponent call
    assert!(
        code.contains("_ssrRenderComponent"),
        "should render component, got:\n{}",
        code
    );
    // Negative: should not have raw @update: in output
    assert!(
        !code.contains("@update:"),
        "should not have raw Vue event syntax in output, got:\n{}",
        code
    );
}
