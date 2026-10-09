use super::*;

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

// Script SSR flags (non-inline: attached ssrRender, plain setup return)
//
// Official non-inline shape: 8-param `ssrRender` with `$setup.*`/`$props.*`
// routing, never a free `_ctx.*` alias for setup bindings. Matches
// `@vue/compiler-sfc` (`ssr:true` and `ssr:false` share a byte-identical
// script tail, `__isScriptSetup` included) and the pinned rc.5 SSR goldens.

/// Non-inline SSR must NOT set `__ssrInlineRender` (that flag means setup
/// returns the render function — true-inline SSR, which Verter doesn't do).
/// It MUST still mark the setup return with `__isScriptSetup`, matching
/// official exactly — see the module doc comment above for why the
/// earlier "must not" version of this assertion was wrong.
#[test]
fn ssr_script_non_inline_no_false_inline_flags() {
    let source =
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>";
    let script = gen_ssr_script(source);
    assert!(
        !script.contains("__ssrInlineRender"),
        "non-inline SSR must not claim __ssrInlineRender, got:\n{}",
        script
    );
    assert!(
        script.contains("__isScriptSetup"),
        "non-inline SSR must mark setup return with __isScriptSetup, matching \
         official exactly, got:\n{}",
        script
    );
    // Positive: still returns the bindings object
    assert!(
        script.contains("__returned__") || script.contains("return {"),
        "setup should still return bindings for the instance proxy, got:\n{}",
        script
    );
    // Negative: should not have __vapor
    assert!(
        !script.contains("__vapor"),
        "SSR script should not have __vapor, got:\n{}",
        script
    );
}

