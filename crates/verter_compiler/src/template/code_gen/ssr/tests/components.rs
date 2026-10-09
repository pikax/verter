use super::*;

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
// Component variable naming: kebab-case → underscore
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
// Event handler props on components
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

/// @ai-generated — SSR props use $props. (non-inline ssrRender).
#[test]
fn ssr_props_dot_notation() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "msg",
            false,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let code = gen_ssr_template_with_runtime(
        r#"<script setup>
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        runtime,
    );
    assert!(
        code.contains("$props.msg"),
        "SSR props should use dot notation, got:\n{}",
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
    // Negative: should not fall back to _ctx for a setup-declared component
    assert!(
        !code.contains("_ctx.MyComp"),
        "must not emit _ctx.MyComp for a setup-declared component ref in ssrRender, got:\n{}",
        code
    );
}

// ══════════════════════════════════════════════════════════════════
// v-model on components
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
        code.contains("modelValue: $setup.val"),
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
        code.contains("title: $setup.title"),
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
// `ref` in component/root props
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

/// @ai-generated — Options API `props` use `$props.` in non-inline SSR
/// (`ssrRender` has an 8-param signature with `$props` when the SFC has a script block).
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
        "props binding should use $props. prefix in SSR, got:\n{}",
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
        !code.contains("_createVNode(_resolveDynamicComponent(_ctx.comp), null"),
        "root dynamic component should not have null props, got:\n{}",
        code
    );
}

// ── Vue 3.4 same-name v-bind shorthand on COMPONENTS ─────────────
// `:foo` ≡ `:foo="foo"`. Dropping these is a runtime prop-missing bug.

/// Minimal: setup const + `:cards` alone must emit `cards: _ctx.cards`.
#[test]
fn ssr_component_same_name_shorthand_setup_const() {
    let code = gen_ssr_template(
        r#"<script setup>
import Child from './Child.vue'
const cards = [1, 2]
</script>
<template><Child :cards /></template>"#,
    );
    assert!(
        code.contains("cards: _ctx.cards") || code.contains("cards: $setup.cards"),
        "`:cards` shorthand must emit cards binding, got:\n{code}"
    );
    // Negative: must not pass only _attrs (would drop the prop)
    assert!(
        !code.contains("_ssrRenderComponent(_ctx.Child, _attrs, null")
            && !code.contains("_ssrRenderComponent($setup[\"Child\"], _attrs, null"),
        "must not collapse to bare _attrs when :cards is present, got:\n{code}"
    );
}

/// `:cards` must still appear when combined with `v-bind="props"` (spread).
#[test]
fn ssr_component_same_name_shorthand_with_vbind_spread() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "x",
            true,
            [verter_macro_dto::RuntimeConstructor::Number],
        )],
    )]);
    let code = gen_ssr_template_with_runtime(
        r#"<script setup>
import Child from './Child.vue'
const props = defineProps<{ x?: number }>()
const cards = [1]
</script>
<template><Child :cards v-bind="props" /></template>"#,
        runtime,
    );
    assert!(
        code.contains("cards: _ctx.cards") || code.contains("cards:"),
        "`:cards` must survive next to v-bind spread, got:\n{code}"
    );
    // Spread must still be present
    assert!(
        code.contains("_ctx.props") || code.contains("props"),
        "v-bind=\"props\" spread must remain, got:\n{code}"
    );
}

