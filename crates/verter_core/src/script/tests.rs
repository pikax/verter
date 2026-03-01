use super::*;
use crate::types::NodeTag;

/// Helper to create a RootNodeScript for testing.
fn make_script(source: &str, tag_open_str: &str, is_setup: bool) -> (RootNodeScript, String) {
    // Build a minimal SFC with the script block
    let tag_open_end = tag_open_str.len() as u32;
    let content_start = tag_open_end;
    let content_end = content_start + source.len() as u32;
    let close_tag_start = content_end;
    let close_tag_end = close_tag_start + "</script>".len() as u32;

    let full_source = format!("{}{}</script>", tag_open_str, source);

    let script = RootNodeScript {
        tag_open: NodeTag {
            start: 0,
            end: tag_open_end,
            name_end: if is_setup {
                // <script setup> → name_end points past "script"
                8 // past "script" in "<script "
            } else {
                7 // past "script" in "<script"
            },
        },
        tag_close: Some(NodeTag {
            start: close_tag_start,
            end: close_tag_end,
            name_end: close_tag_end - 1,
        }),
        is_setup,
        lang: None,
        src: None,
        generic: None,
        attributes: Vec::new(),
        content: Some(crate::common::Span::new(content_start, content_end)),
    };

    (script, full_source)
}

// ── Test 1: No script blocks ──────────────────────────────────

#[test]
fn no_script_blocks_returns_empty() {
    let alloc = Allocator::default();
    let source = "<template><div>hi</div></template>";
    let mut ct = CodeTransform::new(source, &alloc);

    let result = generate_script(None, None, source, &mut ct, &alloc, &Default::default());

    assert!(result.bindings.is_empty());
    assert!(result.inline_inject_pos.is_none());
    // No imports expected for empty script
}

// ── Test 2: Empty script setup → component wrapper ────────────

#[test]
fn empty_script_setup_produces_wrapper() {
    let alloc = Allocator::default();
    let (setup, full) = make_script("", "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Should contain component wrapper
    assert!(output.contains("const __sfc__"), "output: {}", output);
    assert!(output.contains("_defineComponent"), "output: {}", output);
    assert!(output.contains("__name: 'Test'"), "output: {}", output);
    assert!(output.contains("setup(__props)"), "output: {}", output);
    assert!(
        output.contains("export default __sfc__"),
        "output: {}",
        output
    );
}

// ── Test 3: Script setup with const → binding extracted ───────

#[test]
fn script_setup_extracts_bindings() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hello'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    // Should have extracted "msg" as a binding
    assert!(
        result.bindings.contains_key("msg"),
        "bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
}

// ── Test 4: Script setup with ref → SetupRef binding ──────────

#[test]
fn script_setup_ref_binding_type() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Counter",
            ..Default::default()
        },
    );

    // "count" should be SetupRef
    assert_eq!(
        result.bindings.get("count").copied(),
        Some(BindingType::SetupRef),
        "bindings: {:?}",
        result.bindings
    );
}

// ── Test 5: Import hoisting ───────────────────────────────────

#[test]
fn script_setup_hoists_imports() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Import should appear BEFORE the component definition
    let import_pos = output.find("import { ref } from 'vue'");
    let component_pos = output.find("const __sfc__");
    assert!(
        import_pos.is_some(),
        "import not found in output: {}",
        output
    );
    assert!(
        import_pos.unwrap() < component_pos.unwrap(),
        "import should come before component definition. output: {}",
        output
    );
}

// ── Test 6: Non-inline returns __returned__ ───────────────────

#[test]
fn non_inline_has_return_statement() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hello'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            inline_template: false,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("const __returned__ = {"),
        "should have return statement. output: {}",
        output
    );
    assert!(
        output.contains("__isScriptSetup"),
        "should have __isScriptSetup marker. output: {}",
        output
    );
}

// ── Test 7: Inline mode has no return, sets inject pos ────────

