use super::test_support::assert_legacy;
fn run_full_pipeline(source: &str) -> String {
    assert_legacy(source).code
}
/// The fine-grained output where it resolves components once, at the top of
/// `render`, as Vue does, rather than beside each use.
fn render_code(source: &str) -> String {
    super::test_support::render(source).code
}
#[test]
fn full_pipeline_static_div() {
    let source = "<template><div>hello</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_template("),
        "Expected _template, got: {result}"
    );
    assert!(
        result.contains("function render("),
        "Expected render function, got: {result}"
    );
    assert!(
        result.contains("const n0 = t0()"),
        "Expected template instantiation, got: {result}"
    );
    assert!(
        result.contains("return n0"),
        "Expected return, got: {result}"
    );
}

#[test]
fn full_pipeline_nested_static() {
    let source = "<template><div><span>inner</span></div></template>";
    let result = run_full_pipeline(source);
    // Static nested elements should all be in one template
    assert!(
        result.contains("_template("),
        "Expected _template, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "Static content should have no effects, got: {result}"
    );
}

#[test]
fn full_pipeline_interpolation() {
    let source = "<template><div>{{ msg }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect for interpolation, got: {result}"
    );
    assert!(
        result.contains("_setText"),
        "Expected _setText for interpolation, got: {result}"
    );
    assert!(
        result.contains("_toDisplayString"),
        "Expected _toDisplayString, got: {result}"
    );
}

#[test]
fn html_minimization_simple_div() {
    // Vue 3.6: _template("<div>hello", true) — drops closing tag for last child
    let source = "<template><div>hello</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div>hello\""),
        "Expected minimized HTML '<div>hello', got: {result}"
    );
}

#[test]
fn html_minimization_empty_div() {
    // Vue 3.6: _template("<div>", true) — empty div drops closing tag
    let source = "<template><div></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div>\""),
        "Expected minimized HTML '<div>', got: {result}"
    );
}

#[test]
fn html_minimization_nested() {
    // Vue 3.6: _template("<div><span>inner", true) — drops all trailing close tags
    let source = "<template><div><span>inner</span></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div><span>inner\""),
        "Expected minimized HTML '<div><span>inner', got: {result}"
    );
}

#[test]
fn html_minimization_multiple_children() {
    // Vue 3.6: <div><span>a</span><span>b — only the last child's closing tag is dropped
    let source = "<template><div><span>a</span><span>b</span></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div><span>a</span><span>b\""),
        "Expected minimized HTML with last close tag dropped, got: {result}"
    );
}

#[test]
fn html_minimization_self_closing_br() {
    // Vue 3.6: _template("<br>", true) — no self-close slash
    let source = "<template><br/></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<br>\""),
        "Expected '<br>' (no self-close slash), got: {result}"
    );
}

#[test]
fn html_unquoted_attrs() {
    // Vue 3.6: _template("<div id=app>hi", true) — unquoted attrs when no spaces
    let source = "<template><div id=\"app\">hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<div id=app>hi\""),
        "Expected unquoted attr id=app, got: {result}"
    );
}

#[test]
fn html_unquoted_img_attrs() {
    // Vue 3.6: <img src=a.png alt=pic> — unquoted attrs
    let source = "<template><img src=\"a.png\" alt=\"pic\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"<img src=a.png alt=pic>\""),
        "Expected unquoted img attrs, got: {result}"
    );
}

#[test]
fn html_render_function_signature() {
    // Official `generate()`: non-inline vapor always gets the 5-param
    // signature — `bindingMetadata` defaults to `{}` for `vapor && !ssr`
    // (`compiler-sfc.cjs.js`; rc.5 script-less `slots.vue`).
    let source = "<template><div>hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("function render(_ctx, $props, $emit, $attrs, $slots)"),
        "Expected Vapor render signature 'function render(_ctx, $props, $emit, $attrs, $slots)', got: {result}"
    );
    assert!(
        !result.contains("_cache"),
        "Vapor render should not have _cache param, got: {result}"
    );
}

#[test]
fn html_set_attr_for_data_attributes() {
    // Vue 3.6: _setAttr for data-* attributes, not _setProp
    let source = "<template><div :data-id=\"id\">hi</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setAttr(n0, \"data-id\""),
        "Expected _setAttr for data-* attributes, got: {result}"
    );
}

#[test]
fn event_click_bare_uses_on_not_delegated() {
    // Official rc.5 `transformVOn`: delegation is opt-in via `.delegate`
    // (`isDelegatableEvent = !!delegateModifier && arg.isStatic &&
    // delegatedEvents(arg.content)`). Bare `@click="handler"` uses `_on()`
    // (`vue/props-emit__vapor__*`).
    let source = "<template><button @click=\"handler\">click</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_on(n0, \"click\", "),
        "Expected _on(n0, \"click\", ...) for a bare (unmodified) click handler, got: {result}"
    );
    assert!(
        !result.contains("_delegateEvents") && !result.contains("$evtclick") && !result.contains("_createInvoker"),
        "A bare click handler must NOT delegate without an explicit .delegate modifier, got: {result}"
    );
}

#[test]
fn event_click_delegate_modifier_delegates() {
    // Positive control: `.delegate` activates `_createInvoker` + `$evtclick` + `_delegateEvents`.
    let source = "<template><button @click.delegate=\"handler\">click</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_delegateEvents(\"click\")"),
        "Expected _delegateEvents, got: {result}"
    );
    assert!(
        result.contains("$evtclick"),
        "Expected $evtclick assignment, got: {result}"
    );
    assert!(
        result.contains("_createInvoker"),
        "Expected _createInvoker, got: {result}"
    );
}

#[test]
fn event_multiple_delegate_modifier_delegates() {
    // Multiple events, both explicitly opted into delegation.
    let source =
        "<template><button @click.delegate=\"a\" @mouseover.delegate=\"b\">hi</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_delegateEvents("),
        "Expected _delegateEvents, got: {result}"
    );
    assert!(
        result.contains("$evtclick"),
        "Expected $evtclick, got: {result}"
    );
    assert!(
        result.contains("$evtmouseover"),
        "Expected $evtmouseover, got: {result}"
    );
}

#[test]
fn event_multiple_bare_use_on_not_delegated() {
    // Negative control: without `.delegate`, all events bind via `_on()`.
    let source = "<template><button @click=\"a\" @mouseover=\"b\">hi</button></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_on(n0, \"click\", ") && result.contains("_on(n0, \"mouseover\", "),
        "Expected both events bound via _on(), got: {result}"
    );
    assert!(
        !result.contains("_delegateEvents")
            && !result.contains("$evtclick")
            && !result.contains("$evtmouseover"),
        "Bare handlers must not delegate, got: {result}"
    );
}

#[test]
fn v_show_simple() {
    // Vue 3.6: _applyVShow(n0, () => (_ctx.visible))
    let source = "<template><div v-show=\"visible\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyVShow(n0"),
        "Expected _applyVShow, got: {result}"
    );
}

#[test]
fn v_model_input() {
    // Vue 3.6: _applyTextModel(n0, () => (_ctx.text), _value => (_ctx.text = _value))
    let source = "<template><input v-model=\"text\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyTextModel(n0"),
        "Expected _applyTextModel, got: {result}"
    );
}

#[test]
fn v_model_checkbox() {
    // Vue 3.6: _applyCheckboxModel
    let source = "<template><input type=\"checkbox\" v-model=\"checked\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyCheckboxModel(n0"),
        "Expected _applyCheckboxModel, got: {result}"
    );
}

