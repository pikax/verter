use super::*;

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

// Empty named slots can coexist with non-empty default content.
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

/// F17: a NESTED `<KeepAlive>` must force block topology, raw array children,
/// and carry the `1024 /* DYNAMIC_SLOTS */` patch flag (official Vue).
#[test]
fn keepalive_nested_forces_block_array_and_dynamic_slots() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template><div><KeepAlive><Comp/></KeepAlive></div></template>"#,
    );
    assert!(
        code.contains("(_openBlock(), _createBlock(_KeepAlive"),
        "nested KeepAlive must force block topology.\n{code}"
    );
    assert!(
        code.contains("1024 /* DYNAMIC_SLOTS */"),
        "KeepAlive must carry the DYNAMIC_SLOTS (1024) patch flag.\n{code}"
    );
    // NEGATIVE: never a non-block _createVNode, never a slot object.
    assert!(
        !code.contains("_createVNode(_KeepAlive"),
        "nested KeepAlive must NOT be a non-block _createVNode.\n{code}"
    );
    assert!(
        !code.contains("default: _withCtx") && !code.contains("{default:"),
        "KeepAlive children must be a raw VNode array, not a slot object.\n{code}"
    );
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

/// A `<slot>` outlet's own static props (e.g. `name="header"`) must NOT be
/// pre-order-reserved as a `_hoisted_N` — `leave_element` routes slot
/// outlets to `process_slot_outlet`, an entirely separate function that
/// builds its own `_renderSlot(...)` call and never consults a hoist
/// reservation. Reserving one anyway orphans it: declared, pushed into the
/// preamble, and never referenced anywhere in the render body — exactly
/// the regression this test caught during development of the ancestor/
/// descendant hoist-ordering fix, on `slots.vue`'s `<slot name="header">`.
#[test]
fn static_hoist_slot_outlet_props_are_never_reserved() {
    let code = compile_and_validate_hoisted(
        r#"<template><div class="panel"><header><slot name="header">Untitled</slot></header><main><slot /></main></div></template>"#,
    );
    assert!(
        code.contains(r#"const _hoisted_1 = { class: "panel" }"#),
        "the root's own class object must still hoist to _hoisted_1, got:\n{}",
        code
    );
    assert!(
        !code.contains("_hoisted_2"),
        "the slot outlet's `name` prop must NOT produce an orphaned _hoisted_2 \
         (renderSlot builds its own props argument, never a hoisted const), got:\n{}",
        code
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

    // Should have clean frame body without JSX expression wrapping
    assert!(
        tsx.code.contains("); return (___VERTER___instance.$slots"),
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
        // Pin the standalone template lane — this test targets template
        // codegen details, not the (inline) production default topology.
        inline: Some(false),
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
        &crate::compile::VueMacroSemanticInput::Unavailable,
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
        &crate::compile::VueMacroSemanticInput::Unavailable,
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
        // Pin the standalone template lane — this test targets template
        // codegen details, not the (inline) production default topology.
        inline: Some(false),
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
        &crate::compile::VueMacroSemanticInput::Unavailable,
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

#[test]
fn strict_slots_nested_output_is_byte_identical() {
    // Nested components with named (#header, scoped) + default slots. The code
    // and map golden jointly pin strict-slot helpers plus the isolated typed
    // element pairs used by the IDE projection.
    let result = compile_tsx_strict_slots(SLOT_SFC_NESTED);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("../ide/template/fixtures/strict_slots_nested.tsx.golden"),
        include_str!("../ide/template/fixtures/strict_slots_nested.tsx.map.golden"),
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
    // and isolated typed pairs reach arbitrarily deep components.
    let result = compile_tsx_strict_slots(SLOT_SFC_DEEP);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("../ide/template/fixtures/strict_slots_deep.tsx.golden"),
        include_str!("../ide/template/fixtures/strict_slots_deep.tsx.map.golden"),
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
    // slot check is emitted. The golden still pins its isolated DOM pairs and
    // exact source map.
    let result = compile_tsx_strict_slots(SLOT_SFC_NO_SLOT);
    let tsx = result.tsx.as_ref().expect("tsx output");
    assert_tsx_code_and_map_match(
        tsx,
        include_str!("../ide/template/fixtures/strict_slots_no_slot.tsx.golden"),
        include_str!("../ide/template/fixtures/strict_slots_no_slot.tsx.map.golden"),
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

#[test]
fn inline_template_using_slots_destructures_slots() {
    let result = compile_sfc_inline(
        "<script setup>\nconst x = 1\n</script>\n<template><div :class=\"$slots.default ? 'y' : 'n'\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("setup(__props, { slots: $slots })"),
        "inline setup must destructure slots on template use, got:\n{}",
        code
    );
    assert!(
        code.contains("$slots.default") && !code.contains("_ctx.$slots.default"),
        "template $slots references resolve to the destructured binding, got:\n{}",
        code
    );
}

#[test]
fn inline_plain_template_no_attrs_slots_destructure() {
    // On-use condition: no $attrs/$slots usage → no destructure (official).
    let result = compile_sfc_inline(
        "<script setup>\nconst x = 1\n</script>\n<template><div>{{ x }}</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("setup(__props)"),
        "no attrs/slots destructure without template use, got:\n{}",
        code
    );
    assert!(
        !code.contains("attrs: $attrs") && !code.contains("slots: $slots"),
        "got:\n{}",
        code
    );
}