#[test]
fn inline_mode_no_return_has_inject_pos() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hello'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            inline_template: true,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        !output.contains("return {"),
        "inline mode should not have return statement. output: {}",
        output
    );
    assert!(
        result.inline_inject_pos.is_some(),
        "inline mode should have inject pos"
    );
}

// ── Test 8: Options API script ────────────────────────────────

#[test]
fn options_api_script_wraps_export() {
    let alloc = Allocator::default();
    let content = "\nexport default {\n  data() {\n    return { count: 0 }\n  }\n}\n";
    let (script, full) = make_script(content, "<script>", false);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        Some(&script),
        None,
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("const __sfc__"),
        "should define __sfc__. output: {}",
        output
    );
    assert!(
        output.contains("export default __sfc__"),
        "should export __sfc__. output: {}",
        output
    );
}

// ── Test 9: TypeScript types stripped by default ───────────────

#[test]
fn ts_types_stripped_by_default() {
    let alloc = Allocator::default();
    let content = "\ninterface Props { msg: string }\nconst msg = 'hi'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            keep_ts_types: false,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Interface should be removed
    assert!(
        !output.contains("interface Props"),
        "interface should be stripped. output: {}",
        output
    );
}

// ── Test 10: TypeScript types kept when keep_ts_types ─────────

#[test]
fn ts_types_hoisted_when_keep_ts_types() {
    let alloc = Allocator::default();
    let content = "\ninterface Props { msg: string }\nconst msg = 'hi'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            keep_ts_types: true,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Interface should be hoisted to file top (before component definition)
    let iface_pos = output.find("interface Props");
    let component_pos = output.find("const __sfc__");
    assert!(
        iface_pos.is_some(),
        "interface should be present. output: {}",
        output
    );
    assert!(
        iface_pos.unwrap() < component_pos.unwrap(),
        "interface should be before component. output: {}",
        output
    );
}

// ── Test 11: Scoped style adds __scopeId ──────────────────────

#[test]
fn scoped_style_adds_scope_id() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hi'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            scope_id: "data-v-abc123",
            has_scoped_style: true,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("__sfc__.__scopeId = \"data-v-abc123\""),
        "should add scopeId. output: {}",
        output
    );
}

// ── Test 12: _defineComponent is in imports ───────────────────

#[test]
fn define_component_in_imports() {
    let alloc = Allocator::default();
    let (setup, full) = make_script("", "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    assert!(
        result.imports.contains(&"_defineComponent"),
        "imports: {:?}",
        result.imports
    );
}

// ── Test 13: CSS v-bind vars inject _useCssVars ─────────────

#[test]
fn css_v_binds_inject_use_css_vars() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let v_binds = vec![crate::css::types::VBindVar {
        expression: "count".to_string(),
        var_name: "abc-count".to_string(),
    }];

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            css_v_binds: &v_binds,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_useCssVars"),
        "should inject _useCssVars. output: {}",
        output
    );
    assert!(
        output.contains("count.value"),
        "ref binding should use .value. output: {}",
        output
    );
    assert!(
        result.imports.contains(&"_useCssVars"),
        "imports should include _useCssVars: {:?}",
        result.imports
    );
}

// ── Test 14: defineProps with runtime args ──────────────────────

#[test]
fn define_props_runtime_object() {
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps({ title: String, count: Number })\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Props section should appear in the component definition
    assert!(
        output.contains("props: { title: String, count: Number }"),
        "should have props section. output: {}",
        output
    );
    // The macro call should be replaced with __props
    assert!(
        output.contains("const props = __props"),
        "should replace defineProps with __props. output: {}",
        output
    );
    // "title" should be in bindings as Props
    assert_eq!(
        result.bindings.get("props").copied(),
        Some(BindingType::SetupConst),
        "bindings: {:?}",
        result.bindings
    );
}

// ── Test 15: defineProps with array args ──────────────────────

