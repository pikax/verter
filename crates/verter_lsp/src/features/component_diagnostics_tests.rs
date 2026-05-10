use super::*;
use verter_semantic::analysis::template::{
    AnalyzedPropDefinition, PropValueConstness, TemplateAnalysisSnapshot, TemplateComponentUsage,
    TemplateComponentVModel, TemplatePropUsage,
};
use verter_semantic::analysis::types::AnalyzedMacro;
use verter_semantic::analysis::types::VueApiCallSite;

/// Helper to build a parent analysis with component usages.
fn make_parent_analysis(components: Vec<TemplateComponentUsage>) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                components,
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    }
}

/// Helper to build a child analysis with defined props.
fn make_child_with_props(prop_names: &[&str]) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                prop_definitions: prop_names
                    .iter()
                    .map(|name| AnalyzedPropDefinition {
                        name: name.to_string(),
                        type_annotation: Some("string".into()),
                        has_default: false,
                        is_required: true,
                        is_boolean: false,
                        used_in_template: false,
                        used_in_script: false,
                        span: verter_span::Span::new(0, 0),
                    })
                    .collect(),
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    }
}

fn make_prop(name: &str) -> TemplatePropUsage {
    TemplatePropUsage {
        name: name.to_string(),
        is_bound: true,
        expression: None,
        constness: PropValueConstness::Dynamic,
        referenced_bindings: vec![],
        from_spread: false,
        span: verter_span::Span::new(10, 20),
        name_span: verter_span::Span::new(0, 0),
        is_shorthand: false,
    }
}

fn make_component(
    name: &str,
    import_source: &str,
    props: Vec<TemplatePropUsage>,
) -> TemplateComponentUsage {
    TemplateComponentUsage {
        name: name.to_string(),
        import_source: Some(import_source.to_string()),
        is_dynamic: false,
        props,
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }
}

// -- find_unknown_props tests --

#[test]
fn unknown_prop_produces_diagnostic() {
    // Parent: <Child :foo="bar" />, Child: defineProps<{ msg: string }>()
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("foo")],
    )]);
    let child = make_child_with_props(&["msg"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    // Positive: foo is unknown
    assert_eq!(unknowns.len(), 1, "should find 1 unknown prop");
    assert_eq!(unknowns[0].prop_name, "foo");
    assert_eq!(unknowns[0].component_name, "Child");
    // Negative: msg should NOT be flagged
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "msg"),
        "msg should not be flagged"
    );
}

#[test]
fn known_prop_no_diagnostic() {
    // Parent: <Child :msg="val" />, Child: defineProps<{ msg: string }>()
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("msg")],
    )]);
    let child = make_child_with_props(&["msg"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(unknowns.is_empty(), "should have no unknown props");
}

#[test]
fn builtin_attrs_not_flagged() {
    // Parent: <Child class="foo" style="color:red" />
    // key and ref are already filtered out at extraction level
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![
            TemplatePropUsage {
                name: "class".to_string(),
                is_bound: false,
                expression: None,
                constness: PropValueConstness::Const,
                referenced_bindings: vec![],
                from_spread: false,
                span: verter_span::Span::new(10, 21),
                name_span: verter_span::Span::new(0, 0),
                is_shorthand: false,
            },
            TemplatePropUsage {
                name: "style".to_string(),
                is_bound: false,
                expression: None,
                constness: PropValueConstness::Const,
                referenced_bindings: vec![],
                from_spread: false,
                span: verter_span::Span::new(22, 40),
                name_span: verter_span::Span::new(0, 0),
                is_shorthand: false,
            },
        ],
    )]);
    let child = make_child_with_props(&[]); // No props defined

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    // Positive: empty — class and style are builtin
    assert!(
        unknowns.is_empty(),
        "builtin attrs (class, style) should not be flagged"
    );
    // Negative: no class or style in unknowns
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "class"),
        "class should not be unknown"
    );
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "style"),
        "style should not be unknown"
    );
}

#[test]
fn use_attrs_suppresses_all_unknown_props() {
    // Child has useAttrs() → no diagnostics regardless of props passed
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("unknown1"), make_prop("unknown2")],
    )]);
    let child = FileAnalysisSnapshot {
        vue_api_calls: (vec![VueApiCallSite {
            api: VueApiClassification::UseAttrs,
            span: verter_span::Span::new(30, 42),
            arg_value: None,
            has_type_params: false,
            is_async_callback: false,
            callback_params: vec![],
        }])
        .into(),
        ..Default::default()
    };

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "useAttrs() should suppress all unknown prop diagnostics"
    );
}

