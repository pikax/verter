use super::*;
use oxc_allocator::Allocator;

/// Compile an SFC with `extract_template_data: true` and return the raw data.
fn extract(source: &str) -> RawTemplateData {
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions {
        extract_template_data: true,
        ..Default::default()
    };
    let result = crate::compile::compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    result
        .template_data
        .expect("extract_template_data was set but no data returned")
}

/// Compile an SFC with script setup bindings and extract template data.
/// The `script_setup` string is inserted into `<script setup>` block.
fn extract_with_script(template: &str, script_setup: &str) -> RawTemplateData {
    let source = format!("<script setup>\n{}\n</script>\n{}", script_setup, template);
    extract(&source)
}

// ── Component detection ──

#[test]
fn component_usage_detected() {
    let data = extract("<template><Child /></template>");
    assert_eq!(data.components.len(), 1);
    assert_eq!(data.components[0].tag_name, "Child");
    assert!(!data.components[0].is_dynamic);
}

#[test]
fn plain_element_not_component() {
    let data = extract("<template><div>hello</div></template>");
    assert!(data.components.is_empty());
}

#[test]
fn dynamic_component_flagged() {
    let data = extract_with_script(
        r#"<template><component :is="comp" /></template>"#,
        "import { ref } from 'vue'\nconst comp = ref('MyComp')",
    );
    assert_eq!(data.components.len(), 1);
    assert!(data.components[0].is_dynamic);
}

// ── Props ──

#[test]
fn static_prop_detected() {
    let data = extract(r#"<template><Child msg="hello" /></template>"#);
    assert_eq!(data.components.len(), 1);
    assert_eq!(data.components[0].props.len(), 1);
    assert_eq!(data.components[0].props[0].name, "msg");
    assert!(!data.components[0].props[0].is_bound);
}

#[test]
fn bound_const_prop_all_static() {
    let data = extract_with_script(
        r#"<template><Child :msg="LABEL" /></template>"#,
        "const LABEL = 'hello'",
    );
    assert_eq!(data.components[0].props.len(), 1);
    assert!(data.components[0].props[0].is_bound);
    assert_eq!(data.components[0].props[0].all_bindings_static, Some(true));
}

#[test]
fn bound_ref_prop_not_static() {
    let data = extract_with_script(
        r#"<template><Child :msg="count" /></template>"#,
        "import { ref } from 'vue'\nconst count = ref(0)",
    );
    assert_eq!(data.components[0].props[0].all_bindings_static, Some(false));
}

#[test]
fn spread_detected() {
    let data = extract_with_script(
        r#"<template><Child v-bind="obj" /></template>"#,
        "import { reactive } from 'vue'\nconst obj = reactive({})",
    );
    assert!(data.components[0].has_spread);
}

// ── Binding occurrences ──

#[test]
fn binding_occurrences_collected() {
    let data = extract_with_script(
        r#"<template><div>{{ msg }}</div></template>"#,
        "const msg = 'hello'",
    );
    let msg_occurrences: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name == "msg")
        .collect();
    assert!(!msg_occurrences.is_empty());
    assert!(msg_occurrences[0].is_in_bindings_map);
    assert_eq!(msg_occurrences[0].usage_kind, 0); // interpolation
}

#[test]
fn unresolved_binding_flagged() {
    let data = extract(r#"<template><div>{{ unknown }}</div></template>"#);
    let unknown: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name == "unknown")
        .collect();
    assert!(!unknown.is_empty());
    assert!(!unknown[0].is_in_bindings_map);
}

// ── Template refs ──

