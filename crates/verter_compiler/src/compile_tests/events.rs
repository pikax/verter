use super::*;

// ==================== v-if / v-else-if / v-else ====================

#[test]
pub(super) fn v_if_only_emits_comment_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="show">yes</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "v-if without v-else should emit comment fallback\n{}",
        code
    );
}

#[test]
pub(super) fn v_if_v_else_if_no_v_else_emits_comment_fallback() {
    let code = compile_and_validate_template(
        r#"<template><div><span v-if="a">A</span><span v-else-if="b">B</span></div></template>"#,
    );
    assert!(
        code.contains("_createCommentVNode"),
        "v-if/v-else-if without v-else should emit comment fallback\n{}",
        code
    );
}

// ==================== v-if / whitespace overlap TDD tests ====================

#[test]
pub(super) fn v_if_standalone_emits_comment_vnode() {
    // Standalone v-if without v-else should produce a comment fallback
    let code =
        compile_and_validate_template("<template><div><div v-if=\"show\">A</div></div></template>");
    assert!(
        code.contains("_createCommentVNode(\"v-if\", true)"),
        "Standalone v-if should emit comment vnode\n{}",
        code
    );
}

// ==================== Multi-statement event handlers ====================

/// A multi-statement handler must be wrapped in `$event => { … }` AND every
/// statement in the list must resolve against the setup bindings.
///
/// The `<script setup>` block is load-bearing. A binding-less fixture gives the
/// compiler nothing to prefix, so a wrapping-only assertion is structurally
/// incapable of observing whether the statements after the first `;` were
/// resolved or copied through verbatim — which is exactly how a four-month-old
/// truncating parse survived under this test's name.
#[test]
fn multi_statement_event_handler_wrapped_and_every_statement_resolved() {
    let code = compile_and_validate_template(
        r#"<script setup>
const emit = defineEmits(['x'])
function doStuff() {}
</script>
<template><button @click="emit('x'); doStuff();">go</button></template>"#,
    );
    assert!(
        code.contains("$event => {"),
        "Multi-statement handler should be wrapped in $event => {{ ... }}\n{}",
        code
    );
    assert!(
        code.contains("$event => {$setup.emit('x'); $setup.doStuff();}"),
        "every statement in the list must resolve, not just the first, got:\n{}",
        code
    );
    // Negative: the truncating parse resolved statement 1 and copied statement 2
    // through verbatim. `doStuff` is a setup-scope binding, so an unprefixed call
    // reads an undeclared global and fails silently at runtime.
    assert!(
        !code.contains("; doStuff()"),
        "the unresolved second statement must not survive, got:\n{}",
        code
    );
}

#[test]
fn assignment_event_handler_wrapped() {
    // Assignment expressions in event handlers need $event => { ... } wrapping
    // to be valid as object literal values.
    let code = compile_and_validate_template(
        r#"<template><button @click="dialog = true">link</button></template>"#,
    );
    assert!(
        code.contains("$event => {"),
        "Assignment handler should be wrapped in $event => {{ ... }}\n{}",
        code
    );
}

#[test]
fn assignment_event_handler_with_modifiers_and_hash_href() {
    // @click.stop.prevent="dialog = true" on an <a href="#"> element.
    let code = compile_and_validate_template(
        r##"<template><a href="#" @click.stop.prevent="dialog = true">link</a></template>"##,
    );
    assert!(
        code.contains("_withModifiers"),
        "Assignment handler with modifiers should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains("$event => {"),
        "Assignment handler with modifiers should be wrapped\n{}",
        code
    );
}

#[test]
fn empty_string_event_handler_outputs_noop() {
    // @click.stop="" has an empty string value. It should produce a no-op
    // function, not an empty value in the object literal, wrapped in _withModifiers.
    let code =
        compile_and_validate_template(r#"<template><div @click.stop="">text</div></template>"#);
    assert!(
        code.contains("_withModifiers"),
        "Empty string handler with .stop should use _withModifiers\n{}",
        code
    );
}

#[test]
fn vdom_event_handler_applies_ctx_prefix() {
    let code = compile_and_validate_template(
        r#"<template><button @click="handleClick">go</button></template>"#,
    );
    assert!(
        code.contains("_ctx.handleClick") || code.contains("$setup.handleClick"),
        "Event handler identifier should have binding prefix\n{}",
        code
    );
}

#[test]
fn vapor_component_event_with_hyphen_camelcased() {
    // @popup-block → onPopupBlock
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @popup-block="handler" /></template>"#,
    );
    assert!(
        code.contains("onPopupBlock"),
        "Hyphenated event should be camelCased\n{}",
        code
    );
}

/// Vapor must wrap a multi-statement handler in a block AND resolve every
/// statement in it.
///
/// As above, the `<script setup>` bindings are load-bearing: without them the
/// wrapping assertion alone cannot distinguish a fully resolved handler from one
/// truncated at the first `;`.
#[test]
fn vapor_event_with_multi_statement_handler_resolves_every_statement() {
    let code = compile_and_validate_vapor_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template><MyComp @click="a = 1; b = 2" /></template>"#,
    );
    assert!(
        code.contains("() => {"),
        "Multi-statement handler should be wrapped in block\n{}",
        code
    );
    assert!(
        code.contains("() => { _ctx.a = 1; _ctx.b = 2 }"),
        "both statements must resolve, not just the first, got:\n{}",
        code
    );
    // Negative: `b` is a setup-scope `const` in the generated module, so a bare
    // `b = 2` is a build-time const reassignment (rolldown ILLEGAL_REASSIGNMENT).
    assert!(
        !code.contains("; b = 2"),
        "the unresolved second statement must not survive, got:\n{}",
        code
    );
}

#[test]
fn vapor_event_with_trailing_semicolon() {
    // Trailing semicolons should be stripped
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @click="doStuff();" /></template>"#,
    );
    assert!(
        !code.contains(";"),
        "Trailing semicolons should be stripped\n{}",
        code
    );
}

#[test]
fn vapor_component_event_with_colon_camelcased() {
    // @update:modelValue → onUpdateModelValue
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp @update:modelValue="handler" /></template>"#,
    );
    assert!(
        code.contains("onUpdateModelValue"),
        "Colon event should be camelCased\n{}",
        code
    );
}

#[test]
fn vapor_native_event_compound_expr() {
    let code = compile_and_validate_vapor_template(
        r#"<template><div @click="count++, emit('x')"></div></template>"#,
    );
    assert!(
        code.contains("_ctx.count"),
        "Compound event handler should prefix count\n{}",
        code
    );
    assert!(
        code.contains("_ctx.emit"),
        "Compound event handler should prefix emit\n{}",
        code
    );
}

