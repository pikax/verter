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
        attrs: None,
        attributes: Vec::new(),
        content: Some(crate::common::Span::new(content_start, content_end)),
    };

    (script, full_source)
}

/// Build the prepared script for the given blocks and run script codegen,
/// mirroring the production flow where `PreparedScript` is built once and handed
/// to `generate_script`.
fn gen_script<'a>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    source: &'a str,
    ct: &mut CodeTransform<'a>,
    alloc: &'a Allocator,
    options: &ScriptCodeGenOptions<'_>,
) -> ScriptCodeGenResult<'a> {
    let prepared =
        crate::script::prepared::PreparedScript::build(source, script, script_setup, alloc);
    let result = generate_script(script, script_setup, &prepared, source, ct, alloc, options);

    // Mirror the production compile pipeline: under force_js the whole-program
    // body strip runs AFTER generate_script (compile/mod.rs), owning all TS
    // removal (annotations, casts, generics, type-only imports, type decls).
    // Running it here keeps these unit tests faithful to real output.
    if !options.keep_ts_types {
        if let Some(setup) = prepared.setup() {
            crate::strip_types::typescript::strip_typescript_body_types(
                setup.program(),
                ct,
                setup.content_start(),
                setup.content_str(),
            );
        }
        if let Some(companion) = prepared.companion() {
            crate::strip_types::typescript::strip_typescript_types(
                companion.program(),
                ct,
                companion.content_start(),
                companion.content_str(),
            );
        }
    }

    result
}

// ── Test 1: No script blocks ──────────────────────────────────