#[test]
fn define_props_runtime_array() {
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps(['title', 'count'])\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("props: ['title', 'count']"),
        "should have array props section. output: {}",
        output
    );
    assert!(
        output.contains("const props = __props"),
        "should replace defineProps with __props. output: {}",
        output
    );
}

// ── Test 16: defineEmits with array ──────────────────────────

#[test]
fn define_emits_runtime_array() {
    let alloc = Allocator::default();
    let content = "\nconst emit = defineEmits(['click', 'update'])\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("emits: ['click', 'update']"),
        "should have emits section. output: {}",
        output
    );
    assert!(
        output.contains("const emit = __emit"),
        "should replace defineEmits with __emit. output: {}",
        output
    );
    // Setup context should destructure emit
    assert!(
        output.contains("emit: __emit"),
        "should have emit in setup context. output: {}",
        output
    );
}

// ── Test 17: defineExpose ─────────────────────────────────────

#[test]
fn define_expose_replaces_with_dunder_expose() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hi'\ndefineExpose({ msg })\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("__expose({ msg })"),
        "should replace defineExpose with __expose. output: {}",
        output
    );
    assert!(
        !output.contains("defineExpose"),
        "should not contain defineExpose. output: {}",
        output
    );
    // Setup context should destructure expose
    assert!(
        output.contains("expose: __expose"),
        "should have expose in setup context. output: {}",
        output
    );
}

// ── Test 18: defineSlots ──────────────────────────────────────

#[test]
fn define_slots_replaces_with_use_slots() {
    let alloc = Allocator::default();
    let content = "\nconst slots = defineSlots()\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_useSlots()"),
        "should replace defineSlots with _useSlots(). output: {}",
        output
    );
    assert!(
        !output.contains("defineSlots"),
        "should not contain defineSlots. output: {}",
        output
    );
    assert!(
        result.imports.contains(&"_useSlots"),
        "imports should include _useSlots: {:?}",
        result.imports
    );
}

// ── Test 19: defineOptions ────────────────────────────────────

#[test]
fn define_options_extracts_to_component_level() {
    let alloc = Allocator::default();
    let content = "\ndefineOptions({ inheritAttrs: false })\nconst msg = 'hi'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Options should appear at component level (before __name)
    assert!(
        output.contains("inheritAttrs: false"),
        "should include options content. output: {}",
        output
    );
    assert!(
        !output.contains("defineOptions"),
        "should not contain defineOptions. output: {}",
        output
    );
}

// ── Test 20: defineModel ──────────────────────────────────────

#[test]
fn define_model_replaces_with_use_model() {
    let alloc = Allocator::default();
    let content = "\nconst model = defineModel()\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_useModel(__props, 'modelValue')"),
        "should replace defineModel with _useModel. output: {}",
        output
    );
    assert!(
        !output.contains("defineModel"),
        "should not contain defineModel. output: {}",
        output
    );
    assert!(
        result.imports.contains(&"_useModel"),
        "imports should include _useModel: {:?}",
        result.imports
    );
}

// ── Test 20b: defineModel with named model ────────────────────

#[test]
fn define_model_named_replaces_with_use_model() {
    let alloc = Allocator::default();
    let content = "\nconst show = defineModel('show')\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_useModel(__props, 'show')"),
        "should replace defineModel('show') with _useModel(__props, 'show'). output: {}",
        output
    );
    assert!(
        !output.contains("''show''"),
        "should not have double-quoted name. output: {}",
        output
    );
    assert!(
        result.imports.contains(&"_useModel"),
        "imports should include _useModel: {:?}",
        result.imports
    );
}

// ── Test 21: defineProps + defineEmits combined ───────────────

#[test]
fn define_props_and_emits_combined() {
    let alloc = Allocator::default();
    let content =
        "\nconst props = defineProps({ msg: String })\nconst emit = defineEmits(['click'])\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("props: { msg: String }"),
        "should have props section. output: {}",
        output
    );
    assert!(
        output.contains("emits: ['click']"),
        "should have emits section. output: {}",
        output
    );
    assert!(
        output.contains("emit: __emit"),
        "should destructure emit. output: {}",
        output
    );
}