/// The empty-SFC shell must not leak into template-only components: those
/// keep their existing no-synthetic-script shape (the main-module assembler
/// owns their shell).
#[test]
fn template_only_sfc_still_emits_no_synthetic_script() {
    let result = compile_sfc("<template><div>x</div></template>");
    assert!(
        result.script.is_none(),
        "template-only SFC must not gain a synthetic script block, got:\n{:?}",
        result.script.as_ref().map(|s| &s.code)
    );
    assert!(
        result.template.is_some(),
        "template block must still compile"
    );
}

// ==================== Component PatchFlags ====================

// @ai-generated - Components with dynamic props should emit PATCH_PROPS flag
#[test]
fn component_with_dynamic_prop_emits_patch_props() {
    let result = compile_sfc(
        r#"<template><div><MyComp :msg="val" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should have patch flag 8 (PROPS)
    assert!(
        tpl.code.contains("8 /* PROPS */") || tpl.code.contains(", 8,"),
        "component with dynamic prop should emit PATCH_PROPS (8), got:\n{}",
        tpl.code
    );
    // Should have dynamic props array
    assert!(
        tpl.code.contains(r#"["msg"]"#),
        "component with dynamic prop should list dynamic props, got:\n{}",
        tpl.code
    );
}

// ==================== Inline event handler wrapping ====================

// @ai-generated - Inline event handlers with function calls need $event => () wrapping
#[test]
fn inline_event_handler_gets_arrow_wrapping() {
    let result = compile_sfc(
        r#"<template><div><button @click="onClick(tab)">click</button></div></template>
<script setup>
const tab = ref('a')
function onClick(t) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$event => ("),
        "inline event handler call should be wrapped with $event => (), got:\n{}",
        tpl.code
    );
}

#[test]
fn member_expression_event_handler_not_wrapped() {
    let result = compile_sfc(
        r#"<template><div><button @click="onClick">click</button></div></template>
<script setup>function onClick() {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("$event =>"),
        "simple member expression event handler should NOT be wrapped, got:\n{}",
        tpl.code
    );
}

// ==================== Dynamic props array ====================

// Event-handler keys never enter dynamicProps: Vue relies on stable invoker
// caching for listeners, so a handler binding never needs a PATCH_PROPS
// re-patch. Confirmed directly against the real `@vue/compiler-sfc` (rc.5)
// oracle — `@click`-only props objects emit neither a dynamicProps array nor
// the 8 /* PROPS */ flag, and `@click` never appears in a mixed array
// alongside a genuinely dynamic prop like `:disabled`.
#[test]
fn element_with_event_handler_has_dynamic_props_array() {
    let result = compile_sfc(
        r#"<template><button @click="handler">text</button></template>
<script setup>const handler = () => {};</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("[\"onClick\"]") && !tpl.code.contains("8 /* PROPS */"),
        "an @click-only props object must not produce a dynamicProps array \
         or a PROPS patch flag, got:\n{}",
        tpl.code
    );
}

#[test]
fn element_with_dynamic_bind_and_event_has_dynamic_props_array() {
    // :disabled is genuinely dynamic (needs PATCH_PROPS); @click never does.
    let result = compile_sfc(
        r#"<template><button :disabled="isDisabled" @click="handler">go</button></template>
<script setup>const isDisabled = true; const handler = () => {};</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("[\"disabled\"]"),
        "dynamicProps array should contain only the genuinely dynamic \"disabled\" prop, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("\"onClick\""),
        "onClick must not appear in dynamicProps, got:\n{}",
        tpl.code
    );
}

// @ai-generated - Tests that template ref attribute is emitted in VDOM render output
#[test]
fn template_ref_emitted_in_vdom_render() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const container = ref()
</script>
<template><div ref="container">hello</div></template>"#,
    );
    assert!(
        code.contains("ref: \"container\""),
        "Template ref should be emitted as ref: \"container\" in props. Got:\n{}",
        code
    );
}

#[test]
pub(super) fn optional_tuple_element_in_define_emits() {
    // Regression: menu.vue from vue-vben-admin panics on TSTupleElement
    // with optional tuple elements like `[string, string?]` in defineEmits.
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("menu.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: true,
        ..Default::default()
    };
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_emits_entry(
        1,
        1,
        ["open", "select"],
    )]);
    let result = compile(
        r#"<script lang="ts" setup>
import { Menu } from '@vben-core/menu-ui';

// Runtime props isolate the optional-tuple test to typed defineEmits.
const props = defineProps({
  accordion: Boolean,
  collapse: Boolean,
  collapseShowTitle: Boolean,
  defaultActive: String,
  menus: Array,
  mode: String,
  rounded: Boolean,
  theme: String,
});

const emit = defineEmits<{
  open: [string, string[]];
  select: [string, string?];
}>();

function handleMenuSelect(key: string) {
  emit('select', key, props.mode);
}

function handleMenuOpen(key: string, path: string[]) {
  emit('open', key, path);
}
</script>

<template>
  <Menu
:accordion="accordion"
:collapse="collapse"
:collapse-show-title="collapseShowTitle"
:default-active="defaultActive"
:menus="menus"
:mode="mode"
:rounded="rounded"
scroll-to-active
:theme="theme"
@open="handleMenuOpen"
@select="handleMenuSelect"
  />
</template>
"#,
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Runtime(runtime),
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.script.is_some());
    let script = result.script.unwrap();
    assert!(
        script.code.contains("_defineComponent"),
        "should contain _defineComponent"
    );
}

// ==================== Event modifier handling ====================