#[test]
fn v_model_with_trim_modifier() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { trim: true })
    let source = "<template><input v-model.trim=\"text\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_applyTextModel(n0"),
        "Expected _applyTextModel, got: {result}"
    );
    assert!(
        result.contains("trim: true"),
        "Expected trim modifier, got: {result}"
    );
}

#[test]
fn v_model_with_number_modifier() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { number: true })
    let source = "<template><input v-model.number=\"val\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("number: true"),
        "Expected number modifier, got: {result}"
    );
}

#[test]
fn v_model_with_multiple_modifiers() {
    // Vue 3.6: _applyTextModel(n0, getter, setter, { trim: true, number: true })
    let source = "<template><input v-model.trim.number=\"val\"></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("trim: true"),
        "Expected trim modifier, got: {result}"
    );
    assert!(
        result.contains("number: true"),
        "Expected number modifier, got: {result}"
    );
}

#[test]
fn v_html_directive() {
    // Vue 3.6: _renderEffect(() => _setHtml(n0, _ctx.rawHtml))
    let source = "<template><div v-html=\"rawHtml\"></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setHtml(n0"),
        "Expected _setHtml, got: {result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "Expected _renderEffect for v-html, got: {result}"
    );
}

#[test]
fn component_simple() {
    // A statically-resolved component uses _createComponent, never the
    // _createComponentWithFallback SSR/vdom-interop fallback helper --
    // verified against the real compiler.
    let source = "<template><my-element></my-element></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_resolveComponent(\"my-element\")"),
        "Expected _resolveComponent, got: {result}"
    );
    assert!(
        result.contains("_createComponent("),
        "Expected _createComponent, got: {result}"
    );
    assert!(
        !result.contains("_createComponentWithFallback("),
        "must not use the fallback helper for a plain component, got: {result}"
    );
}

#[test]
fn component_with_static_props() {
    // _createComponent(comp, { title: "hello" }, null, true)
    let source = "<template><my-comp title=\"hello\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createComponent("),
        "Expected _createComponent, got: {result}"
    );
    assert!(
        result.contains("title: \"hello\""),
        "Expected static title prop, got: {result}"
    );
}

#[test]
fn component_with_dynamic_props() {
    // Vue 3.6: _createComponent(comp, { title: () => (expr) })
    let source = "<template><my-comp :title=\"msg\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("title: () => (_ctx.msg)"),
        "Expected dynamic title prop with arrow fn, got: {result}"
    );
}

#[test]
fn component_with_event() {
    // Vue 3.6: _createComponent(comp, { onClick: () => handler })
    let source = "<template><my-comp @click=\"handler\"></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("onClick:"),
        "Expected onClick prop, got: {result}"
    );
}

#[test]
fn component_with_default_slot() {
    // Vue 3.6: _createComponent(comp, null, { default: () => { ... } })
    let source = "<template><my-comp><span>hello</span></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("default: () => {"),
        "Expected default slot closure, got: {result}"
    );
}

#[test]
fn slot_default_outlet() {
    // Official rc.5: unnamed slot with no fallback omits trailing defaults —
    // `_createSlot()`, not `_createSlot("default", null)`.
    let source = "<template><slot></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot()"),
        "Expected _createSlot() with all trailing default args omitted, got: {result}"
    );
}

#[test]
fn slot_named_outlet() {
    // Vue 3.6: _createSlot("header", null)
    let source = "<template><slot name=\"header\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot(\"header\""),
        "Expected _createSlot(\"header\"), got: {result}"
    );
}

#[test]
fn slot_outlet_dynamic_prop_wraps_in_getter() {
    let source = "<script setup>\ndefineProps({ count: { type: Number, default: 0 } })\n</script>\n<template><slot name=\"footer\" :total=\"count\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot(\"footer\", { total: () => ($props.count) })"),
        "Expected the dynamic prop wrapped in a getter, got: {result}"
    );
}

#[test]
fn slot_outlet_constant_prop_skips_getter() {
    let source = "<template><slot name=\"footer\" :total=\"42\" :label=\"'x'\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("total: 42") && !result.contains("total: () => (42)"),
        "Expected a numeric-literal prop unwrapped, got: {result}"
    );
    assert!(
        result.contains("label: 'x'") && !result.contains("() => ('x')"),
        "Expected a string-literal prop unwrapped, got: {result}"
    );
}

#[test]
fn slot_outlet_static_attr_becomes_prop() {
    let source = "<template><slot name=\"footer\" variant=\"outline\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot(\"footer\", { variant: \"outline\" })"),
        "Expected the static attribute passed through as a slot prop, got: {result}"
    );
}

#[test]
fn scoped_slot_outlet_spread_is_silently_dropped() {
    let source = "<script setup>\nimport { ref } from 'vue'\nconst extra = ref({ a: 1 })\n</script>\n<template><slot v-bind=\"extra\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createSlot()")
            && !result.contains("_ctx.extra")
            && !result.contains("$setup.extra"),
        "disclosed gap: a v-bind spread's data must currently vanish \
         entirely from the slot outlet, got: {result}"
    );
}

#[test]
fn scoped_slot_outlet_dynamic_key_emits_wrong_literal_key() {
    let source = "<script setup>\nimport { ref } from 'vue'\nconst key = ref('a')\nconst val = ref(1)\n</script>\n<template><slot :[key]=\"val\"></slot></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("[key]")
            && !result.contains("_ctx.key]:")
            && !result.contains("$setup.key]:"),
        "disclosed gap: a dynamic slot-outlet key must currently emit as a \
         bogus literal key, never the computed runtime key, got: {result}"
    );
}

#[test]
fn v_if_simple() {
    // Vue 3.6: _createIf(() => (_ctx.show), () => { ... })
    let source = "<template><div v-if=\"show\">yes</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
}

#[test]
fn v_if_else_chain() {
    // Vue 3.6: _createIf(() => (cond), () => {...}, () => {...})
    let source = "<template><div v-if=\"show\">yes</div><div v-else>no</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
    // v-else branch should be nested as third arg
    assert!(
        result.contains(", () => {"),
        "Expected v-else closure, got: {result}"
    );
    // Should only return one node (the whole chain is a single root)
    assert!(
        result.contains("return n0"),
        "Expected single return n0, got: {result}"
    );
}

#[test]
fn v_if_else_if_else_chain() {
    // Vue 3.6: _createIf(() => (a), () => {...}, () => _createIf(() => (b), () => {...}, () => {...}))
    let source = "<template><div v-if=\"a\">A</div><div v-else-if=\"b\">B</div><div v-else>C</div></template>";
    let result = run_full_pipeline(source);
    // Outer _createIf
    assert!(
        result.contains("_createIf("),
        "Expected _createIf, got: {result}"
    );
    // Nested _createIf for v-else-if
    let count = result.matches("_createIf(").count();
    assert_eq!(
        count, 2,
        "Expected 2 _createIf calls (one nested), got {count} in: {result}"
    );
    // Should only return one node
    assert!(
        result.contains("return n0"),
        "Expected single return n0, got: {result}"
    );
}

#[test]
fn v_for_simple() {
    // Vue 3.6: _createFor(() => (_ctx.items), (item) => { ... })
    let source = "<template><div v-for=\"item in items\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createFor("),
        "Expected _createFor, got: {result}"
    );
}