// ── Test 22: macro output is valid JS ─────────────────────────

#[test]
fn macro_output_is_valid_js() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst props = defineProps({ title: String })\nconst emit = defineEmits(['click'])\ndefineExpose({ title: props.title })\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "MacroTest",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "macro output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

// ── Test 23: Output is valid JS (basic) ──────────────────────────

#[test]
fn output_is_valid_js() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Counter",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Validate JS syntax with OXC parser
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

// ── Test 24: defineProps extracts prop bindings for resolver ──────

#[test]
fn define_props_object_extracts_prop_bindings() {
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps({ title: String, count: Number })\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    // Prop names should be extracted as Props bindings for template resolver
    assert_eq!(
        result.bindings.get("title").copied(),
        Some(BindingType::Props),
        "title should be Props. bindings: {:?}",
        result.bindings
    );
    assert_eq!(
        result.bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props. bindings: {:?}",
        result.bindings
    );
}

// ── Test 25: defineProps array extracts prop bindings ─────────────

#[test]
fn define_props_array_extracts_prop_bindings() {
    let alloc = Allocator::default();
    let content = "\ndefineProps(['title', 'count'])\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    assert_eq!(
        result.bindings.get("title").copied(),
        Some(BindingType::Props),
        "title should be Props. bindings: {:?}",
        result.bindings
    );
    assert_eq!(
        result.bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props. bindings: {:?}",
        result.bindings
    );
}

// ── Test 26: Inline mode inject_pos points to close tag ──────────

#[test]
fn inline_mode_inject_pos_at_close_tag() {
    let alloc = Allocator::default();
    let content = "\nconst msg = 'hello'\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let close_tag_start = setup.tag_close.as_ref().unwrap().start;
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            inline_template: true,
            ..Default::default()
        },
    );

    // inject_pos should be the start of </script> tag
    assert_eq!(
        result.inline_inject_pos,
        Some(close_tag_start),
        "inject_pos should be at close tag start"
    );
}

// ── Test 27: Returned object includes setup bindings only ────────

#[test]
fn returned_object_includes_only_setup_bindings() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\nconst msg = 'hi'\ndefineProps({ title: String })\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            inline_template: false,
            ..Default::default()
        },
    );

    let output = ct.build_string();
    // Return should include setup bindings (count, msg) but not props (title)
    assert!(
        output.contains("const __returned__ = {"),
        "should have return statement. output: {}",
        output
    );
    let return_idx = output.find("const __returned__ = {").unwrap();
    let return_end = output[return_idx..].find('}').unwrap() + return_idx + 1;
    let return_obj = &output[return_idx..return_end];
    assert!(
        return_obj.contains("count"),
        "return should include count. return: {}",
        return_obj
    );
    assert!(
        return_obj.contains("msg"),
        "return should include msg. return: {}",
        return_obj
    );
    assert!(
        !return_obj.contains("title"),
        "return should NOT include prop 'title'. return: {}",
        return_obj
    );
}

// ── Test 28: Output structure order ──────────────────────────────

#[test]
fn output_structure_order() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            has_scoped_style: true,
            scope_id: "data-v-abc",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Verify ordering: import → component def → scopeId → export
    let import_pos = output.find("import { ref }").unwrap();
    let component_pos = output.find("const __sfc__").unwrap();
    let scope_pos = output.find("__sfc__.__scopeId").unwrap();
    let export_pos = output.find("export default __sfc__").unwrap();

    assert!(import_pos < component_pos, "import before component");
    assert!(component_pos < scope_pos, "component before scopeId");
    assert!(scope_pos < export_pos, "scopeId before export");
}

// ── Test 29: E2E complex SFC — all features valid JS ─────────────