/// Destructuring defineProps + `:invert` / `:brightness` same-name shorthands.
#[test]
fn ssr_component_same_name_shorthand_destructured_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [
            crate::test_helpers::runtime_prop(
                "invert",
                true,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop(
                "brightness",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let code = gen_ssr_template_with_runtime(
        r#"<script setup lang="ts">
import Child from './Child.vue'
const { invert = false, brightness = 0 } = defineProps<{
  invert?: boolean
  brightness?: number
}>()
</script>
<template><Child :invert :brightness /></template>"#,
        runtime,
    );
    // Destructured props resolve through $props./bindings — pin the
    // resolved VALUE expression, not key presence: a regression emitting a
    // bare `invert` (free identifier → ReferenceError in non-inline
    // ssrRender) must fail this test.
    assert!(
        code.contains("invert: $props.invert") && code.contains("brightness: $props.brightness"),
        "destructured prop shorthands must resolve values through $props., got:\n{code}"
    );
    assert!(
        !code.contains("invert: invert") && !code.contains("brightness: brightness"),
        "shorthand values must never emit bare identifiers, got:\n{code}"
    );
}

/// Source-order: explicit keys stay in template order with shorthand.
#[test]
fn ssr_component_same_name_shorthand_preserves_source_order() {
    let code = gen_ssr_template(
        r#"<script setup>
import Child from './Child.vue'
const a = 1
const b = 2
const c = 3
</script>
<template><Child :a :b="b" :c /></template>"#,
    );
    let ia = code.find("a:").expect("a key");
    let ib = code.find("b:").expect("b key");
    let ic = code.find("c:").expect("c key");
    assert!(
        ia < ib && ib < ic,
        "prop keys must follow template declaration order a,b,c; got positions a={ia} b={ib} c={ic} in:\n{code}"
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
    // Vue generates: _createVNode(Child, { store: _ctx.store }, null, 8 /* PROPS */, ["store"])
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
    // constVal is literal-const → Vue: _createVNode(Child, { title: _ctx.constVal })
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
        !vdom_part.contains("_createVNode(_ctx.Upload, null,"),
        "props should NOT be null when ref is present, got:\n{}",
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
        code.contains(r#"$setup["vFoo"]"#),
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
    // Tag <el-icon> with camelCase binding `elIcon` → should resolve to _ctx.elIcon
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
    // Tag <el-icon> with PascalCase binding `ElIcon` → should resolve to _ctx.ElIcon
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

#[test]
fn test_ssr_component_with_many_v_models_no_panic() {
    // Regression: a component carrying many v-model/@update pairs plus two
    // handlers for the same event exercises several duplicate-key groups at
    // once; collapsing one group must not disturb the others.
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

#[test]
fn test_ssr_scope_id_component() {
    let code = gen_ssr_template(
        r#"<template><MyComp /></template>
<script setup>
import MyComp from './MyComp.vue'
</script>
<style scoped>.foo { color: red; }</style>"#,
    );
    // Positive: should pass scope ID as string arg to _ssrRenderComponent
    assert!(
        code.contains(", \"data-v-"),
        "should pass scope ID string to _ssrRenderComponent, got:\n{}",
        code
    );
    // Negative: should NOT use runtime _scopeId variable
    assert!(
        !code.contains(", _scopeId"),
        "should not pass _scopeId variable, got:\n{}",
        code
    );
}

/// A ROOT COMPONENT must receive `_cssVars` in its props — official
/// `ssrInjectCssVars` injects on every root-level node, components
/// included. Before the fix `const _cssVars` was emitted but never used
/// (all custom properties lost).
#[test]
fn ssr_css_vbind_reaches_root_component_props() {
    let code = gen_ssr_template(
        r#"<script setup>
import Child from './Child.vue'
import { ref } from 'vue'
const color = ref('green')
</script>
<template><Child /></template>
<style scoped>
.vb { color: v-bind(color) }
</style>"#,
    );
    assert!(
        code.contains("const _cssVars"),
        "must define _cssVars, got:\n{code}"
    );
    assert!(
        code.contains("_mergeProps(") && code.contains(", _cssVars)"),
        "root component props must merge _cssVars, got:\n{code}"
    );
}

/// A string literal in a bound attribute is opaque to property assembly: it is
/// carried through to emission verbatim, whatever it spells, while the class
/// and style parts around it still merge.
#[test]
fn ssr_string_literal_prop_value_survives_class_and_style_merge() {
    let code = gen_ssr_template(
        r#"<script setup>
const cls = 'x'
</script>
<template><div :class="cls" :data-note="'plain text'" style="color:red" :style="{ color: 'blue' }">t</div></template>"#,
    );
    assert!(
        code.contains("'plain text'"),
        "the user's literal must survive verbatim, got:\n{code}"
    );
    assert!(
        code.contains(r#"class: $setup.cls"#),
        "the dynamic class must still emit, got:\n{code}"
    );
    assert!(
        code.contains(r#"style: [{"color":"red"}, { color: 'blue' }]"#),
        "static and dynamic style must merge in source order, got:\n{code}"
    );
}
