use super::*;

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

// ══════════════════════════════════════════════════════════════════
// static style as JS object in mergeProps
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

/// @ai-generated — Same-name shorthand `:class` (Vue 3.4+) resolves to arg name as expression.
#[test]
fn ssr_same_name_shorthand_class() {
    // Non-root element so :class uses inline _ssrRenderClass path.
    // Use script setup so `class` ref resolves to _ctx.class.
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

#[test]
fn test_ssr_no_scope_id_without_scoped_style() {
    let code = gen_ssr_template(
        r#"<template><div class="foo">hello</div></template>
<style>.foo { color: red; }</style>"#,
    );
    // Negative: without <style scoped>, no scope ID should appear
    assert!(
        !code.contains("data-v-"),
        "should NOT have scope ID without scoped style, got:\n{}",
        code
    );
}

/// Static + dynamic style on the root must merge into `style: [static, dynamic]`.
#[test]
fn ssr_root_static_and_dynamic_style_merge_array() {
    let code = gen_ssr_template(
        r#"<script setup>
const sty = { color: "red" }
</script>
<template>
  <div style="margin:1px" :style="sty">m</div>
</template>"#,
    );
    assert!(
        code.contains("style: [") && code.contains("margin"),
        "static+dynamic style must merge into array, got:\n{code}"
    );
    // Negative: duplicate style keys (last-wins bug)
    let style_keys = code.matches("style:").count();
    assert!(
        style_keys <= 2, // one in object is fine; not two sibling keys
        "should not emit duplicate style keys (found {style_keys}), got:\n{code}"
    );
}

/// SSR must inject `_cssVars` for style v-bind() so custom properties reach HTML.
#[test]
fn ssr_css_vbind_injects_css_vars() {
    let code = gen_ssr_template(
        r#"<script setup>
import { ref } from 'vue'
const color = ref('green')
</script>
<template><div class="vb">vbind</div></template>
<style scoped>
.vb { color: v-bind(color) }
</style>"#,
    );
    assert!(
        code.contains("_cssVars"),
        "ssrRender must define _cssVars for v-bind CSS, got:\n{code}"
    );
    assert!(
        code.contains("_cssVars")
            && (code.contains("mergeProps") || code.contains("_ssrRenderAttrs")),
        "root attrs must merge _cssVars, got:\n{code}"
    );
    assert!(
        code.contains("_ctx.color") || code.contains("color"),
        "css var expression must reference the binding, got:\n{code}"
    );
}

/// NON-ROOT static + dynamic style must merge into ONE `style` attribute —
/// official merges style parts for every element; two `style=` attributes
/// silently drop one (first wins in browsers).
#[test]
fn ssr_nested_static_plus_dynamic_style_merges_single_attribute() {
    let code = gen_ssr_template(
        r#"<script setup>
const x = { color: 'blue' }
</script>
<template><div><span style="color:red" :style="x">t</span></div></template>"#,
    );
    let style_count = code.matches(" style=").count();
    assert_eq!(
        style_count, 1,
        "static + dynamic style must merge into exactly ONE style attribute, got:\n{code}"
    );
    assert!(
        code.contains("_ssrRenderStyle(["),
        "merged style must render the array form, got:\n{code}"
    );
    // SOURCE order: static first (it appears first in the template).
    let merged_start = code.find("_ssrRenderStyle([").expect("merged style");
    let merged = &code[merged_start..];
    let static_pos = merged.find("color").expect("static style part");
    let dyn_pos = merged.find("$setup.x").expect("dynamic style part");
    assert!(
        static_pos < dyn_pos,
        "authoring order: static style precedes :style, got:\n{code}"
    );
}

/// Reversed authoring order (`:style` before static) keeps source order in
/// the merged array.
#[test]
fn ssr_nested_dynamic_then_static_style_keeps_source_order() {
    let code = gen_ssr_template(
        r#"<script setup>
const x = { color: 'blue' }
</script>
<template><div><span :style="x" style="color:red">t</span></div></template>"#,
    );
    assert_eq!(code.matches(" style=").count(), 1);
    let merged_start = code.find("_ssrRenderStyle([").expect("merged style");
    let merged = &code[merged_start..];
    let dyn_pos = merged.find("$setup.x").expect("dynamic style part");
    let static_pos = merged.find("color").expect("static style part");
    assert!(
        dyn_pos < static_pos,
        ":style authored first must come first in the merged array, got:\n{code}"
    );
}

/// MULTI-ROOT templates carry `_cssVars` on EACH root-level element
/// (official injects per root node; `_attrs` fallthrough stays
/// single-root-only and must NOT appear).
#[test]
fn ssr_css_vbind_reaches_each_multi_root_element() {
    let code = gen_ssr_template(
        r#"<script setup>
import { ref } from 'vue'
const color = ref('green')
</script>
<template>
  <div class="a">one</div>
  <div class="b">two</div>
</template>
<style scoped>
.a { color: v-bind(color) }
</style>"#,
    );
    let uses = code.matches("_cssVars.style").count();
    assert!(
        uses >= 2,
        "each multi-root element must carry the css vars (expected 2 uses, got {uses}):\n{code}"
    );
    assert!(
        !code.contains("_attrs)") || !code.contains("_mergeProps(_attrs"),
        "multi-root must not gain _attrs fallthrough from css vars, got:\n{code}"
    );
}

#[test]
fn root_dynamic_class_before_static_class_emits_one_class_key() {
    // `:class` ahead of `class` reserves the merged entry's position first;
    // the later static `class` must reuse that position rather than open a
    // second one. Two `class` keys in one object literal is invalid Vue props
    // output — the second silently wins at runtime.
    let code = gen_ssr_template(
        r#"<template><div :class="a" class="b" id="x"></div></template>
<script setup>const a = 1</script>"#,
    );
    assert_eq!(
        code.matches("class: ").count(),
        1,
        "exactly one class entry expected, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"{ class: ["b", $setup.a], id: "x" }"#),
        "merged class keeps the first class-ish prop's position, got:\n{}",
        code
    );
}

#[test]
fn spread_element_dynamic_class_before_static_class_emits_one_class_key() {
    // Same reservation rule when a `v-bind` spread precedes both class props,
    // so the attrs object is no longer the first `_mergeProps` argument.
    let code = gen_ssr_template(
        r#"<template><div v-bind="o" :class="a" class="b"></div></template>
<script setup>const a = 1; const o = {}</script>"#,
    );
    assert_eq!(
        code.matches("class: ").count(),
        1,
        "exactly one class entry expected, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"_mergeProps($setup.o, { class: ["b", $setup.a] }, _attrs)"#),
        "spread stays first and the merged class is the only attrs entry, got:\n{}",
        code
    );
}

#[test]
fn root_static_class_key_maps_back_to_the_authored_class_attribute() {
    // The attrs object's `class` key is one of the few generated keys carrying
    // a source anchor: an IDE landing on `class` in the emitted `_mergeProps`
    // object must resolve to the authored `class=` attribute name. The offset
    // is derived from the entry's own position inside the rendered object, so
    // an off-by-N here silently mis-maps rather than failing to compile.
    let source = r#"<template><div id="x" class="b"></div></template>"#;
    let alloc = Allocator::new();
    let result = compile(
        source,
        &CodegenOptions {
            filename: Some("App.vue".to_string()),
            ..Default::default()
        },
        &VerterCompileOptions {
            force_js: true,
            ssr: true,
            source_map: true,
            ..Default::default()
        },
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");

    let key_offset = tpl
        .code
        .find("class: ")
        .expect("the attrs object carries a class key");
    let (gen_line, gen_col) = ssr_map_byte_offset_to_line_col(&tpl.code, key_offset);

    let authored = source.find("class=").expect("authored class attribute");
    let expected = ssr_map_byte_offset_to_line_col(source, authored);

    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(&tpl.source_map)
        .expect("valid source-map JSON");
    let lookup = map.generate_lookup_table();
    let token = map
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("a token at or before the class key");
    assert_eq!(
        (token.get_dst_line(), token.get_dst_col()),
        (gen_line, gen_col),
        "the class key must own its own mapping, not inherit an earlier token's, got:\n{}",
        tpl.code
    );
    assert_eq!(
        (token.get_src_line(), token.get_src_col()),
        expected,
        "the class key must map to the authored class attribute name, got:\n{}",
        tpl.code
    );
}