#[test]
pub(super) fn event_modifier_prevent_uses_with_modifiers() {
    // @click.prevent="handler" should wrap with _withModifiers
    let code = compile_and_validate_template(
        r#"<template><div @click.prevent="handler">text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "Event with .prevent modifier should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains(r#""prevent""#),
        "Should include 'prevent' in modifier list\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_stop_prevent_combined() {
    // @click.stop.prevent should wrap handler with _withModifiers(handler, ["stop", "prevent"])
    let code = compile_and_validate_template(
        r#"<template><div @click.stop.prevent="handler">text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "Multiple modifiers should use _withModifiers\n{}",
        code
    );
    assert!(
        code.contains(r#""stop""#) && code.contains(r#""prevent""#),
        "Should include both 'stop' and 'prevent' in modifier list\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_capture_goes_into_key() {
    // @click.capture="handler" should produce onClickCapture: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.capture="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickCapture"),
        "Capture modifier should be appended to event key name\n{}",
        code
    );
    // Should NOT use _withModifiers for capture (it's an option modifier)
    assert!(
        !code.contains("_withModifiers"),
        "Capture modifier should not use _withModifiers\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_once_goes_into_key() {
    // @click.once="handler" should produce onClickOnce: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.once="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickOnce"),
        "Once modifier should be appended to event key name\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_passive_goes_into_key() {
    // @click.passive="handler" should produce onClickPassive: handler
    let code = compile_and_validate_template(
        r#"<template><div @click.passive="handler">text</div></template>"#,
    );
    assert!(
        code.contains("onClickPassive"),
        "Passive modifier should be appended to event key name\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_keyup_enter_uses_with_keys() {
    // @keyup.enter="handler" should wrap with _withKeys
    let code =
        compile_and_validate_template(r#"<template><input @keyup.enter="handler" /></template>"#);
    assert!(
        code.contains("_withKeys"),
        "Key modifier should use _withKeys\n{}",
        code
    );
    assert!(
        code.contains(r#""enter""#),
        "Should include 'enter' in key list\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_empty_handler_with_prevent() {
    // @click.prevent="" should produce _withModifiers(() => {}, ["prevent"])
    let code =
        compile_and_validate_template(r#"<template><div @click.prevent="">text</div></template>"#);
    assert!(
        code.contains("_withModifiers"),
        "Empty handler with .prevent should still use _withModifiers\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_prevent_only_no_value() {
    // @contextmenu.prevent (no value) should produce _withModifiers(() => {}, ["prevent"])
    let code = compile_and_validate_template(
        r#"<template><div @contextmenu.prevent>text</div></template>"#,
    );
    assert!(
        code.contains("_withModifiers"),
        "No-value handler with .prevent should use _withModifiers\n{}",
        code
    );
}

#[test]
pub(super) fn event_modifier_on_component_generates_import() {
    // When a component has @click.stop="handler", the compiled output
    // should include _withModifiers AND the import for it.
    let result = compile_sfc(r#"<template><MyComponent @click.stop="handler" /></template>"#);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_withModifiers"),
        "Component event with .stop modifier should use _withModifiers\n{}",
        tpl.code
    );
    // The import must be present — without it, _withModifiers is a ReferenceError at runtime
    assert!(
        tpl.imports.contains(&"_withModifiers"),
        "Component event with modifiers should import withModifiers from vue\nimports: {:?}\n{}",
        tpl.imports,
        tpl.code
    );
}

// ==================== Type-based defineEmits ====================

#[test]
pub(super) fn type_based_define_emits_generates_emits_option() {
    // defineEmits<{ mousedown: [event: MouseEvent] }>() should generate
    // emits: ["mousedown"] in the component definition.
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_emits_entry(
        0,
        0,
        ["mousedown"],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
const emit = defineEmits<{ mousedown: [event: MouseEvent] }>()
</script>
<template><div>test</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains(r#"emits:"#) || script.code.contains(r#"emits :"#),
        "Type-based defineEmits should generate emits option in component definition\n{}",
        script.code
    );
    assert!(
        script.code.contains("mousedown"),
        "Emits option should include 'mousedown'\n{}",
        script.code
    );
}

#[test]
pub(super) fn type_based_define_emits_call_signature_generates_emits_option() {
    // defineEmits<{ (e: 'change', value: string): void }>() should generate
    // emits: ["change"] in the component definition.
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_emits_entry(
        0,
        0,
        ["change"],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
const emit = defineEmits<{ (e: 'change', value: string): void }>()
</script>
<template><div>test</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains(r#"emits:"#) || script.code.contains(r#"emits :"#),
        "Type-based defineEmits with call signature should generate emits option\n{}",
        script.code
    );
    assert!(
        script.code.contains("change"),
        "Emits option should include 'change'\n{}",
        script.code
    );
}

#[test]
fn vapor_event_handler_uses_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst onClick = () => {}\n</script>\n<template><button @click=\"onClick\">click</button></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor event handler should not use $setup. prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured prop in event handler expression
#[test]
pub(super) fn destructured_prop_in_event_handler() {
    let result = compile_sfc(
        r#"<template><button @click="handler">click</button></template>
<script setup lang="ts">const { handler } = defineProps<{ handler: () => void }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.handler"),
        "destructured prop in event handler should resolve to $props.handler, got:\n{}",
        tpl.code
    );
}

// ══════════════════════════════════════════════════════════════════════
// Bug 1: Duplicate event handler keys — merge into arrays
// ══════════════════════════════════════════════════════════════════════

/// @ai-generated — Two handlers on same event with different modifiers merged into array
#[test]
pub(super) fn duplicate_event_handlers_same_event_merged_into_array() {
    let result = compile_sfc(
        r#"<template><div @keydown="a" @keydown.stop="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Should have merged array syntax: onKeydown: [handler1, handler2]
    assert!(
        tpl.code.contains("onKeydown: ["),
        "should have merged array syntax onKeydown: [...], got:\n{}",
        tpl.code
    );
    // Should NOT have duplicate keys (two separate "onKeydown:" entries)
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "should have exactly one onKeydown: key (merged), got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Three+ handlers on same event all merged into array
#[test]
pub(super) fn multiple_event_handlers_same_event_merged_into_array() {
    let result = compile_sfc(
        r#"<template><div @keydown="a" @keydown.stop="b" @keydown.prevent="c"></div></template>
<script setup>const a = () => {}; const b = () => {}; const c = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "should have merged array syntax, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "should have exactly one onKeydown: key (all merged), got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Key modifiers on same event merged into array
#[test]
pub(super) fn key_modifiers_same_event_merged() {
    let result = compile_sfc(
        r#"<template><div @keydown.enter="a" @keydown.tab="b"></div></template>
<script setup>const a = () => {}; const b = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onKeydown: ["),
        "key modifier handlers should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "key modifier handlers should be merged, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Mixed: some events have duplicates, others don't
#[test]
pub(super) fn mixed_duplicate_and_unique_events() {
    let result = compile_sfc(
        r#"<template><div @click="a" @keydown="b" @keydown.stop="c" @mouseenter="d"></div></template>
<script setup>const a = () => {}; const b = () => {}; const c = () => {}; const d = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // onKeydown should be merged into array
    assert!(
        tpl.code.contains("onKeydown: ["),
        "onKeydown should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onKeydown:").count(),
        1,
        "onKeydown should appear as one key (merged), got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("onClick:"),
        "onClick should be present, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("onMouseenter:"),
        "onMouseenter should be present, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Single handler with modifier (no merge needed, regression test)
#[test]
pub(super) fn single_event_handler_no_merge() {
    let result = compile_sfc(
        r#"<template><div @click.stop="a"></div></template>
<script setup>const a = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onClick"),
        "single handler should still produce onClick key, got:\n{}",
        tpl.code
    );
    // Should NOT be wrapped in array
    assert!(
        !tpl.code.contains("[_withModifiers") && !tpl.code.contains("[withModifiers"),
        "single handler should NOT be wrapped in array, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — @input and :onInput produce the same key, must be merged (Vue official behavior)
#[test]
pub(super) fn v_on_and_v_bind_on_same_event_merged() {
    let result = compile_sfc(
        r#"<template><input @input="foo" :onInput="bar" /></template>
<script setup>const foo = () => {}; const bar = () => {}</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("onInput: ["),
        "@input and :onInput should be merged into array, got:\n{}",
        tpl.code
    );
    assert_eq!(
        tpl.code.matches("onInput:").count(),
        1,
        "@input and :onInput should produce one onInput: key, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Dynamic event names cannot be pre-computed, should NOT be merged
#[test]
pub(super) fn dynamic_event_names_not_merged() {
    let result = compile_sfc(
        r#"<template><div @[eventName]="a" @[eventName]="b"></div></template>
<script setup>const eventName = 'click'; const a = () => {}; const b = () => {}</script>"#,
    );
    // Dynamic event names should both appear (can't pre-compute key)
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.message.contains("duplicate")),
        "dynamic event names should not trigger duplicate errors"
    );
}

// ==================== Template-only + scoped styles ====================

/// Template-only component with `<style scoped>` should emit a synthetic
/// script block containing `__scopeId` so Vue's runtime applies the
/// scoped `data-v-*` attributes to DOM elements.
#[test]
pub(super) fn template_only_scoped_style_emits_scope_id_in_script() {
    let result = compile_sfc(
        "<template><div class=\"app\">hello</div></template>\n<style scoped>\n.app { color: red; }\n</style>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(!result.scope_id.is_empty(), "should have scope_id");

    let script = result
        .script
        .as_ref()
        .expect("template-only component with scoped style should emit a synthetic script block");
    assert!(
        script.code.contains("__scopeId"),
        "script should contain __scopeId assignment, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains(&result.scope_id),
        "script should reference the scope_id '{}', got:\n{}",
        result.scope_id,
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "script should export __sfc__, got:\n{}",
        script.code
    );
}

#[test]
fn tsx_infer_function_native_event_matrix_uses_dom_event_authority() {
    let cases = [
        (
            "handleChange",
            "e",
            r#"<select @change="handleChange"></select>"#,
            "change",
        ),
        (
            "handleSubmit",
            "e",
            r#"<form @submit="handleSubmit"></form>"#,
            "submit",
        ),
        (
            "handleKeydown",
            "e",
            r#"<input @keydown="handleKeydown" />"#,
            "keydown",
        ),
        (
            "handleFocus",
            "e",
            r#"<input @focus="handleFocus" />"#,
            "focus",
        ),
        ("handleBlur", "e", r#"<input @blur="handleBlur" />"#, "blur"),
        (
            "handleMouseEnter",
            "e",
            r#"<div @mouseenter="handleMouseEnter"></div>"#,
            "mouseenter",
        ),
        (
            "handleMouseLeave",
            "e",
            r#"<div @mouseleave="handleMouseLeave"></div>"#,
            "mouseleave",
        ),
        (
            "handleAnchorClick",
            "e",
            r##"<a href="#" @click="handleAnchorClick">Link</a>"##,
            "click",
        ),
        (
            "handleDblClick",
            "e",
            r#"<div @dblclick="handleDblClick"></div>"#,
            "dblclick",
        ),
        (
            "handleContextMenu",
            "e",
            r#"<div @contextmenu="handleContextMenu"></div>"#,
            "contextmenu",
        ),
    ];

    for (fn_name, param, template, event_name) in cases {
        let source = format!(
            r#"<script setup lang="ts">
function {fn_name}({param}) {{ return {param} }}
</script>
<template>{template}</template>"#
        );
        let result = compile_tsx(&source);
        assert!(
            result.errors.is_empty(),
            "errors for {}: {:?}",
            fn_name,
            result.errors
        );

        let tsx = result.tsx.as_ref().expect("tsx block");
        let expected = format!(
            "...[{param}]: [(GlobalEventHandlersEventMap & {{ [___VERTER___EventKey: string]: Event }})[\"{event_name}\"]]"
        );
        assert!(
            tsx.code.contains(&expected),
            "Expected inferred native event type for {}.\nExpected snippet: {}\nActual TSX:\n{}",
            fn_name,
            expected,
            tsx.code
        );
    }
}

#[test]
pub(super) fn tsx_infer_function_component_events_from_imported_components() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import MyButton from './MyButton.vue'
import CustomSelect from './CustomSelect.vue'
import DataTable from './DataTable.vue'

function handleChange(e) { return e }
function onClick(e) { return e }
function onSelect(item) { return item }
function handleUpdate(data) { return data }
</script>
<template>
  <MyComp @change="handleChange" />
  <MyButton @click="onClick" />
  <CustomSelect @select="onSelect" />
  <DataTable @update="handleUpdate" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"...[e]: Parameters<NonNullable<Required<InstanceType<typeof MyComp>["$props"]>["onChange"]>>"#
        ),
        "Expected component change event inference from MyComp props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[e]: Parameters<NonNullable<Required<InstanceType<typeof MyButton>["$props"]>["onClick"]>>"#
        ),
        "Expected component click event inference from MyButton props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[item]: Parameters<NonNullable<Required<InstanceType<typeof CustomSelect>["$props"]>["onSelect"]>>"#
        ),
        "Expected component custom event inference from CustomSelect props, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"...[data]: Parameters<NonNullable<Required<InstanceType<typeof DataTable>["$props"]>["onUpdate"]>>"#
        ),
        "Expected component update event inference from DataTable props, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("IntrinsicElementAttributes"),
        "Component event inference should not use native IntrinsicElementAttributes, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_does_not_infer_inline_call_event_handler() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handler(e) {
  return e
}
</script>
<template>
  <div @click="handler()"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handler(e)"),
        "Inline call handler should not trigger parameter inference, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]: Parameters<"),
        "Inline call handler should not be rewritten with inferred tuple rest type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_call_expression_is_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = { toString: () => "ok" }
</script>
<template>
  <div @click="test.toString()"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={() =>"),
        "Call-expression event handler should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("test.toString()"),
        "Wrapped handler should preserve call expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_simple_member_and_arrow_handlers_not_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => 1
const state = { click: () => 2 }
const handler = (event) => event
</script>
<template>
  <div @click="test"></div>
  <div @click="state.click"></div>
  <div @click="handler"></div>
  <div @click="function (...args) { return args }"></div>
  <div @input="(...args) => args"></div>
  <div @touchmove="(event) => { event; }"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={test}"),
        "Simple identifier handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={state.click}"),
        "Member-expression handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={handler}"),
        "Function reference handler should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onInput={(...args) => args}"),
        "Inline spread arrow function should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("onClick={function (...args) { return args }}"),
        "Inline function with spread params should not be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onTouchmove={(event) => { event; }}"),
        "Inline arrow function with explicit parameter should not be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_object_literal_handler_not_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = 1
</script>
<template>
  <div @click="{ test }"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onClick={{ test }}") || tsx.code.contains("onClick={{test}}"),
        "Object-literal handler should remain direct object expression, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("onClick={() =>"),
        "Object-literal handler should not be wrapped in callback, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_string_template_and_ternary_handlers_are_wrapped() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = true
</script>
<template>
  <div @click="'foo'" />
  <div @click="`foo${'test'}`" />
  <div @click="foo ? 'bar' : 'baz'" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("onClick={() => {'foo'}}"),
        "String literal event expression should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={() => {`foo${'test'}`}}"),
        "Template-string event expression should be wrapped, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={() => {foo ? 'bar' : 'baz'}}"),
        "Ternary event expression should be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_name_with_vue_namespace_is_preserved() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => {}
</script>
<template>
  <div @vue:mounted="test" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("onVue:mounted={test}"),
        "Namespaced vue event should map to onVue:mounted, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_hyphenated_name_camelcases_segments() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const test = () => {}
</script>
<template>
  <div @test-camel-case="test" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Kebab-case events preserve hyphens and use spread syntax since
    // "onTest-camel-case" is not a valid JSX identifier.
    assert!(
        tsx.code.contains(r#""onTest-camel-case""#),
        "Kebab event should preserve hyphens in spread syntax, got:\n{}",
        tsx.code
    );
    // Should NOT be camelized
    assert!(
        !tsx.code.contains("onTestCamelCase"),
        "Kebab event should NOT be camelized, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_on_object_literal_rewrites_to_on_event_keys() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const click = () => {}
const mouseenter = () => {}
</script>
<template>
  <button v-on="{ click, mouseenter }" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        normalized.contains("{...{onClick:") && normalized.contains("onMouseenter:"),
        "v-on object literal should map event keys to JSX on* props, got:\n{}",
        tsx.code
    );
    assert!(
        !normalized.contains("{...{click:"),
        "Raw DOM event keys should not remain inside v-on object spread, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_49_event_handlers_with_spread_params_do_not_bind_args_to_ctx() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div
    @click="function (...args) {}"
    @input="(...args) => {}"
    @touchmove="
      (event) => {
        event;
      }
    "
  />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("..._ctx.args"),
        "Spread event parameters must never be prefixed to _ctx, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...___VERTER___ctx.args"),
        "Spread event parameters must never be prefixed to ___VERTER___ctx, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onClick={function (...args) {}}"),
        "Function handler with spread args should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onInput={(...args) => {}}"),
        "Arrow handler with spread args should be preserved, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("onTouchmove={(event) => {") && tsx.code.contains("event;"),
        "Arrow handler with explicit param should stay direct (no wrapper), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_48_event_identifier_does_not_prefix_dollar_event_with_ctx() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div @click="$event"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // `$event` is bound as the handler's sole parameter so it is contextually typed
    // by the JSX event prop, and stays inside the callback scope (never
    // context-prefixed).
    assert!(
        normalized.contains("onClick={($event)=>{$event}}"),
        "Bare $event should be emitted as a contextually-typed ($event) => callback, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.$event"),
        "$event must not be context-prefixed, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___eventCallbacks("),
        "$event handler should not call the generic eventCallbacks wrapper, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_48_event_member_expression_stays_in_event_scope() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <input @input="$event.target" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // `$event` member access stays inside the contextually-typed ($event) =>
    // callback scope, never context-prefixed.
    assert!(
        normalized.contains("onInput={($event)=>{$event.target}}"),
        "$event member expressions stay inside the ($event) => callback scope, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.$event"),
        "$event must not be context-prefixed, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("___VERTER___eventCallbacks("),
        "$event member handler should not call the generic eventCallbacks wrapper, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_event_handler_under_v_if_includes_runtime_guard() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg: string | number = Math.random() > 0.5 ? 'x' : 0
</script>
<template>
  <button v-if="typeof msg === 'string'" @click="msg.toLowerCase()" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    // In the new output, .value is NOT appended — msg stays as msg
    assert!(
        normalized.contains(
            "onClick={()=>{if(!((typeofmsg==='string'))){returnundefined;}msg.toLowerCase()}}"
        ),
        "v-if event handlers should include the guard inside callback for narrowing (no .value), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_issue_79_v_on_object_syntax_supports_explicit_event_map() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const doThis = () => {}
const doThat = () => {}
</script>
<template>
  <button v-on="{ mousedown: doThis, mouseup: doThat }" />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    let normalized: String = tsx.code.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        normalized.contains("onMousedown:doThis") && normalized.contains("onMouseup:doThat"),
        "v-on object explicit map should convert to on* keys, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_template_tag_empty_template_emits_empty_fragment() {
    let result = compile_tsx(r#"<template></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<></>"),
        "Empty template should still emit an empty fragment, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_item_in_items_emits_map_expression() {
    let result = compile_tsx(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("items).map((item") || tsx.code.contains("_ctx.items).map((item"),
        "v-for item in items should compile to .map expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_item_of_items_emits_map_expression() {
    let result =
        compile_tsx(r#"<template><div v-for="item of items">{{ item + 1 }}</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("items).map((item") || tsx.code.contains("_ctx.items).map((item"),
        "v-for item of items should compile to .map expression, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ item + 1 }") || tsx.code.contains("{ _ctx.item + 1 }"),
        "v-for loop body expression should be preserved, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_for_nested_loop_emits_nested_maps() {
    let result = compile_tsx(
        r#"<template><li v-for="item in items"><span v-for="childItem in item.children">{{ item.message }} {{ childItem }}</span></li></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.matches(".map((").count() >= 2,
        "Nested v-for should produce nested map expressions, got:\n{}",
        tsx.code
    );
}

/// @ai-generated — TSX source map: @click event handler maps back
#[test]
fn tsx_sourcemap_event_handler() {
    let source = r#"<script setup>
function handler() {}
</script>

<template>
  <button @click="handler">click</button>
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — Element with event is NOT hoisted
#[test]
fn static_hoist_event_not_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><button @click="fn">click</button></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "element with event should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

#[test]
fn tsx_comp_emitted_for_root_element_without_ref() {
    // Single root: Comp function emitted for root element (for implicit attrs)
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "Should have Comp function for single root element, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___getRootComponent"),
        "Should have getRootComponent with template, got:\n{}",
        tsx.code
    );
    let comp_count = tsx.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count, 1,
        "Should have exactly 1 Comp function (single root), got {} in:\n{}",
        comp_count, tsx.code
    );

    // Multi-root (fragment): no Comp functions for root (no ref, no attrs fallthrough)
    let result2 = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div><MyComp /></template>"#,
    );
    assert!(result2.errors.is_empty(), "errors: {:?}", result2.errors);
    let tsx2 = result2.tsx.as_ref().expect("tsx block");
    // getRootComponent still emitted (returns {})
    assert!(
        tsx2.code.contains("___VERTER___getRootComponent"),
        "Should have getRootComponent with template, got:\n{}",
        tsx2.code
    );
    // No Comp functions (fragment, no ref elements)
    let comp_count2 = tsx2.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count2, 0,
        "Fragment should have 0 Comp functions (no ref), got {} in:\n{}",
        comp_count2, tsx2.code
    );
    // getRootComponent returns {}
    assert!(
        tsx2.code.contains("getRootComponent() { return {};"),
        "Fragment getRootComponent should return empty, got:\n{}",
        tsx2.code
    );
}

#[test]
fn tsx_parse_valid_events_click() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function handleClick() {}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
        "event @click",
    );
}

#[test]
fn tsx_parse_valid_events_inline_expression() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template>
  <button @click="count++">{{ count }}</button>
</template>"#,
        "event inline expression",
    );
}

#[test]
fn tsx_parse_valid_events_with_event_param() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function handle(e: Event) {}
</script>
<template>
  <input @input="handle($event)" />
</template>"#,
        "event with $event",
    );
}

#[test]
fn tsx_parse_valid_events_multi_statement() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template>
  <button @click="a++; b--">go</button>
</template>"#,
        "event multi-statement",
    );
}

#[test]
fn tsx_parse_valid_define_emits() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_emits_entry(
        0,
        0,
        ["change", "update"],
    )]);
    assert_tsx_parses_with_runtime(
        r#"<script setup lang="ts">
const emit = defineEmits<{
  change: [value: string]
  update: [id: number, value: string]
}>()
</script>
<template>
  <button @click="emit('change', 'hello')">go</button>
</template>"#,
        "defineEmits typed",
        runtime,
    );
}

#[test]
fn tsx_parse_valid_v_on_object() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
function onMouseDown() {}
function onMouseUp() {}
</script>
<template>
  <div v-on="{ mousedown: onMouseDown, mouseup: onMouseUp }">drag me</div>
</template>"#,
        "v-on object syntax",
    );
}

