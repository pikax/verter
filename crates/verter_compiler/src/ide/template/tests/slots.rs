use super::*;

#[test]
fn vue_slot_bodies_are_fragment_siblings_not_jsx_element_children() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Panel children="explicit" definitelyInvalid="bad"><div class="card">{{ templateOnly }}</div></Panel></template>"#,
        &[
            ("Panel", BindingType::SetupConst),
            ("templateOnly", BindingType::SetupConst),
        ],
    );
    let facts = jsx_element_body_facts(&result);

    assert_eq!(
        facts.element_count, 2,
        "Panel and div must remain typed JSX elements"
    );
    assert!(
        facts.non_empty_elements.is_empty(),
        "Vue slot bodies must not become React JSX children: {facts:?}\n{result}"
    );
    assert_eq!(
        facts.explicit_children_attributes, 1,
        "an authored children prop must remain on the typed element"
    );
    assert_eq!(
        facts.definitely_invalid_attributes, 1,
        "an invalid authored prop must remain on the typed element for the TypeScript diagnostic"
    );
    assert_eq!(
        facts.template_only_references, 1,
        "detaching slot content must preserve the authored template reference"
    );
}

#[test]
fn v_slot_attribute_removed_from_output() {
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ item }"><span>{{ item }}</span></template></MyComp></template>"#,
    );
    assert!(
        !result.contains("v-slot") && !result.contains("#default"),
        "v-slot/#default must be removed from output, got: {}",
        result
    );
}

// ── Part G: <template v-slot> with v-if ──────────────────────────

#[test]
fn template_v_if_v_slot_skips_iife() {
    // <template v-if v-slot> should NOT get IIFE wrapping (slot handles conditions)
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="show" #default>content</template></MyComp></template>"#,
    );
    // The IIFE pattern should NOT wrap the slot template
    assert!(
        !result.contains("{()=>{if(show){"),
        "template with v-if + v-slot should not get IIFE wrapping, got:\n{}",
        result
    );
}

#[test]
fn jsx_valid_v_slot_component() {
    assert_valid_jsx(
        r#"<template><MyComp v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-slot on component",
    );
}

#[test]
fn jsx_valid_named_slot() {
    assert_valid_jsx(
        r#"<template><MyComp><template #header>Header</template><template #default>Body</template></MyComp></template>"#,
        "named slots with template",
    );
}

#[test]
fn jsx_valid_v_if_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-if="show" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-if + v-slot on component",
    );
}

#[test]
fn jsx_valid_v_for_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-for + v-slot",
    );
}

// ── Slot outlets in TSX ────────────────────────────────────────

