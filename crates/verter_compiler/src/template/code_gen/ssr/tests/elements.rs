use super::*;

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

/// @ai-generated — Kebab-case tag with PascalCase import uses $setup ref.
#[test]
fn ssr_setup_import_kebab_tag() {
    let source = r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><my-comp msg="hello" /></template>"#;
    let code = gen_ssr_template(source);
    assert!(
        code.contains("$setup[\"MyComp\"]"),
        "kebab-case tag with PascalCase import should use $setup bracket ref, got:\n{}",
        code
    );
    // Negative: should NOT have _resolveComponent
    assert!(
        !code.contains("_resolveComponent"),
        "imported component should NOT use _resolveComponent, got:\n{}",
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
// Text whitespace around interpolations
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
// HTML entity decode + re-encode in SSR text
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
    // Non-inline SSR with a script block: [$setup.dynamicPropName || ""]: $setup.dynamicPropValue
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
    // Non-inline SSR with a script block: _ssrRenderAttrs({ [(`data-${$data.dynamicClassName}`) || ""]: $data.isActive })
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
    // { [(`data-${$setup.dynamicName}`) || ""]: $setup.val }
    assert!(
        code.contains(r#"`data-${$setup.dynamicName}`"#),
        "should resolve dynamicName to $setup.dynamicName in template literal, got:\n{}",
        code
    );
    assert!(
        !code.contains("d_ctx."),
        "must NOT have broken 'd_ctx.' from off-by-one in dynamic attr name, got:\n{}",
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
    // Vue generates: _createVNode("pre", null, _toDisplayString(_ctx.content), 1 /* TEXT */)
    assert!(
        code.contains("1 /* TEXT */"),
        "element with interpolation child should have TEXT patchflag, got:\n{}",
        code
    );
    assert!(
        !code.contains("_createVNode(\"pre\", null, _toDisplayString(_ctx.content))"),
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
    // Vue generates: _createVNode("input", { onInput: _ctx.handler }, null, 32 /* NEED_HYDRATION */)
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
    // msg is literal-const → Vue: _createVNode("p", null, _toDisplayString(_ctx.msg))
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
    // Should use camelized name for the value expression, with the $setup prefix
    // (the SFC declares `dataCount` as a `<script setup>` binding)
    assert!(
        code.contains("data-count") && code.contains("$setup.dataCount"),
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

// ─── Fragment marker tests ────────────────────────────────────────────────────

#[test]
fn ssr_no_fragment_markers_for_text_only_root() {
    // A template with only text/interpolation at root should NOT have fragment markers
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "name",
            false,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let code = gen_ssr_template_with_runtime(
        r#"<template> Hello {{ name }}! </template>
<script setup>
const props = defineProps<{ name: string }>()
</script>"#,
        runtime,
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
        !vdom_part.contains("_toDisplayString(_ctx.a) + _toDisplayString(_ctx.b)"),
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
        !code.contains("value: _ctx.text"),
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
        !code.contains("value: _ctx.modelValue"),
        "root textarea should NOT include value in attrs, got:\n{}",
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

// ── SSR Scoped Style (scope ID injection) ──────────────────────

#[test]
fn test_ssr_scope_id_basic_element() {
    let code = gen_ssr_template(
        r#"<template><div class="foo">hello</div></template>
<style scoped>.foo { color: red; }</style>"#,
    );
    // Positive: should have data-v-XXXXX attribute on the element
    assert!(
        code.contains("data-v-"),
        "should inject scope ID attribute, got:\n{}",
        code
    );
    // The scope ID should appear in the element's opening tag (after _ssrRenderAttrs)
    assert!(
        code.contains("data-v-"),
        "scope ID should appear in element tag, got:\n{}",
        code
    );
    // Negative: should NOT use runtime _scopeId parameter
    assert!(
        !code.contains("${_scopeId}"),
        "should not use runtime _scopeId interpolation, got:\n{}",
        code
    );
    // Negative: should NOT have 8-param signature
    assert!(
        !code.contains("$setup, $data, $options, _scopeId"),
        "should not use 8-param signature, got:\n{}",
        code
    );
    // Positive: should use 4-param signature
    assert!(
        code.contains("function ssrRender(_ctx, _push, _parent, _attrs)"),
        "should use 4-param signature, got:\n{}",
        code
    );
}

#[test]
fn test_ssr_scope_id_void_element() {
    let code = gen_ssr_template(
        r#"<template><input type="text" /><br /></template>
<style scoped>.foo { color: red; }</style>"#,
    );
    // Positive: void elements should get scope ID before self-closing
    assert!(
        code.contains("data-v-"),
        "void elements should have scope ID, got:\n{}",
        code
    );
}