#[test]
fn e2e_complex_sfc_valid_js() {
    let alloc = Allocator::default();
    let content = "\nimport { ref, computed } from 'vue'\nimport MyComponent from './MyComponent.vue'\n\nconst props = defineProps({ title: String, modelValue: Number })\nconst emit = defineEmits(['update:modelValue', 'click'])\ndefineOptions({ inheritAttrs: false })\ndefineExpose({ title: props.title })\n\nconst count = ref(0)\nconst doubled = computed(() => count.value * 2)\nconst msg = 'hello'\n\nfunction increment() {\n  count.value++\n  emit('click', count.value)\n}\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let v_binds = vec![crate::css::types::VBindVar {
        expression: "count".to_string(),
        var_name: "abc-count".to_string(),
    }];

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "ComplexComponent",
            has_scoped_style: true,
            scope_id: "data-v-abc123",
            css_v_binds: &v_binds,
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "complex SFC should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );

    // Verify key structural elements
    assert!(output.contains("inheritAttrs: false"), "output: {}", output);
    assert!(
        output.contains("__name: 'ComplexComponent'"),
        "output: {}",
        output
    );
    assert!(
        output.contains("props: { title: String, modelValue: Number }"),
        "output: {}",
        output
    );
    assert!(
        output.contains("emits: ['update:modelValue', 'click']"),
        "output: {}",
        output
    );
    assert!(output.contains("expose: __expose"), "output: {}", output);
    assert!(output.contains("emit: __emit"), "output: {}", output);
    assert!(output.contains("_useCssVars"), "output: {}", output);
    assert!(
        output.contains("__sfc__.__scopeId = \"data-v-abc123\""),
        "output: {}",
        output
    );

    // Verify bindings
    assert_eq!(
        result.bindings.get("count").copied(),
        Some(BindingType::SetupRef)
    );
    // computed() returns a ComputedRef → classified as SetupRef
    assert_eq!(
        result.bindings.get("doubled").copied(),
        Some(BindingType::SetupRef)
    );
    // 'hello' is a literal → classified as LiteralConst
    assert_eq!(
        result.bindings.get("msg").copied(),
        Some(BindingType::LiteralConst)
    );
    assert_eq!(
        result.bindings.get("title").copied(),
        Some(BindingType::Props)
    );
    assert_eq!(
        result.bindings.get("modelValue").copied(),
        Some(BindingType::Props)
    );

    // Verify imported component binding
    assert_eq!(
        result.bindings.get("MyComponent").copied(),
        Some(BindingType::SetupImport),
        "Imported .vue component should be SetupImport binding. All bindings: {:?}",
        result.bindings
    );

    // Verify imports
    assert!(result.imports.contains(&"_defineComponent"));
    assert!(result.imports.contains(&"_useCssVars"));
}

// ── Test 30: E2E inline mode valid JS ────────────────────────────

#[test]
fn e2e_inline_mode_valid_js() {
    let alloc = Allocator::default();
    let content = "\nimport { ref } from 'vue'\nconst props = defineProps({ title: String })\nconst count = ref(0)\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "InlineTest",
            inline_template: true,
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "inline mode output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );

    // No return statement in inline mode
    assert!(
        !output.contains("return {"),
        "inline mode should not have return. output: {}",
        output
    );

    // inject_pos should be set
    assert!(result.inline_inject_pos.is_some());

    // Bindings should still be populated
    assert_eq!(
        result.bindings.get("count").copied(),
        Some(BindingType::SetupRef)
    );
    assert_eq!(
        result.bindings.get("title").copied(),
        Some(BindingType::Props)
    );
}

// ── Test 31: Multiple imports hoisted correctly ───────────────────