#[test]
fn v_for_preserves_user_params() {
    // Official v-for renames loop vars to `_for_item{depth}`/`_for_key{depth}`
    // (position 1 is always "key"). User names do not survive the signature.
    let source = "<template><div v-for=\"(item, index) in items\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("(_for_item0, _for_key0) => {"),
        "Expected renamed params (_for_item0, _for_key0), got: {result}"
    );
    assert!(
        !result.contains("(item, index) => {"),
        "the user's own variable names must not survive into the closure \
         signature, got: {result}"
    );
}

#[test]
fn v_for_with_key() {
    // Vue 3.6: _createFor(() => (items), (item) => {...}, (item) => (item.id))
    let source =
        "<template><div v-for=\"item in items\" :key=\"item.id\">{{ item }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_createFor("),
        "Expected _createFor, got: {result}"
    );
    assert!(
        result.contains("item.id"),
        "Expected :key expression item.id, got: {result}"
    );
}

#[test]
fn template_ref_static() {
    // Vue 3.6.0-rc.15 `genSetTemplateRef`: a static ref outside a loop calls
    // the imported `setStaticTemplateRef`, and no setter is declared, so no
    // undeclared `_setTemplateRef` may be called.
    let source = "<template><div ref=\"el\">content</div></template>";
    let output = assert_legacy(source);
    let result = &output.code;
    assert!(
        result.contains("_setStaticTemplateRef(n0, \"el\")"),
        "Expected _setStaticTemplateRef, got: {result}"
    );
    assert!(
        !result.contains("_setTemplateRef"),
        "Expected no ref setter, got: {result}"
    );
    assert!(
        output.helpers.contains(&"_setStaticTemplateRef"),
        "{:?}",
        output.helpers
    );
    assert!(
        !output.helpers.contains(&"_createTemplateRefSetter"),
        "{:?}",
        output.helpers
    );
}

#[test]
fn v_once_with_dynamic_binding() {
    // Vue 3.6: v-once effects become direct statements (no _renderEffect)
    let source = "<template><div v-once :id=\"foo\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setProp(n0, \"id\""),
        "Expected _setProp for v-once dynamic binding, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-once should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_once_with_interpolation() {
    // Vue 3.6: v-once interpolation is direct _setText (no _renderEffect)
    let source = "<template><div v-once>{{ msg }}</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setText("),
        "Expected _setText for v-once interpolation, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-once should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_memo_static() {
    // Vue 3.6: v-memo with static content is a no-op (just static template)
    let source = "<template><div v-memo=\"[dep]\">content</div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_template("),
        "Expected _template for v-memo static content, got: {result}"
    );
    assert!(
        !result.contains("_renderEffect"),
        "v-memo static should NOT have _renderEffect, got: {result}"
    );
}

#[test]
fn v_memo_dynamic() {
    // v-memo with dynamic content wraps render effect in _withMemo
    let source = r#"<template><div v-memo="[x]" :class="cls">{{ msg }}</div></template>"#;
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_withMemo([x]"),
        "Expected _withMemo([x]) for v-memo dynamic content, got: {result}"
    );
    assert!(
        result.contains("_renderEffect"),
        "v-memo dynamic should have _renderEffect wrapper, got: {result}"
    );
    assert!(
        result.contains("_cache"),
        "v-memo should reference _cache, got: {result}"
    );
    assert!(
        !result.contains("v-memo"),
        "v-memo attribute must not appear in output, got: {result}"
    );
}

#[test]
fn non_root_component_emits_insertion_state() {
    let source = "<template><div><my-comp></my-comp></div></template>";
    let result = render_code(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root component, got: {result}"
    );
    assert!(
        result.contains("_createComponent("),
        "Expected _createComponent, got: {result}"
    );
    // The parent div should still have a template
    assert!(
        result.contains("_template("),
        "Expected _template for parent div, got: {result}"
    );
}

#[test]
fn non_root_slot_emits_insertion_state() {
    let source = "<template><div><slot></slot></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root slot, got: {result}"
    );
    // Unnamed fallback-less slot omits trailing defaults; only content of the
    // div, so insertion is 1-arg append.
    assert!(
        result.contains("_createSlot()"),
        "Expected _createSlot() with all trailing default args omitted, got: {result}"
    );
}

#[test]
fn non_root_named_slot_emits_insertion_state() {
    let source = "<template><div><slot name=\"header\"></slot></div></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState for non-root named slot, got: {result}"
    );
    assert!(
        result.contains("_createSlot(\"header\""),
        "Expected _createSlot(\"header\"), got: {result}"
    );
}

#[test]
fn non_root_component_after_sibling() {
    let source = "<template><div><span>text</span><my-comp></my-comp></div></template>";
    let result = render_code(source);
    assert!(
        result.contains("_setInsertionState("),
        "Expected _setInsertionState, got: {result}"
    );
    // Second child (index 1), mounted once — numeric index, no `<!>`.
    // Official rc.5: `_setInsertionState(n2, 2)`, not the former 4-arg call.
    assert!(
        result.contains(", 1)"),
        "Expected child index 1 for component after span, got: {result}"
    );
    assert!(
        !result.contains("<!>"),
        "a mounted-once component never needs a persistent anchor, got: {result}"
    );
}

#[test]
fn component_with_named_slot() {
    let source =
        "<template><my-comp><template #header><span>Header</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"header\": () => {"),
        "Expected named slot 'header' closure, got: {result}"
    );
    assert!(
        !result.contains("\"default\": () => {"),
        "Should not have default slot when only named slot is present, got: {result}"
    );
}

#[test]
fn component_with_multiple_named_slots() {
    let source = "<template><my-comp><template #header><span>Header</span></template><template #footer><span>Footer</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"header\": () => {"),
        "Expected named slot 'header', got: {result}"
    );
    assert!(
        result.contains("\"footer\": () => {"),
        "Expected named slot 'footer', got: {result}"
    );
    assert!(
        !result.contains("_: 2"),
        "Vapor's plain named-slots object must not carry a VDOM-style \
         stability marker, got: {result}"
    );
}

#[test]
fn component_with_named_and_default_slots() {
    let source = "<template><my-comp><template #header><span>Header</span></template><span>Default content</span></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"header\": () => {"),
        "Expected named slot 'header', got: {result}"
    );
    assert!(
        result.contains("\"default\": () => {"),
        "Expected implicit default slot, got: {result}"
    );
}

#[test]
fn component_with_scoped_slot() {
    let source = "<template><my-comp><template #default=\"{ item }\"><span>{{ item }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"default\": (_slotProps0) => {"),
        "Expected the destructured param collapsed to _slotProps0, got: {result}"
    );
    assert!(
        result.contains("_slotProps0.item"),
        "Expected the destructured `item` rewritten to _slotProps0.item, got: {result}"
    );
}

#[test]
fn component_with_named_scoped_slot() {
    let source = "<template><my-comp><template #header=\"{ title }\"><span>{{ title }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"header\": (_slotProps0) => {"),
        "Expected the destructured param collapsed to _slotProps0, got: {result}"
    );
    assert!(
        result.contains("_slotProps0.title"),
        "Expected the destructured `title` rewritten to _slotProps0.title, got: {result}"
    );
}