#[test]
fn tsx_global_component_spread_event_resolves_via_fallback_const() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function onPing(s: string) { void s; }
</script>
<template>
  <GlobalEmitComp @ping="onPing($event)" @ping="onPing($event)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Script generation emits the GlobalComponents fallback const for the unimported
    // component through the @verter/types helper (a REAL import statement the tsgo
    // provider resolves from the virtual TSX — never an `import('vue')` type query).
    assert!(
        tsx.code.contains(
            "const GlobalEmitComp = {} as ___VERTER___GlobalComponentType<'GlobalEmitComp'>;"
        ),
        "must emit the GlobalComponents fallback const: {}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("import('vue').GlobalComponents"),
        "the import('vue') type-query form is retired from the virtual TSX: {}",
        tsx.code
    );
    // The duplicate-spread `$event` resolves through that same fallback const.
    assert!(
        tsx.code.contains(
            r#"$event: Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onPing"]>>[0]"#
        ),
        "spread $event must resolve via InstanceType<typeof GlobalEmitComp>: {}",
        tsx.code
    );
    // No spread-event antipattern — untyped `$event`, the intrinsic-attribute surface,
    // the retired eventCallbacks helper, or the tsgo-unresolvable `GlobalComponents[...]`
    // indexed event type (the fallback const's `GlobalComponents extends` is distinct).
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn tsx_local_component_spread_event_resolves_via_binding_no_fallback() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import EmitChild from './EmitChild.vue';
function onPick(n: number) { void n; }
</script>
<template>
  <EmitChild @pick="onPick($event)" @pick="onPick($event)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"$event: Parameters<NonNullable<Required<InstanceType<typeof EmitChild>["$props"]>["onPick"]>>[0]"#
        ),
        "spread $event must resolve via the imported binding InstanceType<typeof EmitChild>: {}",
        tsx.code
    );
    // An imported component must NOT also get a GlobalComponents fallback const.
    assert!(
        !tsx.code
            .contains("const EmitChild = {} as ___VERTER___GlobalComponentType"),
        "imported component must NOT get a fallback const: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

/// Interface/type alias/enum handling under force_js. TIGHTENED (F1 guard): the
/// enum must lower to its runtime IIFE, not merely leave `Color` behind via the
/// incidental `Color.Red` reference — a dropped enum is a runtime ReferenceError.
#[test]
fn force_js_strips_declarations_and_emits_enum_runtime() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
interface Foo { a: string }
type Bar = number | string
enum Color { Red, Green }
const c = Color.Red
</script>
<template><div>{{ c }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("interface Foo"), "interface must go: {code}");
    assert!(!code.contains("type Bar"), "type alias must go: {code}");
    assert!(!code.contains("enum Color"), "enum keyword must go: {code}");
    // Runtime IIFE shape — a fully-dropped enum passes a bare `contains("Color")`.
    assert!(
        code.contains("var Color;"),
        "enum must emit runtime `var Color;`, got: {code}"
    );
    assert!(
        code.contains("(function(Color)"),
        "enum must emit runtime IIFE `(function(Color)`, got: {code}"
    );
    assert!(
        code.contains("Color[Color[\"Red\"] = 0]"),
        "enum members must be assigned in the IIFE, got: {code}"
    );
}