#[test]
fn no_script_blocks_returns_empty() {
    let alloc = Allocator::default();
    let source = "<template><div>hi</div></template>";
    let mut ct = CodeTransform::new(source, &alloc);

    let result = gen_script(None, None, source, &mut ct, &alloc, &Default::default());

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

    let _result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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
        output.contains("_useModel(__props, \"modelValue\")"),
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

    let result = gen_script(
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
        output.contains("_useModel(__props, \"show\")"),
        "should replace defineModel('show') with a safely quoted _useModel call. output: {}",
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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

    let result = gen_script(
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

    let _result = gen_script(
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

    let result = gen_script(
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
        output.contains("_useModel(__props, \"modelValue\")"),
        "default model. output: {}",
        output
    );
    assert!(
        output.contains("_useModel(__props, \"title\")"),
        "title model. output: {}",
        output
    );
    assert!(
        output.contains("_useModel(__props, \"count\")"),
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

    let _result = gen_script(
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

// ── Test 34: withDefaults method shorthand remains valid method syntax ──

#[test]
fn with_defaults_method_shorthand_produces_valid_default_method() {
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
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop(
                "validateOn",
                true,
                [verter_macro_dto::RuntimeConstructor::Array],
            ),
            crate::test_helpers::runtime_prop(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "FormTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();

    // The authored method body remains method syntax under the synthesized
    // `default` key. `default: () {}` would be invalid JavaScript.
    assert!(
        !output.contains("default: () {"),
        "should NOT contain invalid method shorthand 'default: () {{'. output: {}",
        output
    );
    assert!(
        output.contains("\"default\"() {"),
        "method shorthand should remain a valid default method. output: {}",
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

// ── force-js section stripping: withDefaults, multi-declarator, array names ──
//
// Every props/emits section that copies a macro argument verbatim must be
// TypeScript-stripped in force-js mode. The synthesized `withDefaults` sections
// and any non-first declarator's macro section are produced from raw source
// slices, so the section text — not just the residual script body — must be
// stripped. The output must additionally parse as plain JavaScript.

/// Assert a force-js code-gen output parses as ECMAScript modules (no TS).
fn assert_valid_js(output: &str) {
    let js_alloc = oxc_allocator::Allocator::default();
    let parser_result =
        oxc_parser::Parser::new(&js_alloc, output, oxc_span::SourceType::mjs()).parse();
    assert!(
        parser_result.errors.is_empty(),
        "force-js output must be valid JavaScript.\nOutput:\n{}\nErrors: {:?}",
        output,
        parser_result.errors
    );
}

#[test]
fn with_defaults_force_js_strips_ts_from_object_defaults() {
    // withDefaults copies each default value verbatim into the synthesized props
    // section; a `[] as string[]` default must lose its TS cast in force-js.
    let alloc = Allocator::default();
    let content = "\ninterface Props { items?: string[]; color?: string }\nconst props = withDefaults(defineProps<Props>(), { items: [] as string[], color: 'primary' })\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "items",
                true,
                [verter_macro_dto::RuntimeConstructor::Array],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "WD",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        !output.contains("as string[]"),
        "force-js must strip the `as string[]` cast from the default. output:\n{}",
        output
    );
    assert!(
        output.contains("default: []"),
        "stripped array default should remain `default: []`. output:\n{}",
        output
    );
    assert_valid_js(&output);
}

#[test]
fn with_defaults_force_js_strips_satisfies_and_non_null() {
    // `satisfies` and the non-null `!` are TS-only; a leaked `!` would make the
    // output invalid JS, so the mjs parse is the strongest guard here.
    let alloc = Allocator::default();
    let content = "\ninterface Props { count?: number; label?: string }\nconst defaultLabel = 'fallback'\nconst props = withDefaults(defineProps<Props>(), { count: 0 satisfies number, label: defaultLabel! })\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "count",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "label",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "WD",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        !output.contains("satisfies"),
        "force-js must strip `satisfies`. output:\n{}",
        output
    );
    assert!(
        !output.contains("defaultLabel!"),
        "force-js must strip the non-null `!`. output:\n{}",
        output
    );
    assert!(
        output.contains("default: 0"),
        "`0 satisfies number` should strip to `0`. output:\n{}",
        output
    );
    assert_valid_js(&output);
}

#[test]
fn with_defaults_force_js_strips_ts_from_variable_ref_defaults() {
    // This fixture isolates rewriting a non-object defaults expression. The
    // locally declared prop type makes the runtime projection authoritative;
    // unresolved macro semantics are covered by the fail-closed boundary tests.
    let alloc = Allocator::default();
    let content = "\ninterface Props { color?: string }\nconst baseDefaults = { color: 'primary' }\nconst props = withDefaults(defineProps<Props>(), baseDefaults as Props)\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "color",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "WD",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_mergeDefaults("),
        "variable-ref defaults should merge with authoritative runtime props. output:\n{}",
        output
    );
    assert!(
        !output.contains("as Props"),
        "force-js must strip the `as Props` cast from the defaults argument. output:\n{}",
        output
    );
    assert_valid_js(&output);
}

#[test]
fn multi_declarator_force_js_strips_ts_from_second_macro() {
    // `const p = defineProps(...), e = defineEmits(...)` — the emits section comes
    // from the second declarator, which the producer-keyed strip must still reach.
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps({ label: String }), emit = defineEmits({ change: (value: string) => true })\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Multi",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        !output.contains("(value: string)"),
        "force-js must strip the typed emits validator param. output:\n{}",
        output
    );
    assert_valid_js(&output);
}

#[test]
fn define_props_array_ts_wrapped_element_binds_clean_name() {
    // `['foo' as const, 'bar']` — the prop name is the string literal, not the
    // TS-wrapped expression. The binding must be `foo`, never `foo' as const`.
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps(['foo' as const, 'bar'])\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Arr",
            ..Default::default()
        },
    );

    assert!(
        result.bindings.contains_key("foo"),
        "TS-wrapped string element must bind the clean name `foo`. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        result.bindings.contains_key("bar"),
        "plain string element must bind `bar`. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        !result.bindings.keys().any(|k| k.contains("as const")),
        "no binding may carry the TS wrapper text. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );

    let output = ct.build_string();
    assert!(
        !output.contains("as const"),
        "force-js must strip the `as const` from the runtime props array. output:\n{}",
        output
    );
    assert_valid_js(&output);
}

#[test]
fn define_props_array_escaped_quote_binds_decoded_name() {
    // The element name is read from the AST string-literal value (decoded), not a
    // raw quote-stripped slice — so `'foo\'bar'` binds `foo'bar`.
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps(['foo\\'bar'])\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Esc",
            ..Default::default()
        },
    );

    assert!(
        result.bindings.contains_key("foo'bar"),
        "escaped-quote element should bind the decoded name `foo'bar`. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        !result.bindings.contains_key("foo\\'bar"),
        "the raw backslash-escaped slice must not be used as the binding name. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
}

#[test]
fn define_props_array_dynamic_element_names_nothing() {
    // A non-literal (dynamic) array element names no prop. `dynamicName` is an
    // undeclared identifier — it must not surface as a prop binding.
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps(['foo', dynamicName])\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Dyn",
            ..Default::default()
        },
    );

    assert!(
        result.bindings.contains_key("foo"),
        "string element must bind `foo`. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        !result.bindings.contains_key("dynamicName"),
        "a dynamic identifier element must not be treated as a prop name. bindings: {:?}",
        result.bindings.keys().collect::<Vec<_>>()
    );
}

// ── Optional Boolean props: official parity (no `default: undefined`) ──

/// In development, official plugin-vue emits
/// `{ type: Boolean, required: false }` for an optional Boolean prop with no
/// default. The runtime resolves an absent optional Boolean to `false`; an
/// explicit `default: undefined` diverges observably.
#[test]
fn optional_boolean_prop_emits_no_default_type_based() {
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps<{ disabled?: boolean, label?: string }>()\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [
            crate::test_helpers::runtime_prop(
                "disabled",
                true,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop(
                "label",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "BoolTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("disabled: { type: Boolean, required: false }"),
        "optional Boolean prop must emit the official dev shape, got:\n{output}"
    );
    assert!(
        !output.contains("default: undefined"),
        "no prop may carry `default: undefined` (official emits no default), got:\n{output}"
    );
    assert!(
        output.contains("label: { type: String, required: false }"),
        "optional non-Boolean prop keeps the official dev shape, got:\n{output}"
    );
}

/// Same official shape on the withDefaults path when the Boolean prop has
/// no declared default: no `default: undefined`, and declared defaults for
/// OTHER props still emit.
#[test]
fn optional_boolean_prop_emits_no_default_with_defaults_path() {
    let alloc = Allocator::default();
    let content = "\nconst props = withDefaults(defineProps<{ disabled?: boolean, color?: string }>(), { color: 'red' })\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop(
                "disabled",
                true,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "BoolTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("disabled: { type: Boolean, required: false }"),
        "optional Boolean without a declared default keeps the official dev shape, got:\n{output}"
    );
    assert!(
        !output.contains("default: undefined"),
        "withDefaults path must not invent `default: undefined`, got:\n{output}"
    );
    assert!(
        output.contains("default: 'red'"),
        "declared defaults must still emit, got:\n{output}"
    );
}

// ── `_mergeDefaults` emission must carry its runtime import ──

/// `withDefaults(defineProps<T>(), { ...SPREAD })` compiles to
/// `_mergeDefaults(...)`; the vue import list must carry
/// `_mergeDefaults` or the output throws ReferenceError at runtime.
#[test]
fn merge_defaults_spread_pushes_runtime_import() {
    let alloc = Allocator::default();
    let content =
        "\nconst props = withDefaults(defineProps<{ a?: string }>(), { ...SHARED_DEFAULTS })\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop(
            "a",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "MergeTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_mergeDefaults("),
        "spread defaults must compile through _mergeDefaults, got:\n{output}"
    );
    assert!(
        result.imports.contains(&"_mergeDefaults"),
        "emitting _mergeDefaults REQUIRES the runtime import, got imports: {:?}",
        result.imports
    );
}

/// Variable (non-literal) defaults wrap with `_mergeDefaults(base, VAR)`
/// and must also carry the import.
#[test]
fn merge_defaults_variable_pushes_runtime_import() {
    let alloc = Allocator::default();
    let content = "\nconst props = withDefaults(defineProps<{ a?: string }>(), DEFAULTS)\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop(
            "a",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "MergeTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("_mergeDefaults("),
        "variable defaults must compile through _mergeDefaults, got:\n{output}"
    );
    assert!(
        result.imports.contains(&"_mergeDefaults"),
        "emitting _mergeDefaults REQUIRES the runtime import, got imports: {:?}",
        result.imports
    );
}

/// Negative: a plain typed defineProps never pulls the mergeDefaults
/// import.
#[test]
fn plain_define_props_has_no_merge_defaults_import() {
    let alloc = Allocator::default();
    let content = "\nconst props = defineProps<{ a?: string }>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "PlainTest",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(!output.contains("_mergeDefaults("));
    assert!(
        !result.imports.contains(&"_mergeDefaults"),
        "plain defineProps must not import mergeDefaults, got imports: {:?}",
        result.imports
    );
}

/// A props-shaped type surface routed into `defineEmits` must NOT invent an
/// `emits: [...]` array from prop key names — emit names come exclusively
/// from resolved call signatures (fail-closed negative for the deleted
/// props→emits recovery fallback).
#[test]
fn define_emits_props_only_surface_does_not_invent_emits() {
    let alloc = Allocator::default();
    let content = "\nconst emit = defineEmits<{ title: string, count: number }>()\n";
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "EmitsTest",
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        !output.contains("emits: [\"title\""),
        "prop key names must never become emit names, got:\n{output}"
    );
    assert!(
        !output.contains("\"count\""),
        "props-only surface must not produce any emits entries, got:\n{output}"
    );
}

/// The legitimate named-tuple property form keeps producing emits.
#[test]
fn define_emits_named_tuple_property_form_still_produces_emits() {
    let alloc = Allocator::default();
    let content = "\nconst emit = defineEmits<{ change: [id: number], close: [] }>()\n";
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_emits_entry(
        0,
        0,
        ["change", "close"],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = CodeTransform::new(&full, &alloc);

    let _result = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "EmitsTest",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );

    let output = ct.build_string();
    assert!(
        output.contains("emits: [\"change\", \"close\"]"),
        "named-tuple emits must produce the emits array, got:\n{output}"
    );
}

/// force_js import reconstruction must preserve renamed named imports
/// (`import { FixedSizeList as ElFixedSizeList }`). Dropping the export name
/// makes rollup look for a non-existent `ElFixedSizeList` export (element-plus).
#[test]
fn force_js_preserves_named_import_alias() {
    use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
    use oxc_allocator::Allocator;

    let input = r#"
<script setup lang="ts">
import { FixedSizeList as ElFixedSizeList } from './virtual-list'
const List = ElFixedSizeList
</script>
<template><component :is="List" /></template>
"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("Transfer.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        input,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &allocator,
    );
    let code = result
        .script
        .as_ref()
        .map(|s| s.code.as_str())
        .unwrap_or("");
    assert!(
        code.contains("FixedSizeList as ElFixedSizeList"),
        "must preserve export alias FixedSizeList as ElFixedSizeList, got:\n{code}"
    );
    assert!(
        !code.contains("{ ElFixedSizeList }") && !code.contains("{ElFixedSizeList}"),
        "must not rewrite to bare ElFixedSizeList import, got:\n{code}"
    );
}

