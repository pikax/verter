use super::*;

// ══════════════════════════════════════════════════════════════════
// Component slot content (_withCtx dual-branch wrappers)
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
// <slot> outlet rendering (_ssrRenderSlot)
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
        code.contains("_ctx.field"),
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
        code.contains("_ctx.field"),
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

/// @ai-generated — Same-name shorthand in slot props should be included.
#[test]
fn ssr_slot_same_name_shorthand_props() {
    let code = gen_ssr_template(
        r#"<template><div><slot name="header" :items :count="items.length" /></div></template>
<script setup>
const items = ref([])
</script>"#,
    );
    // _ssrRenderSlot(_ctx.$slots, "header", {count: ..., items: $setup.items}, ...)
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
    // The slot prop value should use the camelized name with the $props prefix
    // (the SFC declares `headingValue` through Options API `props: [...]`)
    assert!(
        code.contains("headingValue: $props.headingValue"),
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

#[test]
fn reserved_slot_never_filled_renders_nothing() {
    // `class`/`style` reserve a position before their merged value is known;
    // an abandoned reservation must not leave a stray separator behind.
    use super::super::props_object::PropsObject;

    let mut props = PropsObject::new();
    props.push("a", "1");
    let _abandoned = props.reserve();
    props.push("b", "2");

    assert_eq!(props.render_body(), "a: 1, b: 2");
}

/// Dynamic `#[name]` slots must use a computed property key (`[_ctx.name]`),
/// not a quoted literal `"[name]"` (which names a slot literally "[name]").
#[test]
fn ssr_dynamic_slot_name_computed_key() {
    let code = gen_ssr_template(
        r#"<script setup>
import Slotty from './Slotty.vue'
const name = "header"
</script>
<template>
  <Slotty>
    <template #[name]><b>dyn</b></template>
    body
  </Slotty>
</template>"#,
    );
    assert!(
        code.contains("[$setup.name]"),
        "dynamic slot name must be a computed key, got:\n{code}"
    );
    assert!(
        !code.contains("\"[name]\"") && !code.contains("\"[$setup.name]\""),
        "must not quote the brackets into a literal slot name, got:\n{code}"
    );
    // Positive: still emits the slot body
    assert!(code.contains("dyn"), "slot body missing, got:\n{code}");
}

/// Dynamic slot name with a COMPOUND expression (`#[tab.key]`): the root
/// binding must resolve (`_ctx.tab.key`), never pass through as a free
/// identifier (ReferenceError in non-inline ssrRender).
#[test]
fn ssr_dynamic_slot_name_compound_expression_resolves_root() {
    let code = gen_ssr_template(
        r#"<script setup>
import Child from './Child.vue'
const tab = { key: 'header' }
</script>
<template><Child><template #[tab.key]>content</template></Child></template>"#,
    );
    assert!(
        code.contains("[$setup.tab.key]"),
        "compound dynamic slot name must resolve its root binding, got:\n{code}"
    );
    assert!(
        !code.contains("[tab.key]:") && !code.contains("[tab.key] "),
        "compound dynamic slot name must not pass through unresolved, got:\n{code}"
    );
}

/// Dynamic slot name referencing a v-for ALIAS stays bare (the alias is in
/// scope inside the renderList callback; `_ctx.name` would read undefined
/// and key the slot \"undefined\").
#[test]
fn ssr_dynamic_slot_name_vfor_alias_stays_bare() {
    let code = gen_ssr_template(
        r#"<script setup>
import Child from './Child.vue'
const tabs = ['a', 'b']
</script>
<template>
  <Child>
    <template v-for="name in tabs" #[name]>content</template>
  </Child>
</template>"#,
    );
    assert!(
        !code.contains("[_ctx.name]"),
        "v-for alias in dynamic slot name must NOT be _ctx-prefixed, got:\n{code}"
    );
    assert!(
        code.contains("[name]"),
        "v-for alias must stay bare in the computed slot key, got:\n{code}"
    );
}