#[test]
fn slot_outlet_default() {
    let result = gen_tsx_template(r#"<template><slot /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Default slot outlet should produce ___VERTER___instance.$slots.default?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.default"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_named() {
    let result = gen_tsx_template(r#"<template><slot name="header" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.header?.()"),
        "Named slot outlet should produce ___VERTER___instance.$slots.header?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.header"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item" :data="itemData" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.item"),
        "Slot call should reference ___VERTER___instance.$slots.item. Got: {}",
        result
    );
    assert!(
        result.contains("data: ___VERTER___instance.itemData")
            || result.contains("data:___VERTER___instance.itemData"),
        "Slot props should include data binding with instance prefix (unresolved). Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_fallback() {
    let result = gen_tsx_template(r#"<template><slot>fallback</slot></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Slot with fallback should have ___VERTER___instance.$slots call. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name() {
    let result = gen_tsx_template(r#"<template><slot name="overlay-content" /></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.overlay-content"),
        "Must NOT use dot notation for hyphenated names (parses as subtraction). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item-data" :value="x" /></template>"#);
    assert!(
        result.contains("$slots['item-data']"),
        "Hyphenated slot name with props must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("value:") || result.contains("value :"),
        "Slot props should be present. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_dotted_name() {
    let result = gen_tsx_template(r#"<template><slot name="foo.bar" /></template>"#);
    assert!(
        result.contains("$slots['foo.bar']"),
        "Dotted slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.foo.bar"),
        "Must NOT use dot notation for dotted names. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_fallback() {
    let result =
        gen_tsx_template(r#"<template><slot name="overlay-content">fallback</slot></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name with fallback must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

// ── Slot outlet source map accuracy ─────────────────────────────

#[test]
fn slot_outlet_tag_name_source_mapped_to_slots() {
    // Hovering on `slot` in `<slot name="reference" />` should map to `$slots`
    // in the generated TSX, NOT to `?.()` or other synthetic regions.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(output.contains("$slots"), "should contain $slots: {output}");
    assert!(
        output.contains(".reference"),
        "should contain .reference: {output}"
    );

    // Find source position of `s` in `<slot`
    let slot_src_col = source.find("<slot").unwrap() as u32 + 1; // position of `s`

    // Find the generated position of `$slots`
    let gen_slots_pos = output.find("$slots").unwrap() as u32;

    // The source map token at `s` should map to `$slots` in generated output,
    // NOT to positions past `$slots` (like `?.()`)
    let token_for_slot = tokens.iter().find(|&&(_, _, sc)| sc == slot_src_col);
    assert!(
        token_for_slot.is_some(),
        "should have source map token for `slot` tag name at src col {}. Tokens: {:?}",
        slot_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_slot.unwrap();
    // dst_col should be within the `$slots` region, not past it
    assert!(
        dst_col >= gen_slots_pos && dst_col < gen_slots_pos + 6,
        "slot tag name should map to `$slots` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_slots_pos,
        gen_slots_pos + 6,
        dst_col,
        output
    );
}

#[test]
fn slot_outlet_name_attr_does_not_map_to_call_site() {
    // Positions within the `name="reference"` attribute should NOT map to `?.()`.
    // The slot name value `reference` should map to `.reference` in generated output.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of `reference` value (inside quotes)
    let ref_src_col = source.find("reference").unwrap() as u32;

    // Find generated position of `reference` (in `.reference`)
    let gen_ref_text = ".reference";
    let gen_ref_pos = output.find(gen_ref_text).unwrap() as u32;
    let gen_ref_start = gen_ref_pos + 1; // skip the `.`

    // The token for `reference` should map to the `.reference` region
    let token_for_ref = tokens.iter().find(|&&(_, _, sc)| sc == ref_src_col);
    assert!(
        token_for_ref.is_some(),
        "should have source map token for `reference` at src col {}. Tokens: {:?}",
        ref_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_ref.unwrap();
    assert!(
        dst_col >= gen_ref_start && dst_col < gen_ref_start + 9,
        "reference should map to `.reference` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_ref_start,
        gen_ref_start + 9,
        dst_col,
        output
    );
}

// ── Bug fix tests: verter-tsc false errors ──────────────────────────

#[test]
fn template_v_if_v_slot_no_orphan_iife_close() {
    // Bug: <template v-if v-slot> skips IIFE open but walker adds orphan }} close
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="hasSlot" #indicator="bind"><slot name="indicator" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // Should not have orphan `}}` (IIFE close without matching open)
    // The JSX should be well-structured
    assert!(
        !result.contains("</>}}"),
        "should not have orphan IIFE close after slot template, got: {}",
        result
    );
}

#[test]
fn slot_props_kebab_case_quoted() {
    // Bug: slot scope props with kebab-case names generate unquoted property names
    // e.g., { item-class: "value" } which is invalid JS (item minus class)
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ itemClass }"><slot :item-class="itemClass" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // If slot props contain kebab-case keys, they must be quoted
    // This test verifies we don't generate unquoted hyphenated property names
    if result.contains("item-class") {
        assert!(
            result.contains(r#""item-class""#) || result.contains("'item-class'"),
            "kebab-case slot prop key must be quoted in JS object literal, got: {}",
            result
        );
    }
}

// ── Fix 3: v-slot scoped parameter typing ─────────────────────────

#[test]
fn empty_scoped_slot_retains_its_inferred_instance() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp :value="value" v-slot="{ item }"></MyComp></template>"#,
        &[("value", BindingType::SetupConst)],
    );
    assert!(
        result.contains("const ___VERTER___slotInstance"),
        "{result}"
    );
    assert!(
        result.contains("const { item } = ___VERTER___extractArgumentsFromRenderSlot"),
        "{result}"
    );
    assert_valid_tsx(&result, "empty-scoped-slot");
}

#[test]
fn incomplete_scoped_slot_does_not_reference_an_undeclared_instance() {
    for source in [
        r#"<template><MyComp :value="value" v-slot="{ item }" /></template>"#,
        r#"<template><MyComp :value="value" v-slot="{ item }"></template>"#,
    ] {
        let result = gen_tsx_template_with_bindings(source, &[("value", BindingType::SetupConst)]);
        assert!(
            !result.contains("extractArgumentsFromRenderSlot(___VERTER___slotInstance"),
            "{result}"
        );
    }
}

#[test]
fn component_props_use_parent_scope_before_own_slot_bindings() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp :value="value" v-slot="{ value }">{{ value }}</MyComp></template>"#,
        &[("value", BindingType::Props)],
    );
    assert!(
        result.contains("value={__props.value}"),
        "parent prop must retain its binding: {result}"
    );
    assert!(
        result.contains("\"value\": (__props.value)"),
        "slot inference must read the parent prop: {result}"
    );
    assert!(
        result.contains("{ const { value } = ___VERTER___extractArgumentsFromRenderSlot"),
        "the slot binding must be scoped after its initializer: {result}"
    );
    assert!(
        result.contains("<>{ value }</>"),
        "the child still reads its slot local: {result}"
    );
}

#[test]
fn named_slot_loop_cannot_shadow_the_parent_inference_input() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp :value="item"><template v-for="item in rows" #default="{ value }">{{ value }}</template></MyComp></template>"#,
        &[
            ("item", BindingType::SetupConst),
            ("rows", BindingType::SetupConst),
        ],
    );
    let capture = result
        .find("componentConstructor(MyComp)")
        .expect("parent inference");
    let child_loop = result.find(".map(").expect("named slot loop");
    assert!(
        capture < child_loop,
        "the parent input must be captured before the shadowing loop: {result}"
    );
    assert!(result.contains("\"value\": (item)"), "{result}");
}

#[test]
fn slot_inference_matches_dot_bind_shorthand() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp .value="value" .model-value v-slot="{ item }">{{ item }}</MyComp></template>"#,
        &[
            ("value", BindingType::SetupConst),
            ("modelValue", BindingType::SetupConst),
        ],
    );
    assert!(result.contains("\"value\": (value)"), "{result}");
    assert!(result.contains("\"modelValue\": (modelValue)"), "{result}");
    assert!(
        !result.contains("...(value)"),
        "dot binds are properties, not spreads: {result}"
    );
}

#[test]
fn scoped_slots_infer_from_named_parent_props_in_the_loop_scope() {
    let result = gen_tsx_template(
        r#"<template><div v-for="row in rows"><MyComp :options="row.options" v-model="row.selected"><template #selected="{ value }">{{ value }}</template></MyComp></div><Other /></template>"#,
    );
    assert!(
        result.contains("new (___VERTER___componentConstructor(MyComp))({\"options\": (row.options), \"modelValue\": (row.selected)})"),
        "slot inference must retain the named-slot parent's actual props and loop locals: {result}"
    );
    assert!(!result.contains("instantiateComponent(MyComp, {})"));
}

#[test]
fn scoped_slot_inference_retains_bound_spreads_shorthand_and_camelized_keys() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-bind="props" :options :model-value label="pick" v-slot="{ item }">{{ item }}</MyComp></template>"#,
        &[
            ("props", BindingType::SetupConst),
            ("options", BindingType::SetupConst),
            ("modelValue", BindingType::SetupConst),
        ],
    );
    assert!(result.contains("componentConstructor(MyComp)"), "{result}");
    assert!(result.contains("...(props)"), "{result}");
    assert!(result.contains("\"options\": (options)"), "{result}");
    assert!(result.contains("\"modelValue\": (modelValue)"), "{result}");
    assert!(result.contains("\"label\": \"pick\""), "{result}");
}