#[test]
fn inherit_attrs_false_suppresses_all() {
    // Child has defineOptions({ inheritAttrs: false }) → no diagnostics
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("unknown1")],
    )]);
    let child = FileAnalysisSnapshot {
        script_flags: AnalysisFlags::HAS_INHERIT_ATTRS_FALSE.bits(),
        ..Default::default()
    };

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "inheritAttrs: false should suppress all unknown prop diagnostics"
    );
}

#[test]
fn v_bind_spread_skips_component() {
    // <Child v-bind="obj" :extra="val" /> (has_spread=true) → no diagnostics
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "Child".to_string(),
        import_source: Some("./Child.vue".to_string()),
        is_dynamic: false,
        props: vec![make_prop("extra")],
        has_spread: true, // v-bind="obj" spread
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);
    let child = make_child_with_props(&[]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "v-bind spread should skip component prop checking"
    );
}

#[test]
fn dynamic_component_skipped() {
    // <component :is="comp" :foo="bar" /> → no diagnostics
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "component".to_string(),
        import_source: None,
        is_dynamic: true,
        props: vec![make_prop("foo")],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    let unknowns = find_unknown_props(&parent, &|_| None);
    assert!(
        unknowns.is_empty(),
        "dynamic components should be skipped entirely"
    );
}

#[test]
fn event_prop_not_flagged_when_emit_defined() {
    // Events (@save) are skipped during template extraction and don't
    // appear in component props. This test verifies no false positives
    // when only events are used and emit definitions exist.
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![], // @save is extracted as event handler, not prop
    )]);
    let child = make_child_with_props(&[]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "no false positives when component only has events"
    );
}

#[test]
fn kebab_case_prop_matches_camel_case_definition() {
    // Parent: <Child some-prop="val" />, Child: defineProps<{ someProp: string }>()
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("some-prop")],
    )]);
    let child = make_child_with_props(&["someProp"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    // Positive: should match despite case difference
    assert!(
        unknowns.is_empty(),
        "kebab-case 'some-prop' should match camelCase 'someProp'"
    );
}

#[test]
fn unresolvable_component_no_diagnostic() {
    // Component that can't be resolved → no diagnostics
    let parent = make_parent_analysis(vec![make_component(
        "Unknown",
        "./Unknown.vue",
        vec![make_prop("foo")],
    )]);

    let unknowns = find_unknown_props(&parent, &|_| None);
    assert!(
        unknowns.is_empty(),
        "unresolvable component should not produce diagnostics"
    );
}

#[test]
fn no_import_source_skipped() {
    // Component with no import source (global component) → skip
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "GlobalComp".to_string(),
        import_source: None,
        is_dynamic: false,
        props: vec![make_prop("unknown")],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    let unknowns = find_unknown_props(&parent, &|_| None);
    assert!(
        unknowns.is_empty(),
        "global components without import source should be skipped"
    );
}

#[test]
fn multiple_components_independent_checks() {
    // Two components: one with unknown props, one without
    let parent = make_parent_analysis(vec![
        make_component("ChildA", "./ChildA.vue", vec![make_prop("foo")]),
        make_component(
            "ChildB",
            "./ChildB.vue",
            vec![make_prop("bar"), make_prop("baz")],
        ),
    ]);
    let child_a = make_child_with_props(&["msg"]); // foo is unknown
    let child_b = make_child_with_props(&["bar"]); // baz is unknown

    let unknowns = find_unknown_props(&parent, &|source| match source {
        "./ChildA.vue" => Some(child_a.clone()),
        "./ChildB.vue" => Some(child_b.clone()),
        _ => None,
    });

    assert_eq!(unknowns.len(), 2, "should find 2 unknown props total");
    // Positive: foo on ChildA, baz on ChildB
    assert!(unknowns
        .iter()
        .any(|u| u.prop_name == "foo" && u.component_name == "ChildA"));
    assert!(unknowns
        .iter()
        .any(|u| u.prop_name == "baz" && u.component_name == "ChildB"));
    // Negative: bar should NOT be flagged (it's defined in ChildB)
    assert!(!unknowns.iter().any(|u| u.prop_name == "bar"));
}

// -- component_usage_diagnostics (LSP Diagnostic conversion) --

#[test]
fn diagnostics_have_correct_code_and_source() {
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![TemplatePropUsage {
            name: "unknown".to_string(),
            is_bound: true,
            expression: None,
            constness: PropValueConstness::Dynamic,
            referenced_bindings: vec![],
            from_spread: false,
            span: verter_span::Span::new(15, 30),
            name_span: verter_span::Span::new(0, 0),
            is_shorthand: false,
        }],
    )]);
    let child = make_child_with_props(&["msg"]); // needs at least one prop to trigger checking
    let source = "<template><Child :unknown=\"val\" /></template>";
    let line_index = LineIndex::new_utf16(source);

    let diags = component_usage_diagnostics(&parent, &line_index, &|_| Some(child.clone()));

    assert_eq!(diags.len(), 1);
    // Positive: correct code and source
    assert_eq!(
        diags[0].code,
        Some(NumberOrString::String("verter/unknown-prop".into()))
    );
    assert_eq!(diags[0].source.as_deref(), Some("verter"));
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::WARNING));
    assert!(
        diags[0].message.contains("unknown"),
        "message should contain prop name"
    );
    assert!(
        diags[0].message.contains("Child"),
        "message should contain component name"
    );
    // Negative: code is NOT a different value
    assert_ne!(
        diags[0].code,
        Some(NumberOrString::String("verter/unknown-model".into()))
    );
}