#[test]
fn component_with_scoped_slot_rest_element() {
    let source = "<template><my-comp><template #default=\"{ id, ...rest }\"><span>{{ id }}-{{ rest.x }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"default\": (_slotProps0) => {"),
        "Expected the destructured param collapsed to _slotProps0, got: {result}"
    );
    assert!(
        result.contains("_slotProps0.id"),
        "Expected the sibling leaf rewritten to _slotProps0.id, got: {result}"
    );
    assert!(
        result.contains("_getRestElement(_slotProps0, [\"id\"]).x"),
        "Expected the rest binding to resolve via _getRestElement, got: {result}"
    );
}

#[test]
fn component_with_scoped_slot_default_value() {
    let source = "<template><my-comp><template #default=\"{ id = 99 }\"><span>{{ id }}</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"default\": (_slotProps0) => {"),
        "Expected the destructured param collapsed to _slotProps0, got: {result}"
    );
    assert!(
        result.contains("_getDefaultValue(_slotProps0.id, () => (99))"),
        "Expected the defaulted binding to resolve via _getDefaultValue, got: {result}"
    );
}

#[test]
fn component_with_bare_v_slot() {
    let source =
        "<template><my-comp><template v-slot><span>Content</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"default\": () => {"),
        "Expected bare v-slot to produce default slot, got: {result}"
    );
}

#[test]
fn component_with_hyphenated_slot_name() {
    let source = "<template><my-comp><template #my-slot><span>Content</span></template></my-comp></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("\"my-slot\": () => {"),
        "Expected quoted hyphenated slot name, got: {result}"
    );
}

#[test]
fn builtin_component_transition() {
    let source = "<template><Transition name=\"fade\"><div>content</div></Transition></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_Transition"),
        "Expected _Transition helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

#[test]
fn builtin_component_keep_alive() {
    let source = "<template><KeepAlive><div>content</div></KeepAlive></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_VaporKeepAlive"),
        "Expected _VaporKeepAlive helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

#[test]
fn builtin_component_teleport() {
    let source = "<template><Teleport to=\"body\"><div>content</div></Teleport></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_VaporTeleport"),
        "Expected _VaporTeleport helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

#[test]
fn builtin_component_suspense() {
    let source = "<template><Suspense><div>content</div></Suspense></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_Suspense"),
        "Expected _Suspense helper, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

#[test]
fn builtin_component_kebab_case_keep_alive() {
    let source = "<template><keep-alive><div>content</div></keep-alive></template>";
    let result = run_full_pipeline(source);
    assert!(
        result.contains("_VaporKeepAlive"),
        "Expected _VaporKeepAlive for kebab-case keep-alive, got: {result}"
    );
    assert!(
        !result.contains("_resolveComponent"),
        "Built-in should not use _resolveComponent, got: {result}"
    );
}

#[test]
fn dom_child_index_counts_preceding_siblings_exactly() {
    // runtime-vapor 3.6.0-rc.15 `dom/node.ts`: `next(node, isText)` is
    // `node.nextSibling` (`isText` is only a hydration flag) and
    // `nthChild(node, i)` is `node.childNodes[i]`. A child at DOM index k >= 2
    // without a navigated previous sibling is reached as `_nthChild(parent, k)`,
    // as Vue's `genAccessPath` writes it.
    // A dynamic element after three static element siblings → DOM index 3.
    // Dynamic 4th span text ref consumes id 0 (dynamic-before-nav), so root is `n1`.
    let r = run_full_pipeline(
        "<template><div><span>0</span><span>1</span><span>2</span><span>{{ d }}</span></div></template>",
    );
    assert!(
        r.contains("= _nthChild(n1, 3)"),
        "dynamic 4th element child must be DOM index 3, got:\n{r}"
    );
    assert!(
        !r.contains("_next(n1"),
        "`next` takes a sibling, never a parent and an index, got:\n{r}"
    );
    assert!(
        !r.contains("_nthChild(n1, 2)") && !r.contains("_nthChild(n1, 4)"),
        "DOM index off-by-one for the 4th child, got:\n{r}"
    );

    // span(0) comment(1) p(2): an enabled comment advances the index.
    let r =
        run_full_pipeline("<template><div><span>a</span><!-- c --><p>{{ b }}</p></div></template>");
    assert!(
        r.contains("= _nthChild(n1, 2)"),
        "an enabled comment must advance the DOM index to 2, got:\n{r}"
    );

    // Component after two siblings: numeric index 2 (rc.5 2-arg form).
    let r = render_code("<template><div><a>x</a><b>y</b><c>{{ z }}</c></div></template>");
    assert!(
        r.contains("_setInsertionState(n1, 2)"),
        "component after two siblings must insert at DOM index 2, got:\n{r}"
    );

    // Coalescing: a leading text+interpolation run is ONE child, so the
    // following element is index 1 (not 2). `<div>t{{i}}<span>{{d}}</span></div>`
    // → the span is DOM child 1, the first child's next sibling.
    let r = run_full_pipeline("<template><div>t{{ i }}<span>{{ d }}</span></div></template>");
    assert!(
        r.contains("= _next(_child(n0))"),
        "a leading text+interpolation run coalesces to one child; the span must be DOM index 1, got:\n{r}"
    );
}

#[test]
fn root_element_own_interpolation_gets_txt_statement_and_imports() {
    let result = run_full_pipeline("<template><p>{{ msg }}</p></template>");
    assert!(
        result.contains("const x0 = _txt(n0)"),
        "root's own interpolation must get its own _txt() extraction statement, got:\n{result}"
    );
    assert!(
        result.contains("_setText(x0, _toDisplayString("),
        "got:\n{result}"
    );
    // `run_full_pipeline` returns template code only; Txt/SetText imports are
    // covered by the seed-matrix runtime axis and `finalize_root_element`.
}

#[test]
fn root_element_own_interpolation_plus_dynamic_prop_gets_txt_statement() {
    // Same defect with a sibling dynamic prop (`:disabled` + interpolation).
    let result =
        run_full_pipeline("<template><button :disabled=\"d\">{{ msg }}</button></template>");
    assert!(result.contains("const x0 = _txt(n0)"), "got:\n{result}");
    assert!(
        result.contains("_setProp(n0, \"disabled\""),
        "got:\n{result}"
    );
    assert!(
        result.contains("_setText(x0, _toDisplayString("),
        "got:\n{result}"
    );
}