#[test]
fn multiple_imports_all_hoisted() {
    let alloc = Allocator::default();
    let content = "\nimport { ref, computed } from 'vue'\nimport { useRoute } from 'vue-router'\nconst count = ref(0)\nconst route = useRoute()\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Both imports should be before component definition.
    // `computed` is not used in the script body so it gets elided in force_js mode.
    let vue_import_pos = output.find("import { ref } from 'vue'").unwrap();
    let router_import_pos = output
        .find("import { useRoute } from 'vue-router'")
        .unwrap();
    let component_pos = output.find("const __sfc__").unwrap();

    assert!(
        vue_import_pos < component_pos,
        "vue import before component"
    );
    assert!(
        router_import_pos < component_pos,
        "router import before component"
    );

    // Validate JS
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

// ── Test 32: Multiple defineModel calls deduplicates imports ──────

#[test]
fn multiple_define_model_deduplicates_imports() {
    let alloc = Allocator::default();
    let content = "\nconst model1 = defineModel()\nconst model2 = defineModel('title')\nconst model3 = defineModel('count')\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "MultiModel",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // _useModel should appear in imports exactly once
    let use_model_count = result.imports.iter().filter(|&&i| i == "_useModel").count();
    assert_eq!(
        use_model_count, 1,
        "_useModel should appear exactly once in imports, got {}. imports: {:?}",
        use_model_count, result.imports
    );

    // Output should be valid JS (no duplicate import specifiers)
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "multiple defineModel output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );

    // All three models should be replaced
    assert!(
        output.contains("_useModel(__props, 'modelValue')"),
        "default model. output: {}",
        output
    );
    assert!(
        output.contains("_useModel(__props, 'title')"),
        "title model. output: {}",
        output
    );
    assert!(
        output.contains("_useModel(__props, 'count')"),
        "count model. output: {}",
        output
    );
}

// ── Test 33: Async setup ─────────────────────────────────────────

#[test]
fn async_setup_produces_async_wrapper() {
    let alloc = Allocator::default();
    let content = "\nconst data = await fetch('/api')\n";
    let (setup, full) = make_script(content, "<script setup>", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "AsyncTest",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("async setup(__props)"),
        "should have async setup. output: {}",
        output
    );
}

// ── Test 34: withDefaults method shorthand produces valid arrow function ──

#[test]
fn with_defaults_method_shorthand_produces_arrow_function() {
    let alloc = Allocator::default();
    let content = r#"
withDefaults(defineProps<{
  validateOn?: string[]
  color?: string
}>(), {
  validateOn() { return ['input', 'blur'] },
  color: 'primary'
})
"#;
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "FormTest",
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Method shorthand should be converted to arrow function
    assert!(
        !output.contains("default: () {"),
        "should NOT contain invalid method shorthand 'default: () {{'. output: {}",
        output
    );
    assert!(
        output.contains("default: () => {"),
        "method shorthand should be converted to arrow function. output: {}",
        output
    );

    // Non-method default should remain as-is
    assert!(
        output.contains("default: 'primary'"),
        "non-method default should be unchanged. output: {}",
        output
    );

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "withDefaults with method shorthand should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

// ── External type resolution tests ──────────────────────────────

/// Helper to build pre-resolved external types from a fake external file source.
fn make_external_types(
    type_name: &str,
    dep_source: &str,
) -> rustc_hash::FxHashMap<String, crate::utils::oxc::vue::ResolvedElements> {
    let alloc = oxc_allocator::Allocator::default();
    let resolved =
        crate::utils::oxc::vue::resolve_type::resolve_external_type(type_name, dep_source, &alloc)
            .expect("failed to resolve external type");
    let mut map = rustc_hash::FxHashMap::default();
    map.insert(type_name.to_string(), resolved);
    map
}

#[test]
fn external_type_defineprops_generates_runtime_props() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "MyProps",
        "export interface MyProps { title: string; count: number }",
    );
    let content = "\nimport type { MyProps } from './types'\ndefineProps<MyProps>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    // External props should be in bindings
    assert!(
        result.bindings.contains_key("title"),
        "bindings should contain 'title'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        result.bindings.contains_key("count"),
        "bindings should contain 'count'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );

    let output = ct.build_string();

    // Runtime props should be generated
    assert!(
        output.contains("title:"),
        "should generate runtime props for 'title'. output: {}",
        output
    );
    assert!(
        output.contains("count:"),
        "should generate runtime props for 'count'. output: {}",
        output
    );

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "External type defineProps should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

#[test]
fn external_type_defineprops_optional_prop() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "MyProps",
        "export interface MyProps { label?: string; required: boolean }",
    );
    let content = "\nimport type { MyProps } from './types'\ndefineProps<MyProps>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Optional prop should NOT have "required: true"
    // The label prop line should not contain "required: true"
    let lines: Vec<&str> = output.lines().collect();
    let label_line = lines.iter().find(|l| l.contains("label:"));
    if let Some(line) = label_line {
        assert!(
            !line.contains("required: true"),
            "optional prop 'label' should not be required. line: {}",
            line
        );
    }

    // Required prop SHOULD have "required: true"
    // (field name is "required" which is also a keyword, but the runtime output
    // should include "required: true" for the prop named "required")
    assert!(
        output.contains("required: true"),
        "non-optional prop should have 'required: true'. output: {}",
        output
    );
}