// -- kebab_to_camel tests --

#[test]
fn kebab_to_camel_simple() {
    assert_eq!(kebab_to_camel("foo"), "foo");
}

#[test]
fn kebab_to_camel_hyphenated() {
    assert_eq!(kebab_to_camel("some-prop"), "someProp");
}

#[test]
fn kebab_to_camel_multi_hyphen() {
    assert_eq!(kebab_to_camel("my-long-prop-name"), "myLongPropName");
}

// -- spread prop entry ignored --

#[test]
fn spread_prop_entry_ignored() {
    // Spread prop entries (from_spread: true) should never be flagged
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![TemplatePropUsage {
            name: String::new(),
            is_bound: true,
            expression: None,
            constness: PropValueConstness::Unknown,
            referenced_bindings: vec![],
            from_spread: true,
            span: verter_span::Span::new(0, 0),
            name_span: verter_span::Span::new(0, 0),
            is_shorthand: false,
        }],
    )]);
    let child = make_child_with_props(&[]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(unknowns.is_empty(), "spread prop entries should be ignored");
}

// -- v-model diagnostic tests --

fn make_component_with_vmodels(
    name: &str,
    import_source: &str,
    vmodels: Vec<TemplateComponentVModel>,
) -> TemplateComponentUsage {
    TemplateComponentUsage {
        name: name.to_string(),
        import_source: Some(import_source.to_string()),
        is_dynamic: false,
        props: vec![],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vmodels,
        span: verter_span::Span::new(0, 50),
    }
}

fn make_child_with_models(model_names: &[Option<&str>]) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        macros: model_names
            .iter()
            .map(|name| AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineModel,
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: name.map(|s| s.to_string()),
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 30),
            })
            .collect::<Vec<_>>()
            .into(),
        ..Default::default()
    }
}

#[test]
fn unknown_vmodel_produces_diagnostic() {
    // <Child v-model:title="val" />, Child has no defineModel('title')
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_models(&[]); // No defineModel at all

    let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
    // Positive: title is unknown
    assert_eq!(unknowns.len(), 1, "should find 1 unknown model");
    assert_eq!(unknowns[0].model_name, "title");
    assert_eq!(unknowns[0].component_name, "Child");
    // Negative: should not produce empty model name
    assert!(!unknowns[0].model_name.is_empty());
}

#[test]
fn known_vmodel_no_diagnostic() {
    // <Child v-model:title="val" />, Child: defineModel('title')
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_models(&[Some("title")]);

    let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
    assert!(unknowns.is_empty(), "should have no unknown models");
}