#[test]
fn named_slot_inference_uses_dynamic_prop_keys_and_named_models() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp :[propName]="value" v-model:selected="selected" :model-value><template #default="{ item }">{{ item }}</template></MyComp></template>"#,
        &[
            ("propName", BindingType::SetupConst),
            ("value", BindingType::SetupConst),
            ("selected", BindingType::SetupConst),
            ("modelValue", BindingType::SetupConst),
        ],
    );
    assert!(result.contains("[propName]: (value)"), "{result}");
    assert!(!result.contains("[[propName]]"), "{result}");
    assert!(result.contains("\"selected\": (selected)"), "{result}");
    assert!(result.contains("\"modelValue\": (modelValue)"), "{result}");
}

#[test]
fn v_slot_params_arrow_wrapper() {
    // Component v-slot with params: should generate IIFE with extractArgumentsFromRenderSlot
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
    );
    eprintln!("v-slot params output: {}", result);
    // Positive: should have arrow function wrapper for slot params
    assert!(
        result.contains("{ slotItem }") || result.contains("{slotItem}"),
        "should contain slot params in arrow function: {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot for slot typing: {result}"
    );
    assert!(
        result.contains("componentConstructor"),
        "should retain the constructor for component instance: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should reference default slot name: {result}"
    );
    // Negative: v-slot attribute must not appear
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
    assert!(
        result.contains("const { slotItem } = ___VERTER___extractArgumentsFromRenderSlot")
            || result.contains("const {slotItem} = ___VERTER___extractArgumentsFromRenderSlot"),
        "slot params should bind from the typed slot extract result, got: {result}"
    );
    assert!(
        !result.contains("function({ slotItem })") && !result.contains("function({slotItem})"),
        "slot params should not be introduced as untyped function parameters, got: {result}"
    );
}