/// F1 guard: an `export enum` under force_js must lower to its runtime IIFE and
/// stay valid JS — never a bare `export var E; …` inside the setup() wrapper.
#[test]
fn force_js_exported_enum_emits_runtime_iife() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
export enum Color { Red, Green }
const c = Color.Red
</script>
<template><div>{{ c }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("enum Color"), "enum keyword must go: {code}");
    assert!(
        !code.contains("export var Color"),
        "must not emit `export` inside the setup wrapper, got: {code}"
    );
    assert!(
        code.contains("var Color;"),
        "exported enum must emit runtime `var Color;`, got: {code}"
    );
    assert!(
        code.contains("(function(Color)"),
        "exported enum must emit runtime IIFE, got: {code}"
    );
    assert!(
        code.contains("Color[Color[\"Red\"] = 0]"),
        "exported enum members must be assigned, got: {code}"
    );
}

// =========================================================================
// D2 — template ref binding
// =========================================================================
//
// Official inline (compileScript({ inlineTemplate: true })): a static
// `ref="el"` whose name matches a setup-let/setup-ref/setup-maybe-ref binding
// compiles to `{ ref_key: "el", ref: el }` — the setup binding receives the
// element. A dynamic `:ref="elRef"` resolves in scope (`{ ref: elRef.value }`
// inline / `{ ref: $setup.elRef }` non-inline) and is NEVER hoisted out of
// setup. Non-inline static refs stay `{ ref: "el" }`.