/// The `ssrRender` function body routes every setup binding through the
/// `$setup` POSITIONAL PARAMETER (`$setup.msg`), never a free `_ctx.*`
/// alias — official's real non-inline `ssrRender(_ctx, _push, _parent,
/// _attrs, $props, $setup, $data, $options)` shape, confirmed directly
/// against the real `@vue/compiler-sfc` and every pinned rc.5 SSR golden.
/// This is WHY the setup-return marker (see
/// `ssr_setup_return_carries_script_setup_marker` below) is safe to keep:
/// nothing here depends on `_ctx`'s instance-proxy exposure of setup keys.
#[test]
fn ssr_setup_bindings_route_through_setup_param() {
    let template = gen_ssr_template(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    assert!(
        template.contains("$setup.msg"),
        "setup bindings must route through the $setup parameter, got:\n{template}"
    );
    assert!(
        !template.contains("_ctx.msg"),
        "setup bindings must never route through a free _ctx.* alias, got:\n{template}"
    );
}

/// The setup return marker `__isScriptSetup` is present in non-inline SSR
/// script output — matching official exactly (confirmed directly against
/// the real `@vue/compiler-sfc`: `compileScript({ssr:true})` and
/// `{ssr:false}` produce a byte-identical script tail, marker included).
#[test]
fn ssr_setup_return_carries_script_setup_marker() {
    let script = gen_ssr_script(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    assert!(
        script.contains("Object.defineProperty(__returned__, '__isScriptSetup'"),
        "non-inline SSR setup return must carry the __isScriptSetup marker, \
         got:\n{script}"
    );
}

#[test]
fn ssr_template_only_no_false_inline_render_flag() {
    let source = "<template><div>hello</div></template>";
    let result = compile_sfc_ssr(source);
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script");
    assert!(
        !script.code.contains("__ssrInlineRender"),
        "template-only SSR must not claim __ssrInlineRender, got:\n{}",
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
        !script.code.contains("__ssrInlineRender"),
        "non-inline SSR must not claim __ssrInlineRender, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__isScriptSetup"),
        "non-inline SSR must mark setup return with __isScriptSetup, matching \
         official exactly, got:\n{}",
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
// Setup ref resolution for locally imported components
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
        code.contains("$setup[\"MyComp\"]"),
        "imported component should use $setup bracket ref, got:\n{}",
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
// $setup dot notation (SSR uses _ctx.x like VDOM)
// ══════════════════════════════════════════════════════════════════

/// @ai-generated — SSR setup binding uses $setup. (non-inline ssrRender).
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
}

/// @ai-generated — SSR multi-script: angle bracket type assertion in setup
/// still resolves the setup computed through $setup. prefix.
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
    assert!(
        code.contains("$setup.foo"),
        "should render foo with $setup prefix, got:\n{}",
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

/// @ai-generated — Data bindings from $setup use dot notation in SSR.
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
        "data binding should use $setup.msg, got:\n{}",
        code
    );
    // Negative: should not use bracket form for a plain identifier
    assert!(
        !code.contains(r#"_ctx["msg"]"#) && !code.contains(r#"$setup["msg"]"#),
        "must not emit bracket form for a plain identifier, got:\n{}",
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

/// @ai-generated — Options API `data()` properties use `$data.` in non-inline SSR
/// (`ssrRender` has an 8-param signature with `$data` when the SFC has a script block).
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
        "data binding should use $data. prefix in non-inline SSR, got:\n{}",
        code
    );
}

/// @ai-generated — Options API `computed` uses `$options.` in non-inline SSR
/// (`ssrRender` has an 8-param signature with `$options` when the SFC has a script block).
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
        "computed binding should use $options. prefix in non-inline SSR, got:\n{}",
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
    // <span${_ssrRenderAttr("id", $setup.id)}>shorthand</span>
    assert!(
        code.contains("$setup.id"),
        "should resolve shorthand :id to $setup.id, got:\n{}",
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
    // { class: "slider", type: "range", value: $setup.val }
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
// v-else-if robustness + single-child v-if fragment
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

// @ai-generated — v-if with single child component should not have extra fragment.
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
    // Vue: _createVNode("div", { onClick: _ctx.handler }, "click")
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

/// @ai-generated — VDOM fallback should use $setup. prefix for compound expressions.
/// Vue generates `_ctx.state.count` but Verter was generating bare `state.count`.
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
    // Should have proper function call syntax: Array.isArray(_ctx.model)
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
        vdom_part2.contains(r#"(_openBlock(), _createBlock($setup["Item"]"#),
        "keyed v-for component child should also use _createBlock, got:\n{}",
        vdom_part2
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
        code.contains(r#"_withKeys(_ctx.submit, ["enter"])"#)
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
        !code.contains("value: _ctx.modelValue"),
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

// ─── duplicate props-object key merging ─────────────────────────────────────

#[test]
fn duplicate_key_merge_handles_multiple_interleaved_groups() {
    // Two duplicate key groups, the second one's members sitting after the
    // first one's: collapsing the first group must not disturb the second.
    use super::super::props_object::PropsObject;

    let mut props = PropsObject::new();
    props.push("\"onUpdate:a\"", "$event => (a = $event)");
    props.push("b", "val_b");
    props.push("\"onUpdate:a\"", "$event => (a2 = $event)");
    props.push("c", "val_c");
    props.push("\"onUpdate:d\"", "$event => (d = $event)");
    props.push("e", "val_e");
    props.push("f", "val_f");
    props.push("\"onUpdate:d\"", "$event => (d2 = $event)");

    props.merge_duplicate_keys();

    // Both groups collapse in place, each keeping its first occurrence's
    // position and both values in authoring order.
    assert_eq!(
        props.render_body(),
        concat!(
            r#""onUpdate:a": [$event => (a = $event), $event => (a2 = $event)], "#,
            "b: val_b, c: val_c, ",
            r#""onUpdate:d": [$event => (d = $event), $event => (d2 = $event)], "#,
            "e: val_e, f: val_f"
        )
    );
}

#[test]
fn duplicate_key_merge_distinguishes_quoted_from_bare_keys() {
    // `"foo"` and `foo` are emitted as authored; merging them would rewrite a
    // key the author spelled two different ways into one entry.
    use super::super::props_object::PropsObject;

    let mut props = PropsObject::new();
    props.push("\"foo\"", "1");
    props.push("foo", "2");
    props.merge_duplicate_keys();

    assert_eq!(props.render_body(), r#""foo": 1, foo: 2"#);
}

#[test]
fn test_ssr_scope_id_multi_root() {
    let code = gen_ssr_template(
        r#"<template><div>a</div><span>b</span></template>
<style scoped>.foo { color: red; }</style>"#,
    );
    // Positive: both root elements should get scope ID
    let count = code.matches("data-v-").count();
    assert!(
        count >= 2,
        "both root elements should have scope ID (found {} occurrences), got:\n{}",
        count,
        code
    );
}

/// Drop-in SSR: when the SFC has a `<script setup>` block, non-inline
/// `ssrRender` declares the full 8-param signature `(_ctx, _push, _parent,
/// _attrs, $props, $setup, $data, $options)` — mirroring the VDOM
/// `render(_ctx, _cache, $props, $setup, $data, $options)` rule — and setup
/// bindings route through the declared `$setup` parameter rather than the
/// generic `_ctx` proxy. `$data`/`$options` stay declared-but-unused here
/// since this fixture has no Options-API bindings.
#[test]
fn ssr_non_inline_emits_no_free_setup_binding() {
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
import { ref } from 'vue'
import Child from './Child.vue'
const n = ref(0)
const props = defineProps<{ msg: string }>()
</script>
<template>
  <div>
    <h1>{{ n }}</h1>
    <Child :msg="props.msg" />
    <p v-if="n > 0">pos</p>
  </div>
</template>"#,
        runtime,
    );
    assert!(
        code.contains(
            "function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options)"
        ),
        "expected the 8-param non-inline ssrRender signature, got:\n{}",
        code
    );
    // Positive: setup ref + destructured props const resolve via the declared $setup param
    assert!(
        code.contains("$setup.n"),
        "setup ref must resolve via $setup.n, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.props.msg"),
        "props access must resolve via $setup.props.msg, got:\n{}",
        code
    );
    assert!(
        code.contains(r#"$setup["Child"]"#),
        "imported component must resolve via $setup[\"Child\"], got:\n{}",
        code
    );
    // Negative: $data/$options are declared params but never referenced in the
    // body — this fixture has no Options-API `data`/method bindings, so those
    // names must appear exactly once (the signature declaration itself).
    for unused in ["$data", "$options"] {
        let count = code.matches(unused).count();
        assert_eq!(
            count, 1,
            "ssrRender must not reference free {unused} beyond the signature declaration, got:\n{code}"
        );
    }
}

/// TransitionGroup `name` is forwarded onto the host tag in SSR.
#[test]
fn ssr_transition_group_forwards_name_attr() {
    let code = gen_ssr_template(
        r#"<script setup></script>
<template>
  <TransitionGroup name="list" tag="ul">
    <li v-for="i in 2" :key="i">{{ i }}</li>
  </TransitionGroup>
</template>"#,
    );
    assert!(
        code.contains("name=\"list\"") || code.contains(" name=\\\"list\\\""),
        "TransitionGroup name must appear on host tag, got:\n{code}"
    );
}