#[test]
fn v_slot_named_template_params() {
    // <template #header="{ title }"> should generate typed wrapper with "header"
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ title }"><span>{{ title }}</span></template></MyComp></template>"#,
    );
    eprintln!("named template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""header""#),
        "should reference header slot name: {result}"
    );
    // Negative
    assert!(
        !result.contains("#header") && !result.contains("v-slot:header"),
        "v-slot directive must be removed: {result}"
    );
}

#[test]
fn v_slot_default_template_params() {
    // <template v-slot="{ data }"> should use "default" slot name
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-slot="{ data }"><span>{{ data }}</span></template></MyComp></template>"#,
    );
    eprintln!("default template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should use default slot name: {result}"
    );
}

#[test]
fn v_slot_multiple_named_templates() {
    // Multiple named slots — each gets independent IIFE, params don't leak
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ x }"><span>{{ x }}</span></template><template #footer="{ y }"><span>{{ y }}</span></template></MyComp></template>"#,
    );
    eprintln!("multi-slot output: {}", result);
    assert!(
        result.contains(r#""header""#) && result.contains(r#""footer""#),
        "should reference both slot names: {result}"
    );
    // Count extractArgumentsFromRenderSlot calls — should be 2
    let count = result.matches("extractArgumentsFromRenderSlot").count();
    assert_eq!(
        count, 2,
        "should have 2 extractArgumentsFromRenderSlot calls: {result}"
    );
}

#[test]
fn v_slot_no_params_unchanged() {
    // v-slot without params: no wrapper needed
    let result =
        gen_tsx_template(r#"<template><MyComp v-slot><span>content</span></MyComp></template>"#);
    eprintln!("v-slot no params output: {}", result);
    assert!(
        !result.contains("extractArgumentsFromRenderSlot"),
        "no wrapper for v-slot without params: {result}"
    );
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
}

#[test]
fn v_slot_params_no_instance_prefix() {
    // Slot params must NOT get ___VERTER___instance. prefix
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
        &[("slotItem", BindingType::SetupConst)], // Even if in bindings, slot takes priority
    );
    assert!(
        !result.contains("___VERTER___instance.slotItem"),
        "slot param must NOT get instance prefix: {result}"
    );
}

#[test]
fn partial_v_slot_param_stays_bare_for_completion() {
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem, slotIndex, slotTotal }"><span>{{ sl }}</span></MyComp></template>"#,
    );
    assert!(
        result.contains("{ sl }") || result.contains("{sl}"),
        "partial slot param should stay bare for completion context, got: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.sl"),
        "partial slot param must not get instance prefix, got: {result}"
    );
}

#[test]
fn v_slot_with_v_for() {
    // v-for wraps element, slot wraps children — both should work
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        &[("items", BindingType::SetupConst)],
    );
    eprintln!("v-for + v-slot output: {}", result);
    // Both v-for map and v-slot IIFE should be present
    assert!(
        result.contains(".map("),
        "v-for should produce .map(): {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "v-slot should produce extractArgumentsFromRenderSlot: {result}"
    );
}

