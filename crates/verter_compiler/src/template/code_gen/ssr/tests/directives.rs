use super::*;

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
// Source-order attribute rendering (v-model)
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

/// @ai-generated — Custom directive from setup binding uses _ctx.vFocus (not free $setup).
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

// ══════════════════════════════════════════════════════════════════
// v-model checkbox/radio inline attrs
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

#[test]
fn test_ssr_scope_id_v_html() {
    let code = gen_ssr_template(
        r#"<template><div v-html="content"></div></template>
<script setup>
const content = '<b>bold</b>'
</script>
<style scoped>.foo { color: red; }</style>"#,
    );
    // Positive: v-html element should still get scope ID (after _ssrRenderAttrs)
    assert!(
        code.contains("data-v-"),
        "v-html element should have scope ID, got:\n{}",
        code
    );
}