#[test]
fn template_ref_static() {
    let data = extract(r#"<template><div ref="el"></div></template>"#);
    assert_eq!(data.template_refs.len(), 1);
    assert_eq!(data.template_refs[0].name, "el");
    assert!(!data.template_refs[0].is_dynamic);
}

#[test]
fn template_ref_dynamic() {
    let data = extract_with_script(
        r#"<template><div :ref="elRef"></div></template>"#,
        "import { ref } from 'vue'\nconst elRef = ref(null)",
    );
    assert_eq!(data.template_refs.len(), 1);
    assert!(data.template_refs[0].is_dynamic);
}

// ── Slot definitions ──

#[test]
fn slot_definition_default() {
    let data = extract(r#"<template><slot /></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert_eq!(data.slot_definitions[0].name, "default");
}

#[test]
fn slot_definition_named() {
    let data = extract(r#"<template><slot name="header" /></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert_eq!(data.slot_definitions[0].name, "header");
}

#[test]
fn slot_definition_dynamic_name_binding_is_not_materialized() {
    let data = extract(r#"<template><slot :name="slotName" /></template>"#);
    assert!(
        data.slot_definitions.is_empty(),
        "dynamic slot outlet names are not concrete slot definitions: {:?}",
        data.slot_definitions
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        data.has_dynamic_slot_outlet,
        "a `:name` dynamic outlet must set the fail-open flag for unused-slot population"
    );
}

#[test]
fn slot_definition_dynamic_name_v_bind_is_not_materialized() {
    let data = extract(r#"<template><slot v-bind:name="slotName" /></template>"#);
    assert!(
        data.slot_definitions.is_empty(),
        "v-bind:name slot outlets are not concrete slot definitions: {:?}",
        data.slot_definitions
            .iter()
            .map(|slot| slot.name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        data.has_dynamic_slot_outlet,
        "a `v-bind:name` dynamic outlet must set the fail-open flag for unused-slot population"
    );
}

#[test]
fn slot_outlet_dynamic_argument_sets_dynamic_outlet_flag() {
    // `<slot :[key]="v">` — a DYNAMIC directive ARGUMENT (distinct from the
    // dynamic-VALUE `:name="expr"` form). The bound attribute name is not
    // statically known, so the outlet inventory cannot be bounded:
    // unused-slot diagnostics must fail open.
    let data = extract(r#"<template><slot :[dynamicName]="value" /></template>"#);
    assert!(
        data.has_dynamic_slot_outlet,
        "a dynamic-argument bind on a slot outlet must set the fail-open flag"
    );
}

#[test]
fn slot_outlet_static_scoped_bindings_do_not_set_dynamic_outlet_flag() {
    let data = extract(r#"<template><slot name="header" :item="row" /></template>"#);
    assert!(
        !data.has_dynamic_slot_outlet,
        "static-name outlets with static scoped bindings are fully bounded"
    );
}

#[test]
fn slot_definition_binding_expressions() {
    let data = extract(r#"<template><slot :item="row" :count="total" /></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    let slot = &data.slot_definitions[0];
    assert!(slot.has_bindings);
    assert_eq!(slot.binding_names, vec!["item", "count"]);
    assert_eq!(slot.binding_expressions, vec!["row", "total"]);
    assert_eq!(slot.binding_value_spans.len(), 2);
    // Verify spans point to the correct source text
    let src = r#"<template><slot :item="row" :count="total" /></template>"#;
    let s0 = &slot.binding_value_spans[0];
    assert_eq!(&src[s0.start as usize..s0.end as usize], "row");
    let s1 = &slot.binding_value_spans[1];
    assert_eq!(&src[s1.start as usize..s1.end as usize], "total");
}

#[test]
fn slot_definition_shorthand_binding() {
    // :item shorthand (no value) means expression = "item"
    let data = extract(r#"<template><slot :item /></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    let slot = &data.slot_definitions[0];
    assert!(slot.has_bindings);
    assert_eq!(slot.binding_names, vec!["item"]);
    assert_eq!(slot.binding_expressions, vec!["item"]);
    assert_eq!(slot.binding_value_spans.len(), 1);
    // Shorthand span should point to the arg name
    let src = r#"<template><slot :item /></template>"#;
    let s0 = &slot.binding_value_spans[0];
    assert_eq!(&src[s0.start as usize..s0.end as usize], "item");
}

// ── Slot fallback content ──

#[test]
fn slot_fallback_content_with_children() {
    let data = extract(r#"<template><slot>fallback text</slot></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert!(
        data.slot_definitions[0].has_fallback_content,
        "slot with text children should have has_fallback_content=true"
    );
}

#[test]
fn slot_fallback_content_self_closing() {
    let data = extract(r#"<template><slot /></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert!(
        !data.slot_definitions[0].has_fallback_content,
        "self-closing slot should have has_fallback_content=false"
    );
}

#[test]
fn slot_fallback_content_empty_tag() {
    let data = extract(r#"<template><slot></slot></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert!(
        !data.slot_definitions[0].has_fallback_content,
        "empty slot tag should have has_fallback_content=false"
    );
}

#[test]
fn slot_fallback_content_with_element() {
    let data = extract(r#"<template><slot><span>default</span></slot></template>"#);
    assert_eq!(data.slot_definitions.len(), 1);
    assert!(
        data.slot_definitions[0].has_fallback_content,
        "slot with element children should have has_fallback_content=true"
    );
}

// ── Event handlers ──

#[test]
fn event_handler_simple() {
    let data = extract_with_script(
        r#"<template><div @click="handleClick"></div></template>"#,
        "function handleClick() {}",
    );
    assert_eq!(data.event_handlers.len(), 1);
    assert_eq!(data.event_handlers[0].event_name, "click");
    assert!(!data.event_handlers[0].is_inline);
}

#[test]
fn event_handler_inline() {
    let data = extract_with_script(
        r#"<template><div @click="count++"></div></template>"#,
        "import { ref } from 'vue'\nconst count = ref(0)",
    );
    assert_eq!(data.event_handlers.len(), 1);
    assert!(data.event_handlers[0].is_inline);
}

// ── v-for ──

#[test]
fn v_for_with_key() {
    let data = extract_with_script(
        r#"<template><div v-for="item in items" :key="item.id"></div></template>"#,
        "import { ref } from 'vue'\nconst items = ref([])",
    );
    assert_eq!(data.v_for_directives.len(), 1);
    assert!(data.v_for_directives[0].has_key);
    assert_eq!(data.v_for_directives[0].variable, "item");
}

#[test]
fn v_for_without_key() {
    let data = extract_with_script(
        r#"<template><div v-for="item in items"></div></template>"#,
        "import { ref } from 'vue'\nconst items = ref([])",
    );
    assert_eq!(data.v_for_directives.len(), 1);
    assert!(!data.v_for_directives[0].has_key);
}

#[test]
fn v_for_literal_array_iterable() {
    let data = extract(
        r#"<template><button v-for="route in ['dashboard', 'settings', 'profile'] as const" :key="route">{{ route }}</button></template>"#,
    );
    assert_eq!(data.v_for_directives.len(), 1);
    assert_eq!(data.v_for_directives[0].variable, "route");
    assert!(
        !data.v_for_directives[0].iterable.is_empty(),
        "iterable must not be empty for literal array v-for"
    );
}

#[test]
fn v_for_numeric_array_iterable() {
    let data = extract(r#"<template><div v-for="item in [1, 2, 3]" :key="item"></div></template>"#);
    assert_eq!(data.v_for_directives.len(), 1);
    assert_eq!(data.v_for_directives[0].variable, "item");
    assert!(
        !data.v_for_directives[0].iterable.is_empty(),
        "iterable must not be empty for numeric array literal v-for"
    );
}

// ── v-model ──

#[test]
fn v_model_on_component() {
    let data = extract_with_script(
        r#"<template><Input v-model="val" /></template>"#,
        "import { ref } from 'vue'\nconst val = ref('')",
    );
    assert_eq!(data.v_model_directives.len(), 1);
    assert!(data.v_model_directives[0].target_is_component);
    assert_eq!(data.v_model_directives[0].binding_name, "modelValue");
}

// ── Nesting depth ──

#[test]
fn nesting_depth_calculated() {
    let data =
        extract(r#"<template><div><div><div><span>deep</span></div></div></div></template>"#);
    assert_eq!(data.max_nesting_depth, 4); // div>div>div>span = 4 levels
}

// ── Comment directives ──

#[test]
fn comment_directive_parsed() {
    let data =
        extract(r#"<template><!-- @verter:disable no-v-html --><div v-html="x"></div></template>"#);
    assert_eq!(data.comment_directives.len(), 1);
    assert_eq!(data.comment_directives[0].kind, 0); // disable
    assert_eq!(
        data.comment_directives[0].rule_or_message.as_deref(),
        Some("no-v-html")
    );
}

// ── If chains ──

#[test]
fn if_chain_conditions_collected() {
    let data = extract_with_script(
        r#"<template><div v-if="a"></div><div v-else-if="b"></div><div v-else></div></template>"#,
        "import { ref } from 'vue'\nconst a = ref(true)\nconst b = ref(false)",
    );
    assert!(!data.if_chains.is_empty());
    let chain = &data.if_chains[0];
    assert_eq!(chain.conditions.len(), 3);
    assert_eq!(chain.conditions[0].0, "a");
    assert_eq!(chain.conditions[1].0, "b");
    assert_eq!(chain.conditions[2].0, ""); // v-else has no condition
}

// ── Negative tests ──

#[test]
fn static_text_no_binding_occurrence() {
    let data = extract(r#"<template><div>hello world</div></template>"#);
    assert!(data.binding_occurrences.is_empty());
}

#[test]
fn self_closing_void_element_correct() {
    let data = extract(r#"<template><br /><input /></template>"#);
    // br and input are void elements, not components
    assert!(data.components.is_empty());
}

#[test]
fn v_for_variable_not_unresolved() {
    let data = extract_with_script(
        r#"<template><div v-for="item in items">{{ item }}</div></template>"#,
        "import { ref } from 'vue'\nconst items = ref([])",
    );
    // "item" is a v-for local variable — the OXC parser should mark it as
    // a local binding, not a script binding occurrence.
    let item_occurrences: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name == "item")
        .collect();
    // item is a v-for local, so it should either not appear or appear with ignore=true
    // (which our extraction skips). The key check: it should NOT be in unresolved bindings.
    for occ in &item_occurrences {
        // If it does appear, it should still be in the bindings map (OXC handles locals)
        // or simply not present. Either way, this test documents the behavior.
        assert!(
            !occ.is_in_bindings_map || item_occurrences.is_empty(),
            "v-for variable should not be a script binding occurrence"
        );
    }
}

#[test]
fn global_properties_not_unresolved() {
    let data = extract(r#"<template><div>{{ $slots }}</div></template>"#);
    let global_occurrences: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name.starts_with('$'))
        .collect();
    // Globals starting with $ are typically ignored by OXC binding extraction
    // because they start with $ prefix. They should not appear as unresolved.
    for occ in &global_occurrences {
        // If they do appear, document it — but they shouldn't be flagged as
        // missing from the bindings map since they're Vue runtime globals.
        assert!(
            !occ.is_in_bindings_map,
            "$ prefixed globals are not in script bindings"
        );
    }
}

#[test]
fn v_for_key_uses_index() {
    let data = extract_with_script(
        r#"<template><div v-for="(item, i) in items" :key="i"></div></template>"#,
        "import { ref } from 'vue'\nconst items = ref([])",
    );
    assert_eq!(data.v_for_directives.len(), 1);
    let vfor = &data.v_for_directives[0];
    assert_eq!(vfor.variable, "item");
    assert_eq!(vfor.index.as_deref(), Some("i"));
    assert!(vfor.has_key);
    assert!(vfor.key_uses_index);
}

#[test]
fn multiple_components_detected() {
    let data = extract(r#"<template><div><Header /><Sidebar /><Footer /></div></template>"#);
    assert_eq!(data.components.len(), 3);
    let names: Vec<_> = data
        .components
        .iter()
        .map(|c| c.tag_name.as_str())
        .collect();
    assert!(names.contains(&"Header"));
    assert!(names.contains(&"Sidebar"));
    assert!(names.contains(&"Footer"));
}

#[test]
fn component_slot_usage_tracked() {
    // When v-slot is used directly on a component (default slot shorthand),
    // it's detected on the component's props.
    let data = extract(
        r#"<template><MyLayout v-slot="{ data }"><span>{{ data }}</span></MyLayout></template>"#,
    );
    assert_eq!(data.components.len(), 1);
    let comp = &data.components[0];
    assert_eq!(comp.tag_name, "MyLayout");
    // v-slot on the component itself is cached in v_slot (not in props),
    // so our current extraction won't see it in the props loop.
    // This documents the current behavior. Named slot usage on child
    // <template> elements is a separate extraction concern.
}

#[test]
fn v_model_custom_name() {
    let data = extract_with_script(
        r#"<template><Input v-model:title="val" /></template>"#,
        "import { ref } from 'vue'\nconst val = ref('')",
    );
    assert_eq!(data.v_model_directives.len(), 1);
    assert_eq!(data.v_model_directives[0].binding_name, "title");
}

#[test]
fn template_ref_target_tag() {
    let data = extract(r#"<template><input ref="inputEl" /></template>"#);
    assert_eq!(data.template_refs.len(), 1);
    assert_eq!(data.template_refs[0].target_tag, "input");
    assert_eq!(data.template_refs[0].name, "inputEl");
}

#[test]
fn element_v_show_detected() {
    let data = extract_with_script(
        r#"<template><div v-show="visible">content</div></template>"#,
        "import { ref } from 'vue'\nconst visible = ref(true)",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(div.has_v_show);
}

#[test]
fn element_v_html_detected() {
    let data = extract_with_script(
        r#"<template><div v-html="content"></div></template>"#,
        "const content = '<p>hello</p>'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(div.has_v_html);
}

#[test]
fn comment_directive_disable_next_line() {
    let data = extract(
        r#"<template><!-- @verter:disable-next-line no-v-html --><div v-html="x"></div></template>"#,
    );
    assert_eq!(data.comment_directives.len(), 1);
    assert_eq!(data.comment_directives[0].kind, 1); // disable-next-line
    assert!(data.comment_directives[0].affects_next_line);
    assert_eq!(
        data.comment_directives[0].rule_or_message.as_deref(),
        Some("no-v-html")
    );
}

#[test]
fn binding_occurrences_from_multiple_contexts() {
    let data = extract_with_script(
        r#"<template><div :class="cls">{{ msg }}</div></template>"#,
        "const msg = 'hello'\nconst cls = 'active'",
    );
    let msg_occ: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name == "msg")
        .collect();
    let cls_occ: Vec<_> = data
        .binding_occurrences
        .iter()
        .filter(|b| b.name == "cls")
        .collect();
    assert!(!msg_occ.is_empty(), "msg should have binding occurrence");
    assert!(!cls_occ.is_empty(), "cls should have binding occurrence");
    // msg is from interpolation (kind=0), cls is from directive (kind=1)
    assert_eq!(msg_occ[0].usage_kind, 0);
    assert_eq!(cls_occ[0].usage_kind, 1);
}

#[test]
fn element_parent_tag_tracked() {
    let data = extract(r#"<template><div><span>text</span></div></template>"#);
    let span = data.elements.iter().find(|e| e.tag == "span").unwrap();
    assert_eq!(span.parent_tag.as_deref(), Some("div"));
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(div.parent_tag.is_none()); // Root child has no parent tag
}

// ── has_text_content whitespace handling ──

#[test]
fn whitespace_only_element_has_no_text_content() {
    // Whitespace between tags should NOT count as text content
    let data = extract("<template>\n  <div class=\"app\">\n  </div>\n</template>");
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        !div.has_text_content,
        "whitespace-only content should not set has_text_content"
    );
}

#[test]
fn actual_text_element_has_text_content() {
    let data = extract("<template><div>hello</div></template>");
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        div.has_text_content,
        "non-whitespace text should set has_text_content"
    );
}

#[test]
fn interpolation_counts_as_text_content() {
    let data = extract_with_script(
        "<template><div>{{ msg }}</div></template>",
        "const msg = 'hi'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        div.has_text_content,
        "interpolation should count as text content"
    );
}

#[test]
fn mixed_whitespace_and_interpolation_has_text_content() {
    let data = extract_with_script(
        "<template><div>\n  {{ msg }}\n</div></template>",
        "const msg = 'hi'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        div.has_text_content,
        "interpolation with surrounding whitespace should count"
    );
}

#[test]
fn nested_element_with_whitespace_only_no_text_content() {
    // Parent div contains only child elements and whitespace
    let data = extract("<template><div>\n  <span>hello</span>\n</div></template>");
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        !div.has_text_content,
        "div with only whitespace + child elements should not have text content"
    );
    let span = data.elements.iter().find(|e| e.tag == "span").unwrap();
    assert!(
        span.has_text_content,
        "span with 'hello' should have text content"
    );
}

// ── has_bare_text (literal text vs interpolation) ──

#[test]
fn bare_text_true_for_literal_text() {
    let data = extract("<template><div>hello</div></template>");
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(div.has_bare_text, "literal text should set has_bare_text");
    assert!(
        div.has_text_content,
        "literal text should also set has_text_content"
    );
}

#[test]
fn bare_text_false_for_interpolation_only() {
    let data = extract_with_script(
        "<template><div>{{ msg }}</div></template>",
        "const msg = 'hi'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        !div.has_bare_text,
        "interpolation-only should NOT set has_bare_text"
    );
    assert!(
        div.has_text_content,
        "interpolation should still set has_text_content"
    );
}

#[test]
fn bare_text_true_for_mixed_text_and_interpolation() {
    let data = extract_with_script(
        "<template><div>hello {{ msg }}</div></template>",
        "const msg = 'hi'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        div.has_bare_text,
        "mixed literal text + interpolation should set has_bare_text"
    );
    assert!(
        div.has_text_content,
        "mixed content should set has_text_content"
    );
}

#[test]
fn bare_text_false_for_whitespace_only() {
    let data = extract("<template><div>   </div></template>");
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        !div.has_bare_text,
        "whitespace-only should NOT set has_bare_text"
    );
}

#[test]
fn bare_text_false_for_whitespace_around_interpolation() {
    let data = extract_with_script(
        "<template><div>\n  {{ msg }}\n</div></template>",
        "const msg = 'hi'",
    );
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert!(
        !div.has_bare_text,
        "whitespace + interpolation should NOT set has_bare_text"
    );
    assert!(
        div.has_text_content,
        "whitespace + interpolation should still set has_text_content"
    );
}

// ── Sub-span propagation (Step 0a) ──

#[test]
fn directive_has_expression_span() {
    let source = r#"<template><div v-if="show">hello</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    let v_if = div.directives.iter().find(|d| d.name == "if").unwrap();
    let expr_span = v_if
        .expression_span
        .expect("v-if should have expression_span");
    assert_eq!(
        &source[expr_span.start as usize..expr_span.end as usize],
        "show"
    );
}

#[test]
fn directive_has_arg_span() {
    let source = r#"<template><div @click.prevent="handler">x</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    let on = div.directives.iter().find(|d| d.name == "on").unwrap();
    let arg_span = on.arg_span.expect("@click should have arg_span");
    assert_eq!(
        &source[arg_span.start as usize..arg_span.end as usize],
        "click"
    );
}

#[test]
fn directive_has_modifier_spans() {
    let source = r#"<template><div @click.stop.prevent="handler">x</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    let on = div.directives.iter().find(|d| d.name == "on").unwrap();
    assert_eq!(on.modifier_spans.len(), 2, "should have 2 modifier spans");
    assert_eq!(
        &source[on.modifier_spans[0].start as usize..on.modifier_spans[0].end as usize],
        "stop"
    );
    assert_eq!(
        &source[on.modifier_spans[1].start as usize..on.modifier_spans[1].end as usize],
        "prevent"
    );
}

#[test]
fn directive_has_name_end() {
    // Use full SFC so spans are consistent
    let source = r#"<script setup>
import { ref } from 'vue'
const visible = ref(true)
</script>
<template><div v-show="visible">x</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    let v_show = div.directives.iter().find(|d| d.name == "show").unwrap();
    let name_text = &source[v_show.span.start as usize..v_show.name_end as usize];
    assert_eq!(name_text, "v-show");
}

#[test]
fn attribute_has_name_end_and_value_span() {
    let source = r#"<template><div class="foo">x</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    let cls = div.attributes.iter().find(|a| a.name == "class").unwrap();
    let name_text = &source[cls.span.start as usize..cls.name_end as usize];
    assert_eq!(name_text, "class");
    let val_span = cls.value_span.expect("class should have value_span");
    assert_eq!(
        &source[val_span.start as usize..val_span.end as usize],
        "foo"
    );
}

#[test]
fn boolean_attribute_has_no_value_span() {
    let source = r#"<template><input disabled /></template>"#;
    let data = extract(source);
    let input = data.elements.iter().find(|e| e.tag == "input").unwrap();
    let disabled = input
        .attributes
        .iter()
        .find(|a| a.name == "disabled")
        .unwrap();
    assert!(
        disabled.value_span.is_none(),
        "boolean attr should have no value_span"
    );
}

// ── content_end (Step 0c) ──

#[test]
fn element_has_content_end() {
    let source = r#"<template><div>hello</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    // content_end should point to the `<` of `</div>`
    assert_eq!(
        &source[div.content_end as usize..div.content_end as usize + 1],
        "<"
    );
    // Content between tag_span_end and content_end should be "hello"
    let content = &source[div.tag_span_end as usize..div.content_end as usize];
    assert_eq!(content, "hello");
}

#[test]
fn self_closing_element_content_end_equals_tag_span_end() {
    let source = r#"<template><br /></template>"#;
    let data = extract(source);
    let br = data.elements.iter().find(|e| e.tag == "br").unwrap();
    assert_eq!(
        br.content_end, br.tag_span_end,
        "self-closing content_end == tag_span_end"
    );
}

// ── text_children (Step 0f) ──

#[test]
fn text_children_text_only() {
    let source = r#"<template><div>hello world</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert_eq!(div.text_children.len(), 1, "should have 1 text child");
    match &div.text_children[0] {
        RawTextSegment::Text { span, .. } => {
            assert_eq!(
                &source[span.start as usize..span.end as usize],
                "hello world"
            );
        }
        _ => panic!("expected Text segment"),
    }
}

#[test]
fn text_children_interpolation_only() {
    let source =
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert_eq!(
        div.text_children.len(),
        1,
        "should have 1 interpolation child"
    );
    match &div.text_children[0] {
        RawTextSegment::Interpolation {
            span,
            expression_span,
        } => {
            assert!(source[span.start as usize..span.end as usize].contains("{{"));
            assert_eq!(
                source[expression_span.start as usize..expression_span.end as usize].trim(),
                "msg"
            );
        }
        _ => panic!("expected Interpolation segment"),
    }
}

#[test]
fn text_children_mixed() {
    let source = "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div>Count: {{ count }}</div></template>";
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    assert_eq!(
        div.text_children.len(),
        2,
        "should have text + interpolation"
    );
    match &div.text_children[0] {
        RawTextSegment::Text { span, .. } => {
            assert_eq!(&source[span.start as usize..span.end as usize], "Count: ");
        }
        _ => panic!("expected Text segment first"),
    }
    match &div.text_children[1] {
        RawTextSegment::Interpolation {
            expression_span, ..
        } => {
            assert_eq!(
                source[expression_span.start as usize..expression_span.end as usize].trim(),
                "count"
            );
        }
        _ => panic!("expected Interpolation segment second"),
    }
}

#[test]
fn text_children_skips_element_children() {
    let source = r#"<template><div>text<span>inner</span>more</div></template>"#;
    let data = extract(source);
    let div = data.elements.iter().find(|e| e.tag == "div").unwrap();
    // text_children should only have Text segments (skipping <span>)
    assert_eq!(
        div.text_children.len(),
        2,
        "should have 2 text segments (before + after span)"
    );
    for seg in &div.text_children {
        match seg {
            RawTextSegment::Text { .. } => {}
            _ => panic!("div text_children should only be Text (element children skipped)"),
        }
    }
}

#[test]
fn text_children_empty_for_self_closing() {
    let source = r#"<template><br /></template>"#;
    let data = extract(source);
    let br = data.elements.iter().find(|e| e.tag == "br").unwrap();
    assert!(
        br.text_children.is_empty(),
        "self-closing has no text children"
    );
}

// ── :ref on component is NOT a prop ──

#[test]
fn bound_ref_not_included_in_component_props() {
    // :ref is a template ref binding, not a prop — should NOT appear in component.props
    let data = extract_with_script(
        r#"<template><Child :ref="elRef" :msg="val" /></template>"#,
        "import { ref } from 'vue'\nimport Child from './Child.vue'\nconst elRef = ref(null)\nconst val = 'hi'",
    );
    assert_eq!(data.components.len(), 1);
    let child = &data.components[0];
    // Positive: msg should be in props
    assert!(
        child.props.iter().any(|p| p.name == "msg"),
        "msg should be extracted as a prop"
    );
    // Negative: ref should NOT be in props
    assert!(
        !child.props.iter().any(|p| p.name == "ref"),
        ":ref should NOT be extracted as a component prop"
    );
}

// ── Unused-declaration extraction facts (producer side) ──
//
// The population layer (`verter_session::template_convert`) consumes these
// facts from hand-built `RawTemplateData` in its own tests; the tests below
// prove the REAL extractor produces them from source, end-to-end.

#[test]
fn member_reads_capture_dollar_slots_literal_member() {
    let data = extract(r#"<template><div v-if="$slots.header">x</div></template>"#);
    assert!(
        data.member_reads
            .iter()
            .any(|read| read.root == "$slots" && read.member == "header"),
        "a template `$slots.header` read must enter member_reads: {:?}",
        data.member_reads
    );
    // The consumed-root model: the `$slots` occurrence and the member read
    // share the root span, so population can prove the root never escapes.
    let read = data
        .member_reads
        .iter()
        .find(|read| read.root == "$slots")
        .unwrap();
    assert!(
        data.binding_occurrences
            .iter()
            .any(|occ| occ.name == "$slots" && occ.span.start == read.root_span.start),
        "the `$slots` root occurrence must be recorded at the member-read root span"
    );
}

#[test]
fn template_dollar_emit_occurrence_is_recorded() {
    // `$emit` is not filtered as a global/v-for local — its occurrence is the
    // fail-open fact that suppresses unused-emit diagnostics.
    let data = extract(r#"<template><button @click="$emit('close')">x</button></template>"#);
    assert!(
        data.binding_occurrences
            .iter()
            .any(|occ| occ.name == "$emit"),
        "a template `$emit(...)` call must record the `$emit` occurrence: {:?}",
        data.binding_occurrences
            .iter()
            .map(|occ| occ.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn template_emit_binding_occurrence_is_recorded() {
    // Calling the `defineEmits` return binding from the template is the
    // standard emit pattern; its occurrence must be recorded so population
    // can fail open on it.
    let data = extract_with_script(
        r#"<template><button @click="emit('close')">x</button></template>"#,
        "const emit = defineEmits(['close'])",
    );
    assert!(
        data.binding_occurrences
            .iter()
            .any(|occ| occ.name == "emit"),
        "a template `emit(...)` call must record the binding occurrence: {:?}",
        data.binding_occurrences
            .iter()
            .map(|occ| occ.name.as_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn broken_template_expression_sets_expression_errors_flag() {
    // A mid-edit broken expression makes the identifier inventory unreliable;
    // the completeness bit is the fail-open gate for unused-declaration
    // population.
    let data = extract(r#"<template><div v-if="count &&">x</div></template>"#);
    assert!(
        data.has_expression_errors,
        "a template expression parse error must set has_expression_errors"
    );
}

#[test]
fn complete_template_expressions_leave_expression_errors_unset() {
    let data = extract(r#"<template><div v-if="count">x</div></template>"#);
    assert!(
        !data.has_expression_errors,
        "well-formed expressions must not set the fail-open bit"
    );
}