#[test]
fn nested_slot_forwards_through_plain_wrapper_element() {
    let result = run_full_pipeline(
        "<template><div class=\"panel\"><header><slot name=\"header\">Untitled</slot></header><main><slot /></main></div></template>",
    );
    // Fallback ("Untitled") gets its own hoisted template, like a v-if branch.
    assert!(
        result.contains(r#"_template("Untitled""#),
        "fallback content must hoist to its own template, got:\n{result}"
    );
    // Each wrapper's only content is its slot — 1-arg append, no `<!>`.
    assert!(
        !result.contains("<!>"),
        "neither slot needs an anchor (each is the sole content of its \
         wrapper) — got:\n{result}"
    );
    assert!(result.contains("_createSlot("), "got:\n{result}");
    assert!(result.contains("_setInsertionState("), "got:\n{result}");
    // Skeleton is whitespace-clean: `<div class=panel><header></header><main>`.
    assert!(
        result.contains(r#""<div class=panel><header></header><main>""#),
        "inter-tag whitespace must be stripped (WhitespaceNewline), got:\n{result}"
    );
    // Unnamed fallback-less default slot: `_createSlot()`.
    assert!(
        result.contains("_createSlot()"),
        "an unnamed slot with no fallback must omit all trailing default \
         args, got:\n{result}"
    );
}

#[test]
fn nested_v_if_else_splits_into_separate_anchored_templates() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><p v-if=\"a\">A</p><p v-else>zero</p><ul></ul></div></template><script setup>const a = 1;</script>",
    );
    assert!(
        result.contains(r#"_template("<p>A""#),
        "the v-if branch must hoist to its own template, got:\n{result}"
    );
    assert!(
        result.contains(r#"_template("<p>zero""#),
        "the v-else branch must hoist to its own template, got:\n{result}"
    );
    // Followed by `<ul>` → skeleton is `<!>` there, not either branch's HTML.
    assert!(
        result.contains(r#""<div class=root><!><ul>""#),
        "the ancestor's own skeleton must contain just the `<!>` anchor \
         (branches split into their own templates above), got:\n{result}"
    );
    assert!(result.contains("_createIf("), "got:\n{result}");
    assert!(result.contains("_setInsertionState("), "got:\n{result}");
}

#[test]
fn nested_v_if_else_without_following_sibling_needs_no_anchor() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><p v-if=\"a\">A</p><p v-else>zero</p></div></template><script setup>const a = 1;</script>",
    );
    assert!(
        !result.contains("<!>"),
        "the v-if chain is the div's ONLY content — no anchor needed, got:\n{result}"
    );
    assert!(result.contains("_createIf("), "got:\n{result}");
}

#[test]
fn nested_v_for_with_key_does_not_double_handle_key_as_a_prop() {
    let result = run_full_pipeline(
        "<template><div class=\"root\"><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></div></template><script setup>const items = [];</script>",
    );
    assert!(
        !result.contains("_setProp(") || !result.contains("\"key\""),
        "`:key` must never be emitted as a _setProp call, got:\n{result}"
    );
    assert!(
        result.contains("(item) => (item)"),
        "the key must still reach _createFor's trailing key callback, got:\n{result}"
    );
    assert!(result.contains("_createFor("), "got:\n{result}");
}

#[test]
fn nested_v_for_source_rewrites_outer_loop_variable_through_member_access() {
    let result = run_full_pipeline(
        "<template><div><li v-for=\"item in items\"><p v-if=\"item.show\">\
         <span v-for=\"tag in item.tags\">{{ tag }}</span></p></li></div></template>\
         <script setup>const items = [];</script>",
    );
    assert!(
        result.contains("_createFor(() => (_for_item0.value.tags)"),
        "the inner v-for's source must rewrite the outer loop variable through the \
         member-access chain exactly like official rc.5, got:\n{result}"
    );
    assert!(
        !result.contains("_createFor(() => (item.tags)"),
        "the outer loop variable `item` must never reach generated code unrenamed — a \
         raw `item.tags` reference is a runtime ReferenceError, got:\n{result}"
    );
    assert!(
        result.contains("_for_item0.value.show"),
        "the v-if condition's own outer-loop-variable member access must also rewrite \
         (already covered by the existing in-body mechanism), got:\n{result}"
    );
    // Bound the INNER `_createFor` args with a paren scan — a fixed-width
    // window can still contain the OUTER call's `9` flags.
    let inner_call = isolate_call(&result, "_createFor(() => (_for_item0.value.tags)");
    assert!(
        inner_call.contains("9 /* FAST_REMOVE, IS_SINGLE_NODE */"),
        "the inner v-for is still official's sole-child of the `<p v-if>` for \
         onlyChild/FAST_REMOVE purposes — a v-if parent alone must not disqualify it \
         (official emits flags 9, not 8), got the inner _createFor call's own text \
         (bounded to exactly this call, not the outer one):\n{inner_call}\n\nfull output:\n{result}"
    );
    assert!(
        !inner_call.contains("8 /* IS_SINGLE_NODE */"),
        "the inner call's own flags regressed to 8 (FAST_REMOVE lost), got:\n{inner_call}"
    );
}

#[test]
fn quadruple_nested_v_for_v_if_v_for_matches_official_nesting_and_flags() {
    let result = run_full_pipeline(
        "<template><div><section v-for=\"group in groups\"><li v-for=\"item in group.items\">\
         <p v-if=\"item.show\"><span v-for=\"tag in item.tags\">{{ tag }}</span></p></li>\
         </section></div></template><script setup>const groups = [];</script>",
    );

    // Level 1: outer v-for over `groups`. Its own call text must contain
    // EVERY inner construct (proving the whole chain nests inside it) and
    // its own flags (bounded to itself, not a descendant's).
    let outer_for = isolate_call(&result, "_createFor(() => (_ctx.groups)");
    assert!(
        outer_for.contains("_createFor(() => (_for_item0.value.items)"),
        "level 2 must nest inside level 1's own callback, got:\n{outer_for}"
    );
    assert!(
        outer_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 1's own trailing flags must be its own, got:\n{outer_for}"
    );

    // Level 2: v-for over `group.items`, source rewritten through the
    // level-1 loop variable.
    let level2_for = isolate_call(&result, "_createFor(() => (_for_item0.value.items)");
    assert!(
        level2_for.contains("_createIf(() => (_for_item1.value.show)"),
        "level 3 (v-if) must nest inside level 2's own callback, got:\n{level2_for}"
    );
    assert!(
        level2_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 2's own trailing flags must be its own, got:\n{level2_for}"
    );

    // Level 3: v-if, condition rewritten through the level-2 loop variable.
    let level3_if = isolate_call(&result, "_createIf(() => (_for_item1.value.show)");
    assert!(
        level3_if.contains("_createFor(() => (_for_item1.value.tags)"),
        "level 4 (inner v-for) must nest inside the v-if's own branch body, got:\n{level3_if}"
    );

    // Level 4: innermost v-for, source rewritten through the level-2 loop
    // variable (the v-if branch's own scope carries no NEW loop variable).
    let level4_for = isolate_call(&result, "_createFor(() => (_for_item1.value.tags)");
    assert!(
        level4_for.contains("_for_item2) => {"),
        "level 4's own callback param must be the depth-2 rename, got:\n{level4_for}"
    );
    assert!(
        level4_for
            .trim_end()
            .ends_with("9 /* FAST_REMOVE, IS_SINGLE_NODE */)"),
        "level 4's own trailing flags must be its own, got:\n{level4_for}"
    );
}

#[test]
fn bare_static_v_if_with_no_scope_emits_explicit_null_negative_placeholder() {
    let result = run_full_pipeline("<template><div v-if=\"a\">static</div></template>");
    assert!(
        result.contains(", null, "),
        "a bare NO_SCOPE-eligible v-if must emit an explicit `null` for the \
         skipped negative argument before the flags, got:\n{result}"
    );
}

#[test]
fn dynamic_id_ordering_if_construct_own_id_precedes_branch_content_ids() {
    let r = run_full_pipeline(
        "<template><div v-if=\"a\">A {{ x }}</div><div v-else>B</div></template>",
    );
    let (construct_id, pos) = first_id_after(&r, 0, "const n");
    let (true_content_id, pos) = first_id_after(&r, pos, "const n");
    let (false_content_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        construct_id < true_content_id,
        "if-construct's own id ({construct_id}) must precede the true branch's \
         content id ({true_content_id}), got:\n{r}"
    );
    assert!(
        true_content_id < false_content_id,
        "true-branch content id ({true_content_id}) must precede the false \
         branch's content id ({false_content_id}), got:\n{r}"
    );
}

#[test]
fn dynamic_id_ordering_for_construct_own_id_precedes_item_content_id() {
    let r = run_full_pipeline("<template><li v-for=\"item in items\">{{ item }}</li></template>");
    let (construct_id, pos) = first_id_after(&r, 0, "const n");
    let (item_content_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        construct_id < item_content_id,
        "for-construct's own id ({construct_id}) must precede the item's own \
         content id ({item_content_id}), got:\n{r}"
    );
}

#[test]
fn dynamic_id_ordering_nested_v_if_inside_v_for() {
    let r = run_full_pipeline(
        "<template><li v-for=\"item in items\"><span v-if=\"item.ok\">{{ item.label }}</span><span v-else>skip</span></li></template>",
    );
    let (for_id, _) = construct_own_id_from(&r, 0, "_createFor(");
    let (if_id, if_call_pos) = construct_own_id_from(&r, 0, "_createIf(");
    assert!(
        for_id < if_id,
        "outer for-construct's own id ({for_id}) must precede the nested \
         if-construct's own id ({if_id}), got:\n{r}"
    );

    // The nested if's own id must precede ITS branch content, searched
    // starting right after the if's own call.
    let (true_id, pos) = first_id_after(&r, if_call_pos, "const n");
    let (false_id, _) = first_id_after(&r, pos, "const n");
    assert!(
        if_id < true_id && true_id < false_id,
        "nested if-construct's own id ({if_id}) must precede its branch \
         content ids ({true_id}, {false_id}) in order, got:\n{r}"
    );
}

#[test]
fn dynamic_id_ordering_sibling_if_constructs_do_not_collide() {
    let r = run_full_pipeline(
        "<template><div><p v-if=\"a\">{{ x }}</p><p v-else>zero</p><span v-if=\"b\">{{ y }}</span><span v-else>zero2</span></div></template>",
    );
    let (first_if, first_call_pos) = construct_own_id_from(&r, 0, "_createIf(");
    let (first_true, pos) = first_id_after(&r, first_call_pos, "const n");
    let (first_false, pos) = first_id_after(&r, pos, "const n");
    assert!(
        first_if < first_true && first_true < first_false,
        "first if-construct's own id ({first_if}) must precede its branch \
         content ({first_true}, {first_false}) in order, got:\n{r}"
    );

    let (second_if, second_call_pos) = construct_own_id_from(&r, pos, "_createIf(");
    let (second_true, pos2) = first_id_after(&r, second_call_pos, "const n");
    let (second_false, _) = first_id_after(&r, pos2, "const n");
    assert!(
        second_if < second_true && second_true < second_false,
        "second if-construct's own id ({second_if}) must precede its branch \
         content ({second_true}, {second_false}) in order, got:\n{r}"
    );
    assert!(
        first_if != second_if && first_true != second_true && first_false != second_false,
        "sibling if-constructs must never reuse an id, got first=({first_if}, \
         {first_true}, {first_false}) second=({second_if}, {second_true}, \
         {second_false}) from:\n{r}"
    );
}

#[test]
fn v_on_bare_setup_const_function_reference_is_not_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nfunction onClick() {}\n</script>\n<template><button @click=\"onClick\">x</button></template>",
    );
    assert!(
        r.contains("_on(n0, \"click\", _ctx.onClick)"),
        "a bare SETUP_CONST (function declaration) handler reference must \
         be emitted BARE, never wrapped in `e => ...(e)`, got:\n{r}"
    );
    assert!(
        !r.contains("e => _ctx.onClick(e)"),
        "got an unexpected arrow-wrapped handler:\n{r}"
    );
}