#[test]
fn default_vmodel_checks_model_value() {
    // <Child v-model="val" /> → binding_name is "modelValue"
    // Child: defineModel() → model_name is None (= "modelValue")
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "modelValue".to_string(),
            span: verter_span::Span::new(10, 25),
        }],
    )]);
    let child = make_child_with_models(&[None]); // defineModel() without name

    let unknowns = find_unknown_models(&parent, &|_| Some(child.clone()));
    // Positive: modelValue is known
    assert!(
        unknowns.is_empty(),
        "default v-model should match defineModel()"
    );

    // Negative: without defineModel, it should be unknown
    let child_empty = FileAnalysisSnapshot::default();
    let unknowns2 = find_unknown_models(&parent, &|_| Some(child_empty.clone()));
    assert_eq!(
        unknowns2.len(),
        1,
        "default v-model should be unknown without defineModel"
    );
}

#[test]
fn vmodel_diagnostic_has_correct_code() {
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(15, 35),
        }],
    )]);
    let child = make_child_with_models(&[]);
    let source = "<template><Child v-model:title=\"val\" /></template>";
    let line_index = LineIndex::new_utf16(source);

    let diags = component_usage_diagnostics(&parent, &line_index, &|_| Some(child.clone()));

    assert_eq!(diags.len(), 1);
    // Positive: correct diagnostic code
    assert_eq!(
        diags[0].code,
        Some(NumberOrString::String("verter/unknown-model".into()))
    );
    assert!(
        diags[0].message.contains("title"),
        "message should contain model name"
    );
    // Negative: code is NOT the prop code
    assert_ne!(
        diags[0].code,
        Some(NumberOrString::String("verter/unknown-prop".into()))
    );
}

#[test]
fn dynamic_component_vmodel_skipped() {
    // <component :is="comp" v-model:title="val" /> → no diagnostics
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "component".to_string(),
        import_source: None,
        is_dynamic: true,
        props: vec![],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
        span: verter_span::Span::new(0, 50),
    }]);

    let unknowns = find_unknown_models(&parent, &|_| None);
    assert!(
        unknowns.is_empty(),
        "dynamic components should be skipped for v-model checks"
    );
}

// ── data-*/aria-* fallthrough tests ─────────────────────────────

use verter_semantic::analysis::template::TemplateElement;

/// Helper to build a child analysis with props and a given number of root elements.
fn make_child_with_roots(prop_names: &[&str], root_count: usize) -> FileAnalysisSnapshot {
    let mut elements: Vec<TemplateElement> = Vec::new();
    for i in 0..root_count {
        elements.push(TemplateElement {
            tag: "div".to_string(),
            span: verter_span::Span::new((i * 100) as u32, ((i + 1) * 100) as u32),
            parent_index: None, // root element
            ..Default::default()
        });
    }
    FileAnalysisSnapshot {
        template: Some(
            (TemplateAnalysisSnapshot {
                prop_definitions: prop_names
                    .iter()
                    .map(|name| AnalyzedPropDefinition {
                        name: name.to_string(),
                        type_annotation: Some("string".into()),
                        has_default: false,
                        is_required: true,
                        is_boolean: false,
                        used_in_template: false,
                        used_in_script: false,
                        span: verter_span::Span::new(0, 0),
                    })
                    .collect(),
                elements,
                ..Default::default()
            })
            .into(),
        ),
        ..Default::default()
    }
}

#[test]
fn data_attr_not_flagged_on_non_fragment_component() {
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "SingleRoot".to_string(),
        import_source: Some("./SingleRoot.vue".to_string()),
        is_dynamic: false,
        has_spread: false,
        props: vec![make_prop("data-test")],
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    // Single root element → non-fragment → data-* should fall through
    let child = make_child_with_roots(&["msg"], 1);
    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    assert!(
        unknowns.is_empty(),
        "data-test should NOT be flagged on non-fragment component (fallthrough)"
    );
}

#[test]
fn data_attr_flagged_on_fragment_component() {
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "FragmentComp".to_string(),
        import_source: Some("./FragmentComp.vue".to_string()),
        is_dynamic: false,
        has_spread: false,
        props: vec![make_prop("msg"), make_prop("data-test")],
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    // Two root elements → fragment → data-* cannot fall through
    let child = make_child_with_roots(&["msg"], 2);
    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    // Positive: data-test IS flagged on fragment
    assert_eq!(unknowns.len(), 1, "only data-test should be flagged");
    assert_eq!(unknowns[0].prop_name, "data-test");
    // Negative: msg is a declared prop and should NOT be flagged
    assert!(
        unknowns.iter().all(|u| u.prop_name != "msg"),
        "declared prop 'msg' must not be flagged"
    );
}