#[test]
fn strict_slots_component_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><TabItem /><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot call with default slot and TabItem children
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("$slots"),
        "should reference $slots, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem constructor, got:\n{}",
        result
    );
    // Negative: no v-if or v-for artifacts in slot check
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_html_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><input /><span></span></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with HTML element type references
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("HTMLElementTagNameMap"),
        "should reference HTMLElementTagNameMap, got:\n{}",
        result
    );
    assert!(
        result.contains("\"input\""),
        "should reference input element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-slot"),
        "no v-slot in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_text_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>hello world</Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with string type for text
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for text, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for text node, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("HTMLElementTagNameMap"),
        "should not have HTMLElementTagNameMap for text, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_named_slot() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot referencing named slot 'header'
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'header'"),
        "should reference header slot name, got:\n{}",
        result
    );
    // Negative: should NOT have 'default' slot call
    assert!(
        !result.contains("'default'"),
        "should not have default slot (only named), got:\n{}",
        result
    );
}

#[test]
fn strict_slots_mixed_named_default() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template><template #default><span /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: two separate strictRenderSlot calls
    assert!(
        result.contains("'header'"),
        "should have header slot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should have default slot, got:\n{}",
        result
    );
    // Count occurrences of strictRenderSlot
    let count = result.matches("strictRenderSlot").count();
    assert!(
        count >= 2,
        "should have at least 2 strictRenderSlot calls, got {}, output:\n{}",
        count,
        result
    );
}

#[test]
fn strict_slots_no_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs /></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for self-closing components
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for self-closing, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_whitespace_only() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>   \n   </Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for whitespace-only children
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for whitespace-only children, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_dynamic_component() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><component :is="comp"><span /></component></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // Negative: no strictRenderSlot for dynamic <component :is>
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for dynamic component, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_disabled() {
    let result = gen_tsx_template_with_bindings(
        "<template><Tabs><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Negative: no strictRenderSlot when strict_slots is false (default helper)
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot when disabled, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-if="show" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("show", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem still in the strict slot check (v-if doesn't change the type)
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem in slot check, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-for="i in 3" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: TabItem still in the strict slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_with_v_slot_params() {
    // When a component has v-slot params, BOTH extractArgumentsFromRenderSlot
    // (for slot props typing) AND strictRenderSlot (for children checking) should appear.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-slot="{ item }"><TabItem /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: both helpers present
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should have extractArgumentsFromRenderSlot for slot params, got:\n{}",
        result
    );
    assert!(
        result.contains("strictRenderSlot"),
        "should have strictRenderSlot for children, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem in slot check, got:\n{}",
        result
    );
    // Negative: no raw v-slot
    assert!(
        !result.contains("v-slot"),
        "v-slot directive should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_narrowing() {
    // v-if/v-else branches produce different element types — both should be in the array
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><div v-if="isA" /><span v-else /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("isA", BindingType::SetupRef),
        ],
    );
    // Positive: both div and span in the slot check array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("\"div\""),
        "should reference div element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_nested() {
    // <template v-for> with a slot name should still collect children correctly
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><template v-for="item in items" #default><TabItem /></template></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("items", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem in default slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_sourcemap_component_child() {
    // Verify that the source map has a token mapping the child constructor
    // name back to its position in the template.
    let source = "<template><Tabs><TabItem /></Tabs></template>";
    let (output, tokens) = gen_tsx_template_strict_slots_with_map(
        source,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );

    // Find `TabItem` position in the source (after `<`)
    let tab_item_src_col = source.find("<TabItem").unwrap() as u32 + 1; // skip `<`

    // The strictRenderSlot call should contain TabItem with a mapped token
    assert!(
        output.contains("strictRenderSlot"),
        "should have strictRenderSlot in output: {}",
        output
    );

    // Find a token that maps to the TabItem source position
    let has_tab_item_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == tab_item_src_col);
    assert!(
        has_tab_item_token,
        "should have a source map token at TabItem position (col {}), tokens: {:?}\noutput: {}",
        tab_item_src_col, tokens, output
    );
}

#[test]
fn strict_slots_sourcemap_html_child() {
    // Verify sourcemap mapping for HTML element children
    let source = "<template><Tabs><input /></Tabs></template>";
    let (output, tokens) =
        gen_tsx_template_strict_slots_with_map(source, &[("Tabs", BindingType::SetupImport)]);

    // `input` position in source (after `<`)
    let input_src_col = source.find("<input").unwrap() as u32 + 1;

    assert!(
        output.contains("HTMLElementTagNameMap[\"input\"]"),
        "should have HTMLElementTagNameMap in output: {}",
        output
    );

    // Find a token that maps to the input source position
    let has_input_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == input_src_col);
    assert!(
        has_input_token,
        "should have a source map token at input position (col {}), tokens: {:?}\noutput: {}",
        input_src_col, tokens, output
    );
}

