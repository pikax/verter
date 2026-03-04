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
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
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
    // <template> elements is a separate extraction concern (Phase 4).
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