#[test]
fn v_on_bare_non_const_reference_is_still_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst onClick = ref(() => {})\n</script>\n<template><button @click=\"onClick\">x</button></template>",
    );
    assert!(
        r.contains("e => _ctx.onClick.value(e)") || r.contains("e => _ctx.onClick(e)"),
        "a non-const (ref-backed) handler reference must still be \
         arrow-wrapped, got:\n{r}"
    );
}

#[test]
fn v_on_dotted_member_expression_is_always_wrapped() {
    let r = run_full_pipeline(
        "<script setup>\nfunction useHandlers() { return { onClick() {} } }\nconst handlers = useHandlers()\n</script>\n<template><button @click=\"handlers.onClick\">x</button></template>",
    );
    assert!(
        r.contains("e => _ctx.handlers.onClick(e)"),
        "a dotted member-expression handler must always be arrow-wrapped \
         regardless of the root binding's own type, got:\n{r}"
    );
}

#[test]
fn root_element_statements_are_emitted_before_effects() {
    let r = run_full_pipeline(
        "<template><button :disabled=\"disabled\" @click=\"onClick\">{{ label }}</button></template>",
    );
    let on_pos = r
        .find("_on(")
        .expect("expected an _on(...) call in the output");
    let effect_pos = r
        .find("_renderEffect(")
        .expect("expected a _renderEffect(...) call in the output");
    assert!(
        on_pos < effect_pos,
        "the _on(...) statement must be emitted BEFORE the _renderEffect(...) \
         block, regardless of :disabled being written before @click in \
         source, got:\n{r}"
    );
}

#[test]
fn root_own_ref_deferred_past_later_sibling_subtree() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\nconst items = ['a', 'b', 'c']\n</script>\n<template>\n  <div class=\"root\">\n    <p v-if=\"count > 0\">{{ count }}</p>\n    <p v-else>zero</p>\n    <ul>\n      <li v-for=\"item in items\" :key=\"item\">{{ item }}</li>\n    </ul>\n  </div>\n</template>",
    );
    assert!(
        r.contains("const n10 = t3()"),
        "root's own ref must be the HIGHEST id (10), allocated only after \
         the trailing <ul>/v-for subtree consumed ids 5-8, got:\n{r}"
    );
    assert!(
        r.contains("const n9 = _child(n10)"),
        "the if-block's anchor (9) must be minted BEFORE root's own ref \
         (10) within the same establishment — anchor first, container \
         second, matching official's real allocation order, got:\n{r}"
    );
    assert!(
        r.contains("const n8 = _next(n9)"),
        "<ul>'s own established ref (8) must chain from the anchor (9), \
         got:\n{r}"
    );
    assert!(
        r.contains("const n5 = _createFor("),
        "the v-for construct's own id (5) must come right after the if-\
         chain's ids (0-4) are fully consumed, unaffected by root's own \
         ref timing, got:\n{r}"
    );
    assert!(
        r.contains("const n0 = _createIf("),
        "the if construct's own id must stay 0 (first allocated), got:\n{r}"
    );
    // Negative: eager mint would give root id 5, colliding with the for-construct.
    assert!(
        !r.contains("const n5 = t3()"),
        "root's own ref must not be prematurely minted as id 5, got:\n{r}"
    );
}

#[test]
fn wrapping_elements_own_refs_deferred_past_later_sibling() {
    let r = run_full_pipeline(
        "<template>\n  <div class=\"panel\">\n    <header>\n      <slot name=\"header\">Untitled</slot>\n    </header>\n    <main>\n      <slot />\n    </main>\n  </div>\n</template>",
    );
    assert!(
        r.contains("const n6 = t1()"),
        "root's own ref must be the HIGHEST id (6), allocated only after \
         BOTH <header>'s and <main>'s own subtrees are fully consumed, \
         got:\n{r}"
    );
    assert!(
        r.contains("const n3 = _child(n6)"),
        "<header>'s own established ref (3) must chain from root (6), \
         got:\n{r}"
    );
    assert!(
        r.contains("const n5 = _next(n3)"),
        "<main>'s own established ref (5) must chain from <header> (3), \
         not be minted before it, got:\n{r}"
    );
    assert!(
        r.contains("const n0 = _createSlot(\"header\""),
        "the header slot's own construct id must stay 0 (first \
         allocated), got:\n{r}"
    );
    assert!(
        r.contains("const n4 = _createSlot()"),
        "the default slot's own construct id must be 4, right after the \
         header slot's own subtree (0-3) is fully consumed, got:\n{r}"
    );
}