#[test]
fn aria_attr_not_flagged_on_non_fragment_component() {
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "SingleRoot".to_string(),
        import_source: Some("./SingleRoot.vue".to_string()),
        is_dynamic: false,
        has_spread: false,
        props: vec![make_prop("aria-label")],
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    let child = make_child_with_roots(&["msg"], 1);
    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    assert!(
        unknowns.is_empty(),
        "aria-label should NOT be flagged on non-fragment component"
    );
}

// ── Macro fallback path tests ───────────────────────────────────
//
// In production, `template.prop_definitions` is always empty.
// Props come from `macros[DefineProps].prop_fields` instead.

/// Helper: child with DefineProps macro prop_fields, NO template.prop_definitions.
fn make_child_with_macro_props(prop_names: &[&str]) -> FileAnalysisSnapshot {
    use verter_semantic::analysis::types::{AnalyzedPropField, TypeResolutionSource};

    FileAnalysisSnapshot {
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: prop_names
                .iter()
                .map(|name| AnalyzedPropField {
                    name: name.to_string(),
                    span: verter_span::Span::new(0, 0),
                    type_annotation: None,
                    is_optional: false,
                    description: None,
                    tags: vec![],
                    resolution_source: TypeResolutionSource::Rust,
                    resolution_error: None,
                    type_expr: None,
                    type_expr_scope: None,
                })
                .collect(),
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 30),
        }]
        .into(),
        // template.prop_definitions is empty — matches production
        ..Default::default()
    }
}

#[test]
fn macro_fallback_known_prop_not_flagged() {
    // Child has DefineProps macro with "msg" in prop_fields
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("msg")],
    )]);
    let child = make_child_with_macro_props(&["msg"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    // Positive: msg is known via macro fallback
    assert!(
        unknowns.is_empty(),
        "macro fallback should recognize 'msg' as defined prop"
    );
}

#[test]
fn macro_fallback_unknown_prop_flagged() {
    // Child has DefineProps macro with "msg" only
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("msg"), make_prop("unknown")],
    )]);
    let child = make_child_with_macro_props(&["msg"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    // Positive: unknown should be flagged
    assert_eq!(unknowns.len(), 1, "should find 1 unknown prop");
    assert_eq!(unknowns[0].prop_name, "unknown");
    // Negative: msg should NOT be flagged
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "msg"),
        "msg should not be flagged via macro fallback"
    );
}

#[test]
fn macro_fallback_with_defaults_pattern() {
    // withDefaults wraps defineProps — the inner DefineProps macro has the real props
    use verter_semantic::analysis::types::{AnalyzedPropField, TypeResolutionSource};

    let child = FileAnalysisSnapshot {
        macros: vec![
            AnalyzedMacro {
                kind: AnalyzedMacroKind::WithDefaults,
                is_type_based: false,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(0, 50),
            },
            AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![
                    AnalyzedPropField {
                        name: "msg".to_string(),
                        span: verter_span::Span::new(0, 0),
                        type_annotation: None,
                        is_optional: false,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                        type_expr: None,
                        type_expr_scope: None,
                    },
                    AnalyzedPropField {
                        name: "count".to_string(),
                        span: verter_span::Span::new(0, 0),
                        type_annotation: None,
                        is_optional: false,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                        type_expr: None,
                        type_expr_scope: None,
                    },
                ],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                span: verter_span::Span::new(10, 40),
            },
        ]
        .into(),
        ..Default::default()
    };

    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("msg"), make_prop("count")],
    )]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "withDefaults + defineProps inner props should be recognized"
    );
}

#[test]
fn macro_fallback_kebab_camel_match() {
    // Child has camelCase "lazyRender", parent uses kebab-case ":lazy-render"
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("lazy-render")],
    )]);
    let child = make_child_with_macro_props(&["lazyRender"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));
    assert!(
        unknowns.is_empty(),
        "kebab 'lazy-render' should match camel 'lazyRender' via macro fallback"
    );
}