/// Options-API `<script lang="ts">` (no setup) is not rewritten by
/// process_script_only — force_js must still strip `import type` (element-plus
/// focus-trap.vue under rollup).
#[test]
fn force_js_options_api_strips_import_type() {
    use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
    use oxc_allocator::Allocator;

    let input = r#"
<script lang="ts">
import { defineComponent, ref } from 'vue'
import type { PropType } from 'vue'
export default defineComponent({
  props: { el: Object as PropType<HTMLElement> },
  setup() { return { x: ref(1) } },
})
</script>
<template><div /></template>
"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("Focus.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        input,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &allocator,
    );
    let code = result
        .script
        .as_ref()
        .map(|s| s.code.as_str())
        .unwrap_or("");
    assert!(
        !code.contains("import type"),
        "import type must be stripped under force_js options API, got:\n{code}"
    );
    assert!(
        code.contains("defineComponent") && code.contains("from 'vue'"),
        "value imports must remain, got:\n{code}"
    );
}

/// force_js must not leave ghost import fragments inside setup after
/// reconstructing value-only imports (element-plus radio.vue:
/// `import { type RadioProps, radioEmits, … }` → leftover
/// `radioEmits, … } from './radio'` mid-setup).
#[test]
fn force_js_mixed_type_value_import_has_no_ghost_body_fragment() {
    use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
    use oxc_allocator::Allocator;

    let input = r#"
<script setup lang="ts">
import { type RadioProps, radioEmits, radioPropsDefaults } from './radio'
// Runtime-syntax macros isolate mixed-import rewriting from typed macro handoff.
const props = defineProps(radioPropsDefaults)
const emit = defineEmits(radioEmits)
</script>
<template><div /></template>
"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("Radio.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        input,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &allocator,
    );
    let code = result
        .script
        .as_ref()
        .map(|s| s.code.as_str())
        .unwrap_or("");
    assert!(
        result.errors.is_empty(),
        "compile should succeed, errors={:?}\n{code}",
        result.errors
    );
    assert!(
        code.contains("import { radioEmits, radioPropsDefaults } from './radio'")
            || code.contains("import { radioEmits, radioPropsDefaults } from \"./radio\""),
        "value-only import must be kept, got:\n{code}"
    );
    // Ghost fragment from double strip: specifier list without `import {`
    // appearing mid-setup after the real import was already hoisted.
    assert!(
        !code.contains("\nradioEmits, radioPropsDefaults } from"),
        "must not leave ghost import remnant inside setup, got:\n{code}"
    );
    let from_count =
        code.matches("from './radio'").count() + code.matches("from \"./radio\"").count();
    assert_eq!(
        from_count, 1,
        "exactly one import from './radio', got {from_count} in:\n{code}"
    );
}