#[test]
fn strict_slots_v_for_component_var() {
    // v-for introduces a component variable — the strict slot check should use
    // the loop variable name as the constructor reference.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-for="Comp in components"><Comp /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("components", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot referencing v-for variable Comp
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    // The child constructor should be "Comp" — the v-for loop variable
    // It appears in the strictRenderSlot array
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Comp"),
        "strictRenderSlot array should contain Comp (v-for variable), got:\n{}",
        slot_call
    );
    // Negative: no raw v-for in output
    assert!(
        !result.contains("v-for"),
        "v-for should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_slot_component_var() {
    // v-slot destructures a component — the strict slot check on the inner
    // component should reference the slot variable name.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Provider v-slot="{ Child }"><Tabs><Child /></Tabs></Provider></template>"#,
        &[
            ("Provider", BindingType::SetupImport),
            ("Tabs", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot on Tabs with Child in the array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    // Find the Tabs strict slot call — it should reference Child
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Child"),
        "strictRenderSlot array should contain Child (v-slot variable), got:\n{}",
        slot_call
    );
    assert!(
        slot_call.contains("'default'"),
        "should reference default slot, got:\n{}",
        slot_call
    );
    // Negative: raw v-slot should not be in output
    assert!(
        !result.contains("v-slot"),
        "v-slot should not appear in output, got:\n{}",
        result
    );
    // Provider also has children (Tabs) so it should also get a strictRenderSlot call
    let second_call = result.match_indices("strictRenderSlot").nth(1);
    assert!(
        second_call.is_some(),
        "Provider should also get a strictRenderSlot call for its default slot, got:\n{}",
        result
    );
}

#[test]
fn v_if_v_for_v_else_slot_outlet_plain_branch() {
    // Lifted ternary where v-else branch is a plain slot outlet
    let result = gen_tsx_template(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_v_else_slot_outlet ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
        "v-if+v-for then slot v-else",
    );
}

#[test]
fn slot_summary_memoized_warm_requery_builds_zero_extra() {
    // The overlay builds a component's slot summary on the FIRST demand and
    // serves every later demand for the same component warm from its memoized
    // cell. A second query for an already-built component must trigger ZERO
    // additional builds. Bypassing the cell (rebuilding per query) would make the
    // second query bump the build count to 2 and fail here.
    use crate::ast::types::{AstNodeKind, TagType};
    use crate::template::oxc::{reset_slot_summary_counts, slot_summary_build_count};

    let source = r#"<template>
  <Card><Panel /></Card>
</template>"#;
    let alloc = Allocator::new();
    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    let template_ast = syntax.take_template_ast().expect("template ast");
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        oxc_span::SourceType::tsx(),
        true,
    );

    // First slot-checkable component in source order (`Card`).
    let comp_id = template_ast
        .nodes
        .iter()
        .enumerate()
        .find_map(|(idx, node)| match &node.kind {
            AstNodeKind::Element(el) if el.tag_type == TagType::Component => {
                Some(crate::types::NodeId(idx))
            }
            _ => None,
        })
        .expect("a component node");

    reset_slot_summary_counts();

    // Cold demand: builds exactly one summary.
    let first = oxc_ast.slot_summary(comp_id, &template_ast, source);
    assert!(first.is_some(), "Card is a slot-checkable component");
    assert_eq!(
        slot_summary_build_count(),
        1,
        "the first demand for a component must build its summary once"
    );

    // Warm demand: same component, served from the memoized cell — ZERO rebuilds.
    let second = oxc_ast.slot_summary(comp_id, &template_ast, source);
    assert!(second.is_some(), "the warm summary must still resolve");
    assert_eq!(
        slot_summary_build_count(),
        1,
        "a warm re-query of an already-built component must build ZERO additional summaries"
    );
}