#[test]
fn empty_props_no_false_positives() {
    // Child resolves but has NO macros and NO prop_definitions → empty defined_props
    // Should NOT flag any props (can't validate without definitions)
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("foo"), make_prop("bar")],
    )]);
    let child = FileAnalysisSnapshot::default(); // No macros, no template

    let unknowns = find_unknown_props(&parent, &|_| Some(child.clone()));

    // With the guard: empty defined_props → skip checking → no diagnostics
    assert!(
        unknowns.is_empty(),
        "empty prop definitions should NOT produce false positive diagnostics"
    );
}

// -- missing required slot tests --

fn make_component_with_slots(
    name: &str,
    import_source: &str,
    slots_used: Vec<String>,
) -> TemplateComponentUsage {
    TemplateComponentUsage {
        name: name.to_string(),
        import_source: Some(import_source.to_string()),
        is_dynamic: false,
        props: vec![],
        has_spread: false,
        slots_used,
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::new(0, 50),
    }
}

fn make_child_with_required_slots(slot_names: &[(&str, bool)]) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            is_type_based: true,
            type_references: vec![],
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: vec![],
            emit_fields: vec![],
            slot_fields: slot_names
                .iter()
                .map(
                    |(name, required)| verter_semantic::analysis::AnalyzedSlotField {
                        name: name.to_string(),
                        is_required: *required,
                        span: verter_span::Span::new(0, 10),
                        bindings: vec![],
                        description: None,
                        tags: vec![],
                        return_type: None,
                        return_expr: None,
                        return_expr_scope: None,
                    },
                )
                .collect(),
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            span: verter_span::Span::new(0, 30),
        }]
        .into(),
        ..Default::default()
    }
}

#[test]
fn missing_required_slot_reports() {
    // Parent: <Child> (no slots), Child: defineSlots<{ default(p: {}): any }>
    let parent = make_parent_analysis(vec![make_component_with_slots(
        "Child",
        "./Child.vue",
        vec![], // no slots provided
    )]);
    let child = make_child_with_required_slots(&[("default", true)]);

    let missing = find_missing_required_slots(&parent, &|_| Some(child.clone()));
    assert_eq!(missing.len(), 1, "should find 1 missing required slot");
    assert_eq!(missing[0].slot_name, "default");
    assert_eq!(missing[0].component_name, "Child");
}

#[test]
fn provided_required_slot_no_report() {
    // Parent: <Child> <template #default>...</template> </Child>
    let parent = make_parent_analysis(vec![make_component_with_slots(
        "Child",
        "./Child.vue",
        vec!["default".to_string()],
    )]);
    let child = make_child_with_required_slots(&[("default", true)]);

    let missing = find_missing_required_slots(&parent, &|_| Some(child.clone()));
    assert!(
        missing.is_empty(),
        "provided required slot should not report: {:?}",
        missing.iter().map(|m| &m.slot_name).collect::<Vec<_>>()
    );
}

#[test]
fn optional_slot_not_provided_no_report() {
    // Parent: <Child> (no slots), Child: defineSlots<{ header?(p: {}): any }>
    let parent = make_parent_analysis(vec![make_component_with_slots(
        "Child",
        "./Child.vue",
        vec![],
    )]);
    let child = make_child_with_required_slots(&[("header", false)]);

    let missing = find_missing_required_slots(&parent, &|_| Some(child.clone()));
    assert!(
        missing.is_empty(),
        "optional slot should not report when not provided"
    );
}

#[test]
fn mixed_required_optional_slots() {
    // Child: default (required), header (optional), footer (required)
    // Parent provides default and header but NOT footer
    let parent = make_parent_analysis(vec![make_component_with_slots(
        "Child",
        "./Child.vue",
        vec!["default".to_string(), "header".to_string()],
    )]);
    let child =
        make_child_with_required_slots(&[("default", true), ("header", false), ("footer", true)]);

    let missing = find_missing_required_slots(&parent, &|_| Some(child.clone()));
    assert_eq!(missing.len(), 1, "should find 1 missing required slot");
    assert_eq!(missing[0].slot_name, "footer");
}

#[test]
fn no_define_slots_no_report() {
    let parent = make_parent_analysis(vec![make_component_with_slots(
        "Child",
        "./Child.vue",
        vec![],
    )]);
    let child = FileAnalysisSnapshot::default();

    let missing = find_missing_required_slots(&parent, &|_| Some(child.clone()));
    assert!(
        missing.is_empty(),
        "no defineSlots should not report missing required slots"
    );
}