/// Prop keys that are not bare JS identifiers (e.g. `onUpdate:visible`) must
/// be quoted in the runtime props object. element-plus tooltip emits this
/// shape from `UseTooltipProps`; unquoted keys are a PARSE_ERROR under
/// rolldown/esbuild.
#[test]
fn runtime_props_quote_colon_keys_like_on_update_visible() {
    let alloc = Allocator::default();
    let content = r#"
interface Props {
  visible?: boolean
  'onUpdate:visible'?: (value: boolean) => void
}
const props = withDefaults(defineProps<Props>(), {
  visible: false,
})
"#;
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "visible",
                true,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "onUpdate:visible",
                true,
                [verter_macro_dto::RuntimeConstructor::Function],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);
    let _ = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );
    let output = ct.build_string();
    assert!(
        output.contains(r#""onUpdate:visible""#) || output.contains(r#"'onUpdate:visible'"#),
        "colon prop keys must be quoted in runtime props, got:\n{output}"
    );
    // Bare unquoted form is a syntax error in the generated module.
    assert!(
        !output.contains("onUpdate:visible: {"),
        "must not emit unquoted onUpdate:visible: key, got:\n{output}"
    );
}

/// `withDefaults(defineProps<T>(), { ...Defaults })` must pass the full
/// defaults expression to `_mergeDefaults` (reka-ui PopperContent).
#[test]
fn with_defaults_object_spread_uses_merge_defaults_full_expr() {
    let alloc = Allocator::default();
    let content = r#"
export const Defaults = { as: 'button', disabled: false }
interface Props { as?: string; disabled?: boolean; value?: string }
const props = withDefaults(defineProps<Props>(), {
  ...Defaults,
  value: 'on',
})
"#;
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "as",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "disabled",
                true,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "value",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let (setup, full) = make_script(content, "<script setup lang=\"ts\">", true);
    let mut ct = crate::code_transform::CodeTransform::new(&full, &alloc);
    let _ = gen_script(
        None,
        Some(&setup),
        &full,
        &mut ct,
        &alloc,
        &ScriptCodeGenOptions {
            component_name: "Test",
            macro_runtime: Some(&runtime),
            ..Default::default()
        },
    );
    let output = ct.build_string();
    assert!(
        output.contains("_mergeDefaults"),
        "spread defaults must use _mergeDefaults, got:\n{output}"
    );
    assert!(
        output.contains("...Defaults") || output.contains("Defaults"),
        "full defaults expression must retain Defaults, got:\n{output}"
    );
    assert!(
        output.contains("value:") && output.contains("'on'"),
        "inline default keys must remain in the typed props or defaults expr, got:\n{output}"
    );
}