#[test]
fn v_if_nested_in_plain_wrapper_still_gets_false_no_scope() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div class=\"root\"><p v-if=\"count > 0\">{{ count }}</p><p v-else>zero</p></div></template>",
    );
    assert!(
        r.contains("FALSE_NO_SCOPE"),
        "a v-if nested inside a plain wrapping element (not another \
         block-creating construct) must still be NO_SCOPE-eligible, got:\n{r}"
    );
}

#[test]
fn v_if_nested_in_v_for_still_denied_no_scope() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref([1])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\"><span v-if=\"item\">a</span><span v-else>b</span></li></ul></template>",
    );
    assert!(
        !r.contains("NO_SCOPE"),
        "a v-if nested inside a v-for's own item body must never be \
         NO_SCOPE-eligible (neither TRUE_ nor FALSE_), got:\n{r}"
    );
}

#[test]
fn v_for_sole_child_of_plain_wrapper_gets_fast_remove_and_single_node() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("9 /* FAST_REMOVE, IS_SINGLE_NODE */"),
        "sole v-for child of a plain wrapper must get FAST_REMOVE + \
         IS_SINGLE_NODE (9), got:\n{r}"
    );
}

#[test]
fn v_for_with_sibling_omits_fast_remove() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li><li>static</li></ul></template>",
    );
    assert!(
        r.contains("8 /* IS_SINGLE_NODE */"),
        "a v-for with a sibling must get IS_SINGLE_NODE only (8), no \
         FAST_REMOVE, got:\n{r}"
    );
    assert!(
        !r.contains("FAST_REMOVE"),
        "FAST_REMOVE must not fire when the v-for has a sibling, got:\n{r}"
    );
}

#[test]
fn v_for_item_renamed_and_unwrapped_in_body() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a', 'b', 'c'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("(_for_item0) => {"),
        "the main closure's own param must be renamed to _for_item0, got:\n{r}"
    );
    assert!(
        r.contains("_toDisplayString(_for_item0.value)"),
        "every in-body reference to the loop variable must be rewritten to \
         _for_item0.value, got:\n{r}"
    );
    assert!(
        !r.contains("_toDisplayString(item)"),
        "the raw loop-variable name must not survive inside the body, got:\n{r}"
    );
}

#[test]
fn v_for_key_callback_stays_unrenamed() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a', 'b', 'c'])\n</script>\n<template><ul><li v-for=\"item in items\" :key=\"item\">{{ item }}</li></ul></template>",
    );
    assert!(
        r.contains("(item) => (item)"),
        "the :key callback must keep the raw, unrenamed loop-variable name, got:\n{r}"
    );
}

#[test]
fn v_for_object_pattern_destructures_to_path_access() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst users = ref([{ id: 1, name: 'a' }])\n</script>\n<template><ul><li v-for=\"{ id, name } in users\" :key=\"id\">{{ name }}</li></ul></template>",
    );
    assert!(
        r.contains("(_for_item0) => {"),
        "the main closure's own param must rename to _for_item0 even for a \
         destructured value, got:\n{r}"
    );
    assert!(
        r.contains("_toDisplayString(_for_item0.value.name)"),
        "the destructured `name` must rewrite to _for_item0.value.name, got:\n{r}"
    );
    assert!(
        !r.contains("_toDisplayString(name)") && !r.contains("_ctx.name"),
        "the raw destructured name must not survive as an unrewritten/\
         _ctx-prefixed reference, got:\n{r}"
    );
    // The `:key` callback keeps the RAW, unrenamed pattern text (official
    // `genCallback`/`genSimpleIdMap` never renames there — same rule as the
    // bare-identifier case).
    assert!(
        r.contains("({ id, name }) => (id)"),
        "the :key callback must keep the raw destructuring pattern, got:\n{r}"
    );
}

#[test]
fn v_for_array_pattern_destructures_to_index_access() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst pairs = ref([[1, 2]])\n</script>\n<template><ul><li v-for=\"[a, b] in pairs\">{{ a }} {{ b }}</li></ul></template>",
    );
    assert!(r.contains("(_for_item0) => {"), "got:\n{r}");
    assert!(
        r.contains("_for_item0.value[0]") && r.contains("_for_item0.value[1]"),
        "array-pattern elements must rewrite to positional index access, got:\n{r}"
    );
}

#[test]
fn v_for_nested_object_pattern_accumulates_path() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst list = ref([{ user: { name: 'x' } }])\n</script>\n<template><ul><li v-for=\"{ user: { name } } in list\">{{ name }}</li></ul></template>",
    );
    assert!(
        r.contains("_for_item0.value.user.name"),
        "a nested destructure must accumulate a multi-hop path, got:\n{r}"
    );
}

#[test]
fn v_for_rest_element_pattern_destructures_via_get_rest_element() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst list = ref([{ id: 1, x: 2 }])\n</script>\n<template><ul><li v-for=\"{ id, ...rest } in list\">{{ id }}-{{ rest.x }}</li></ul></template>",
    );
    assert!(
        r.contains("(_for_item0) => {"),
        "the main closure's own param must rename to _for_item0 even for a \
         rest-element pattern, got:\n{r}"
    );
    assert!(
        r.contains("_for_item0.value.id"),
        "the plain sibling leaf must still resolve to a path access, got:\n{r}"
    );
    assert!(
        r.contains("_getRestElement(_for_item0.value, [\"id\"]).x"),
        "the rest binding must resolve via _getRestElement, excluding its \
         declared sibling keys, got:\n{r}"
    );
    assert!(
        !r.contains("{ id, ...rest }"),
        "the raw authored pattern must NOT survive as the closure's own \
         param once destructuring is attempted, got:\n{r}"
    );
}

#[test]
fn v_for_array_default_value_destructures_via_get_default_value() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst list = ref([[undefined, 2]])\n</script>\n<template><ul><li v-for=\"[a = 99, b] in list\">{{ a }}-{{ b }}</li></ul></template>",
    );
    assert!(
        r.contains("(_for_item0) => {"),
        "the main closure's own param must rename to _for_item0 even for a \
         default-value pattern, got:\n{r}"
    );
    assert!(
        r.contains("_getDefaultValue(_for_item0.value[0], () => (99))"),
        "the defaulted binding must resolve via _getDefaultValue, got:\n{r}"
    );
    assert!(
        !r.contains("_ctx.a"),
        "must not fall through to _ctx.a, got:\n{r}"
    );
}

#[test]
fn v_for_object_shorthand_default_value_stays_disclosed_gap() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst list = ref([{}])\n</script>\n<template><ul><li v-for=\"{ id = 99 } in list\">{{ id }}</li></ul></template>",
    );
    assert!(
        r.contains("_ctx.id"),
        "disclosed gap: an object-shorthand default in v-for still falls \
         through to _ctx.id (the v-for LHS parses as a plain Expression, \
         which rejects `id = 99` as an object-literal property value) — if \
         this now resolves correctly, the v-for LHS parser has gained \
         binding-pattern support and this test (and its doc comment) should \
         be replaced with a real destructuring assertion, got:\n{r}"
    );
}

