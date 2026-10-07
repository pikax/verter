use super::*;

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