#[test]
fn inline_static_ref_with_setup_binding_emits_ref_key_binding() {
    let result = compile_sfc_inline(
        "<script setup>\nimport { ref } from 'vue'\nconst el = ref(null)\n</script>\n<template><div ref=\"el\">x</div></template>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("ref_key: \"el\""),
        "inline static ref to a setup binding must emit ref_key, got:\n{}",
        code
    );
    assert!(
        code.contains("ref: el"),
        "inline static ref to a setup binding must reference the binding directly, got:\n{}",
        code
    );
    // The ref pair must live INSIDE setup (the render closure), never hoisted
    // to module scope (would be a ReferenceError).
    let setup_pos = code.find("setup(__props").expect("setup present");
    let ref_pos = code.find("ref_key: \"el\"").expect("ref_key present");
    assert!(
        ref_pos > setup_pos,
        "ref_key/ref must be inside setup (the render closure), got:\n{}",
        code
    );
    assert!(
        !code.contains("const _hoisted_"),
        "the ref props object must not be hoisted to module scope, got:\n{}",
        code
    );
}

#[test]
fn define_emits_local_ref_is_compile_error() {
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst evt = ref('save')\ndefineEmits([evt])\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineEmits()` in <script setup> cannot reference locally declared variables"
            )),
        "defineEmits referencing a setup-local must be rejected (official), got: {:?}",
        result.errors
    );
}

// =========================================================================
// Multi-statement v-on handlers: EVERY statement is binding-resolved
//
// A `v-on` value is an inline STATEMENT LIST, not a single expression
// (`@click="a = 1; b = 2"`). Parsing it as one expression silently stops at
// the first `;`, leaving every later statement unprefixed: reads resolve to
// nothing and writes hit the setup-scope `const` directly. Official Vue
// (`@vue/compiler-sfc`) resolves every statement, and so must Verter.
// =========================================================================

/// Non-inline (`$setup.`) mode: statement 2+ must be prefixed exactly like
/// statement 1 — for WRITES and for READS alike.
#[test]
fn multi_statement_event_handler_resolves_every_statement() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template><button @click="a = 1; b = 2">x</button></template>"#,
    );
    assert!(
        code.contains("$setup.a = 1"),
        "first statement must be prefixed, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.b = 2"),
        "second statement must be prefixed too, got:\n{}",
        code
    );
    // Negative: the bare, unprefixed write must NOT survive anywhere. In the
    // generated module `b` is a setup-scope `const`, so a bare `b = 2` is a
    // build-time const reassignment (rolldown ILLEGAL_REASSIGNMENT).
    assert!(
        !code.contains("; b = 2"),
        "bare unprefixed assignment must not be emitted, got:\n{}",
        code
    );
}

/// The defect is statement POSITION, not assignment: a plain READ in the
/// second statement is missed the same way.
#[test]
fn multi_statement_event_handler_resolves_reads_after_first_statement() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template><button @click="a = 1; console.log(b)">x</button></template>"#,
    );
    assert!(
        code.contains("$setup.a = 1"),
        "first statement must be prefixed, got:\n{}",
        code
    );
    assert!(
        code.contains("console.log($setup.b)"),
        "a read in the second statement must be prefixed, got:\n{}",
        code
    );
    assert!(
        !code.contains("console.log(b)"),
        "bare unprefixed read must not be emitted, got:\n{}",
        code
    );
}

/// Update expressions (`a++`) and compound assignments (`a += 1`) after the
/// first statement resolve too.
#[test]
fn multi_statement_event_handler_resolves_update_and_compound_assignment() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
const c = ref(0)
</script>
<template><button @click="a = 1; b++; c += 2">x</button></template>"#,
    );
    assert!(
        code.contains("$setup.b++"),
        "update expression after the first statement must be prefixed, got:\n{}",
        code
    );
    assert!(
        code.contains("$setup.c += 2"),
        "compound assignment after the first statement must be prefixed, got:\n{}",
        code
    );
    assert!(
        !code.contains("; b++"),
        "bare unprefixed update must not be emitted, got:\n{}",
        code
    );
    assert!(
        !code.contains("; c += 2"),
        "bare unprefixed compound assignment must not be emitted, got:\n{}",
        code
    );
}

/// Inline mode (the shape the playground production build emits): setup refs
/// are in lexical scope as `const`, so an unprefixed write is a build-time
/// const reassignment. Every statement must go through `.value`.
#[test]
fn multi_statement_event_handler_resolves_every_statement_inline() {
    let result = compile_sfc_inline(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
</script>
<template><button @click="a = 1; b = 2">x</button></template>"#,
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("a.value = 1"),
        "first statement must unwrap the ref, got:\n{}",
        code
    );
    assert!(
        code.contains("b.value = 2"),
        "second statement must unwrap the ref, got:\n{}",
        code
    );
    // Negative: a bare `b = 2` reassigns the setup `const b` — exactly the
    // rolldown ILLEGAL_REASSIGNMENT that broke the playground build.
    assert!(
        !code.contains("; b = 2"),
        "bare const reassignment must not be emitted, got:\n{}",
        code
    );
}

/// A trailing semicolon does not make a value multi-statement, and the
/// single-statement shape is unchanged by the statement-list grammar.
///
/// The container choice is a real behavioural difference from Vue, not a
/// formatting one, so it is pinned here. Vue's `includes(';')` gives this value
/// a BLOCK, which returns `undefined`; the expression container RETURNS the
/// statement's value. Vue's DOM invoker hands the handler to
/// `callWithAsyncErrorHandling`, which attaches a `.catch` when the return value
/// is a promise — so for an async handler Verter routes a rejection to the app
/// error handler where Vue lets it escape unhandled.
#[test]
fn single_statement_event_handler_shape_is_unchanged() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
</script>
<template><button @click="a = 1;">x</button></template>"#,
    );
    assert!(
        code.contains("$setup.a = 1"),
        "single statement with trailing semicolon must still resolve, got:\n{}",
        code
    );

    let async_handler = compile_and_validate_template(
        r#"<script setup>
const asyncFn = async () => {}
</script>
<template><button @click="asyncFn();">x</button></template>"#,
    );
    assert!(
        async_handler.contains("$event => ($setup.asyncFn())"),
        "a trailing `;` takes the expression container, which returns the promise, got:\n{async_handler}"
    );
    assert!(
        !async_handler.contains("$event => {$setup.asyncFn();}"),
        "the block Vue emits here discards the promise and loses the rejection:\n{async_handler}"
    );
}

/// A multi-statement handler whose resolved text contains a `.` and no `(`
/// must still emit parseable TSX. A text-shaped member-expression probe
/// classifies it as a bare handler reference and emits a JSX expression
/// container holding a statement list, which does not parse.
#[test]
fn tsx_parse_valid_events_multi_statement_member_target() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { reactive } from 'vue'
const obj = reactive({ a: 0, b: 0 })
</script>
<template>
  <button @click="obj.a = 1; obj.b = 2">go</button>
</template>"#,
        "event multi-statement member target",
    );
}

/// Statement kinds beyond the simple expression statement resolve their
/// identifiers too. A `v-on` value is a full statement list, so the binding
/// visitor must descend into every statement form, not just the handful an
/// expression can nest.
#[test]
fn multi_statement_event_handler_resolves_all_statement_kinds() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const b = ref(0)
const arr = ref([1, 2])
</script>
<template>
  <button @click="a = 1; throw b">throw</button>
  <button @click="a = 1; switch (b) { case 1: a = 2 }">switch</button>
  <button @click="a = 1; for (const x of arr) { b = x }">forof</button>
  <button @click="a = 1; for (const k in arr) { b = k }">forin</button>
  <button @click="a = 1; try { a = b } catch (e) { a = 2 }">try</button>
  <button @click="a = 1; do { a = b } while (a < 3)">dowhile</button>
  <button @click="a = 1; lbl: { a = b }">labeled</button>
</template>"#,
    );
    for expected in [
        "throw $setup.b",
        "switch ($setup.b)",
        "of $setup.arr",
        "in $setup.arr",
        "try { $setup.a = $setup.b } catch (e) { $setup.a = 2 }",
        "do { $setup.a = $setup.b } while ($setup.a < 3)",
        "lbl: { $setup.a = $setup.b }",
    ] {
        assert!(
            code.contains(expected),
            "expected {expected:?} in generated code, got:\n{code}"
        );
    }
    // Negative: a `catch (e)` parameter is a handler-local binding and must NOT
    // be prefixed, and no bare unprefixed reference to `b` may survive.
    assert!(
        code.contains("catch (e)") && !code.contains("catch ($setup.e)"),
        "catch parameter is a local binding, got:\n{code}"
    );
    assert!(
        !code.contains("throw b"),
        "bare unprefixed throw argument must not be emitted, got:\n{code}"
    );
}

/// `for (i = 0; …)` initialises an EXISTING binding, so the init is a real
/// reference. Visiting only the declaration form of a `for` init leaves the
/// head half-resolved: `for (i = 0; $setup.i < $setup.n; $setup.i++)`.
#[test]
fn event_handler_resolves_for_statement_expression_init() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const i = ref(0)
const n = ref(3)
const log = (x) => x
</script>
<template><button @click="for (i = 0; i < n; i++) log(i)">x</button></template>"#,
    );
    assert!(
        code.contains("for ($setup.i = 0; $setup.i < $setup.n; $setup.i++) $setup.log($setup.i)"),
        "every position in the `for` head must resolve, got:\n{code}"
    );
    // Negative: the bare init is exactly the half-resolved state.
    assert!(
        !code.contains("for (i = 0"),
        "the `for` init must not stay unprefixed, got:\n{code}"
    );
}

/// The same head in INLINE mode, where a bare `i = 0` reassigns the setup
/// `const i` — a build-time const reassignment, not just a stale read.
#[test]
fn event_handler_for_expression_init_never_writes_a_bare_setup_const() {
    let code = compile_and_validate_inline_script(
        r#"<script setup>
import { ref } from 'vue'
const i = ref(0)
const n = ref(3)
const log = (x) => x
</script>
<template><button @click="for (i = 0; i < n; i++) log(i)">x</button></template>"#,
    );
    assert!(
        code.contains("for (i.value = 0; i.value < n.value; i.value++) log(i.value)"),
        "the `for` init must unwrap the ref, got:\n{code}"
    );
    assert!(
        !code.contains("for (i = 0"),
        "a bare `i = 0` reassigns the setup `const` (rolldown ILLEGAL_REASSIGNMENT), got:\n{code}"
    );
}

/// A class DECLARATION's heritage clause is an ordinary value reference. The
/// class NAME is a binding it introduces, so that stays bare.
#[test]
fn event_handler_resolves_class_declaration_heritage() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const Base = class {}
</script>
<template><button @click="a = 1; class X extends Base {}">x</button></template>"#,
    );
    assert!(
        code.contains("class X extends $setup.Base {}"),
        "the heritage clause must resolve, got:\n{code}"
    );
    // Negative: the declared class name is a LOCAL binding, never prefixed.
    assert!(
        !code.contains("class $setup.X"),
        "the declared class name must stay local, got:\n{code}"
    );
}

/// The same position in EXPRESSION form: `class extends Base {}` reaches the
/// heritage clause through `Expression::ClassExpression`, not
/// `Statement::ClassDeclaration`.
#[test]
fn event_handler_resolves_class_expression_heritage() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const Base = class {}
</script>
<template><button @click="a = 1; const C = class extends Base {}">x</button></template>"#,
    );
    assert!(
        code.contains("const C = class extends $setup.Base {}"),
        "a class EXPRESSION's heritage clause must resolve too, got:\n{code}"
    );
    assert!(
        !code.contains("class extends Base"),
        "the heritage clause must not stay unprefixed, got:\n{code}"
    );
}

/// `with (obj) { … }` resolves its object and its body, which is what
/// `@vue/compiler-sfc` emits. (The emitted `with` is a strict-mode error in
/// both compilers; leaving its identifiers bare would not fix that and would
/// make the handler silently reference nothing.)
#[test]
fn event_handler_resolves_with_statement() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const o = ref({})
</script>
<template><button @click="a = 1; with (o) { a = 2 }">x</button></template>"#,
    );
    assert!(
        code.contains("with ($setup.o) { $setup.a = 2 }"),
        "`with` object and body must both resolve, got:\n{code}"
    );
    assert!(
        !code.contains("with (o)"),
        "the `with` object must not stay unprefixed, got:\n{code}"
    );
}