#[test]
fn v_for_sibling_loops_both_get_depth_zero() {
    let r = run_full_pipeline(
        "<script setup>\nimport { ref } from 'vue'\nconst as_ = ref(['a'])\nconst bs = ref(['b'])\n</script>\n<template><div><ul><li v-for=\"a in as_\">{{ a }}</li></ul><ol><li v-for=\"b in bs\">{{ b }}</li></ol></div></template>",
    );
    assert!(
        r.contains("(_for_item0) => {") && r.matches("(_for_item0) => {").count() == 2,
        "sibling v-for loops must BOTH get depth 0 (the counter returns to \
         0 between them, not a running total), got:\n{r}"
    );
    assert!(
        !r.contains("_for_item1"),
        "no sibling v-for should ever reach depth 1, got:\n{r}"
    );
}
fn isolate_call<'a>(text: &'a str, needle: &str) -> &'a str {
    let start = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found in:\n{text}"));
    let open_paren = start + needle.find('(').expect("needle names a call");
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[open_paren..].iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[start..open_paren + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("{needle:?}'s call has no matching close paren in:\n{text}");
}

fn first_id_after(haystack: &str, from: usize, prefix: &str) -> (u32, usize) {
    let rest = &haystack[from..];
    let marker_pos = rest
        .find(prefix)
        .unwrap_or_else(|| panic!("expected to find `{prefix}` in:\n{haystack}"));
    let digits_start = marker_pos + prefix.len();
    let digits_end = rest[digits_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| digits_start + i)
        .unwrap_or(rest.len());
    let id: u32 = rest[digits_start..digits_end]
        .parse()
        .unwrap_or_else(|_| panic!("expected digits after `{prefix}` in:\n{haystack}"));
    (id, from + digits_end)
}

fn construct_own_id_from(haystack: &str, from: usize, call_marker: &str) -> (u32, usize) {
    let call_pos = from
        + haystack[from..]
            .find(call_marker)
            .unwrap_or_else(|| panic!("expected to find `{call_marker}` in:\n{haystack}"));
    let before = &haystack[..call_pos];
    let n_pos = before
        .rfind("const n")
        .unwrap_or_else(|| panic!("expected `const n` before `{call_marker}` in:\n{haystack}"));
    let digits_start = n_pos + "const n".len();
    let digits_end = before[digits_start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|i| digits_start + i)
        .unwrap_or(before.len());
    let id = before[digits_start..digits_end]
        .parse()
        .unwrap_or_else(|_| panic!("expected digits in:\n{haystack}"));
    (id, call_pos)
}

#[test]
fn compute_if_flags_bare_if_no_else_dynamic_positive_omits_argument() {
    // Official `flags === 1`: omit the 4th arg (bare v-if, not NO_SCOPE).
    let flags = super::control_flow::compute_if_flags(
        false,
        false,
        super::control_flow::IfNegative::None,
        Some(0),
        true,
        false,
    );
    assert_eq!(flags, None);
}

#[test]
fn compute_if_flags_bare_if_no_else_static_positive_emits_no_scope_only() {
    // Static positive, no negative — TRUE_NO_SCOPE only (index unused).
    let flags = super::control_flow::compute_if_flags(
        true,
        false,
        super::control_flow::IfNegative::None,
        Some(0),
        true,
        false,
    );
    assert_eq!(
        flags.as_deref(),
        Some("33 /* TRUE_SINGLE_ROOT, TRUE_NO_SCOPE */")
    );
}

#[test]
fn compute_if_flags_dynamic_positive_static_negative_matches_basic_interpolation_golden() {
    // rc.5 `basic-interpolation.vue` `<p v-if>{{ count }}</p><p v-else>zero</p>`.
    let flags = super::control_flow::compute_if_flags(
        false,
        false,
        super::control_flow::IfNegative::Terminal {
            is_static: true,
            multi_root: false,
        },
        Some(0),
        true,
        false,
    );
    assert_eq!(
        flags.as_deref(),
        Some("325 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, FALSE_NO_SCOPE, KEYED_INDEX_0 */")
    );
}

#[test]
fn compute_if_flags_both_branches_dynamic_no_no_scope_bits() {
    let flags = super::control_flow::compute_if_flags(
        false,
        false,
        super::control_flow::IfNegative::Terminal {
            is_static: false,
            multi_root: false,
        },
        Some(2),
        true,
        false,
    );
    assert_eq!(
        flags.as_deref(),
        Some("773 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_2 */")
    );
}

#[test]
fn compute_if_flags_else_if_chain_never_gets_false_no_scope() {
    // `v-else-if` is never NO_SCOPE (`negative.type !== 14`).
    let flags = super::control_flow::compute_if_flags(
        true,
        false,
        super::control_flow::IfNegative::Chain,
        Some(1),
        true,
        false,
    );
    // A `v-else-if` false branch is the ELSE_IF shape (3 << 2).
    assert_eq!(
        flags.as_deref(),
        Some("557 /* TRUE_SINGLE_ROOT, FALSE_ELSE_IF, TRUE_NO_SCOPE, KEYED_INDEX_1 */")
    );
}

#[test]
fn if_branch_shapes_follow_the_official_block_shape() {
    // Official `getBlockShape`: a branch is SINGLE_ROOT only when its content
    // is one element without its own `v-if`; a text-only `<template>` is
    // MULTI_ROOT, and a `v-else-if` false branch is ELSE_IF.
    let r = render_code(
        "<template><div v-if=\"a\">A</div><template v-else>{{ b }}</template></template>",
    );
    assert!(
        r.contains("TRUE_SINGLE_ROOT, FALSE_MULTI_ROOT"),
        "got:
{r}"
    );
    let r = render_code(
        "<template><template v-if=\"a\">{{ b }}</template><div v-else>A</div></template>",
    );
    assert!(
        r.contains("TRUE_MULTI_ROOT, FALSE_SINGLE_ROOT"),
        "got:
{r}"
    );
    let r = render_code(
        "<template><div v-if=\"a\">A</div><div v-else-if=\"b\">B</div><div v-else>C</div></template>",
    );
    assert!(
        r.contains("TRUE_SINGLE_ROOT, FALSE_ELSE_IF"),
        "got:
{r}"
    );
    assert!(
        !r.contains("FALSE_SINGLE_ROOT, TRUE_NO_SCOPE, KEYED_INDEX_0"),
        "got:
{r}"
    );
}

#[test]
fn compute_if_flags_nested_v_if_never_gets_no_scope_bits() {
    // Nested v-if (`allowNoScope` false) suppresses both NO_SCOPE bits.
    let flags = super::control_flow::compute_if_flags(
        true,
        false,
        super::control_flow::IfNegative::Terminal {
            is_static: true,
            multi_root: false,
        },
        Some(0),
        false,
        false,
    );
    assert_eq!(
        flags.as_deref(),
        Some("261 /* TRUE_SINGLE_ROOT, FALSE_SINGLE_ROOT, KEYED_INDEX_0 */")
    );
}

#[test]
fn compute_if_flags_production_mode_omits_comment() {
    let flags = super::control_flow::compute_if_flags(
        false,
        false,
        super::control_flow::IfNegative::Terminal {
            is_static: true,
            multi_root: false,
        },
        Some(0),
        true,
        true,
    );
    assert_eq!(flags.as_deref(), Some("325"));
}