#[test]
fn external_type_defineemits_generates_emits_section() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "MyEmits",
        "export interface MyEmits { (e: 'change', value: string): void; (e: 'update'): void }",
    );
    let content = "\nimport type { MyEmits } from './events'\ndefineEmits<MyEmits>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let _result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // Emits should be in the component definition
    assert!(
        output.contains("\"change\""),
        "should generate emit for 'change'. output: {}",
        output
    );
    assert!(
        output.contains("\"update\""),
        "should generate emit for 'update'. output: {}",
        output
    );

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "External type defineEmits should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

#[test]
fn external_type_with_no_matching_type_falls_back() {
    let alloc = Allocator::default();
    // Provide external types but for a different name
    let external_types =
        make_external_types("OtherProps", "export interface OtherProps { x: string }");
    let content = "\nimport type { UnknownType } from './types'\ndefineProps<UnknownType>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    // No props should be resolved — unknown type is not in external_types
    let has_props = result
        .bindings
        .iter()
        .any(|(_, bt)| bt == &crate::template::code_gen::binding::BindingType::Props);
    assert!(
        !has_props,
        "should have no prop bindings for unresolved type. Got: {:?}",
        result.bindings
    );
}

#[test]
fn external_type_props_dont_conflict_with_companion_types() {
    let alloc = Allocator::default();
    // External file has a prop "shared" and "external"
    let external_types = make_external_types(
        "ExtProps",
        "export interface ExtProps { shared: string; external: number }",
    );
    // Companion <script> block declares a type "ExtProps" with only "shared"
    // External types should NOT override companion types (companion wins)
    let companion_content = "\nexport interface ExtProps { shared: boolean }\nexport default {}\n";
    let (_companion, _) = make_script(companion_content, "<script>", false);

    let setup_content = "\nimport type { ExtProps } from './types'\ndefineProps<ExtProps>()\n";
    let (_setup, _) = make_script(setup_content, "<script setup lang=\"ts\">", true);

    // Build full SFC with both blocks
    let full = format!(
        "<script>{}</script><script setup lang=\"ts\">{}</script>",
        companion_content, setup_content
    );
    // Rebuild script nodes with correct offsets in the full SFC
    let companion_tag_end = "<script>".len() as u32;
    let companion_content_start = companion_tag_end;
    let companion_content_end = companion_content_start + companion_content.len() as u32;
    let companion_close_start = companion_content_end;
    let companion_close_end = companion_close_start + "</script>".len() as u32;

    let setup_tag_start = companion_close_end;
    let setup_tag_end = setup_tag_start + "<script setup lang=\"ts\">".len() as u32;
    let setup_content_start = setup_tag_end;
    let setup_content_end = setup_content_start + setup_content.len() as u32;
    let setup_close_start = setup_content_end;
    let setup_close_end = setup_close_start + "</script>".len() as u32;

    let companion_node = RootNodeScript {
        tag_open: NodeTag {
            start: 0,
            end: companion_tag_end,
            name_end: 7,
        },
        tag_close: Some(NodeTag {
            start: companion_close_start,
            end: companion_close_end,
            name_end: companion_close_end - 1,
        }),
        is_setup: false,
        lang: None,
        src: None,
        generic: None,
        attributes: Vec::new(),
        content: Some(crate::common::Span::new(
            companion_content_start,
            companion_content_end,
        )),
    };

    let setup_node = RootNodeScript {
        tag_open: NodeTag {
            start: setup_tag_start,
            end: setup_tag_end,
            name_end: setup_tag_start + 8,
        },
        tag_close: Some(NodeTag {
            start: setup_close_start,
            end: setup_close_end,
            name_end: setup_close_end - 1,
        }),
        is_setup: true,
        lang: None,
        src: None,
        generic: None,
        attributes: Vec::new(),
        content: Some(crate::common::Span::new(
            setup_content_start,
            setup_content_end,
        )),
    };

    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        Some(&companion_node),
        Some(&setup_node),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    // "shared" should be in bindings (from companion, takes precedence)
    assert!(
        result.bindings.contains_key("shared"),
        "should have 'shared' from companion. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
}

/// @ai-generated — withDefaults + external type: prop names must use pre-resolved key_name,
/// not span extraction (which indexes into the wrong source for external types).
#[test]
fn external_type_withdefaults_uses_key_name() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "ExternalProps",
        "export interface ExternalProps { title?: string; description?: string; color?: string }",
    );
    let content =
        "\nimport type { ExternalProps } from './types'\nconst props = withDefaults(defineProps<ExternalProps>(), {\n  color: 'primary'\n})\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // All three props should appear with correct names
    assert!(
        output.contains("title:"),
        "should have 'title' prop from external type. output:\n{}",
        output
    );
    assert!(
        output.contains("description:"),
        "should have 'description' prop from external type. output:\n{}",
        output
    );
    assert!(
        output.contains("color:") && output.contains("default: 'primary'"),
        "should have 'color' prop with default from withDefaults. output:\n{}",
        output
    );

    // External props should be in bindings
    assert!(
        result.bindings.contains_key("title"),
        "bindings should contain 'title'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "External type withDefaults should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

/// @ai-generated — typeof external value: `defineProps<typeof MyType>()` with exported const
#[test]
fn typeof_external_value_generates_runtime_props() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "MyType",
        "export const MyType: { foo: string; bar: number } = { foo: '', bar: 0 }",
    );
    let content = "\nimport { MyType } from './types'\ndefineProps<typeof MyType>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    // External props should be in bindings
    assert!(
        result.bindings.contains_key("foo"),
        "bindings should contain 'foo'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        result.bindings.contains_key("bar"),
        "bindings should contain 'bar'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );

    let output = ct.build_string();

    // Runtime props should be generated
    assert!(
        output.contains("foo:"),
        "should generate runtime props for 'foo'. output: {}",
        output
    );
    assert!(
        output.contains("bar:"),
        "should generate runtime props for 'bar'. output: {}",
        output
    );

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "typeof external value defineProps should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

/// @ai-generated — typeof external value without type annotation (infer from object literal)
#[test]
fn typeof_external_value_infers_from_object_literal() {
    let alloc = Allocator::default();
    let external_types = make_external_types(
        "defaults",
        "export const defaults = { name: 'test', count: 42, active: true }",
    );
    let content = "\nimport { defaults } from './config'\ndefineProps<typeof defaults>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);

    let result = generate_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            external_types: Some(external_types),
            ..Default::default()
        },
    );

    // External props should be in bindings
    assert!(
        result.bindings.contains_key("name"),
        "bindings should contain 'name'. Got: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );

    let output = ct.build_string();

    // Validate JS syntax
    let js_alloc = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::mjs();
    let parser_result = oxc_parser::Parser::new(&js_alloc, &output, source_type).parse();
    assert!(
        parser_result.errors.is_empty(),
        "typeof object literal defineProps should produce valid JS.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}
