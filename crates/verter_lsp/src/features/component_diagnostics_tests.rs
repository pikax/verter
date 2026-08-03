use super::*;
use verter_semantic::analysis::template::{
    AnalyzedPropDefinition, PropValueConstness, TemplateAnalysisSnapshot, TemplateComponentUsage,
    TemplateComponentVModel, TemplatePropUsage,
};
use verter_semantic::analysis::types::AnalyzedMacro;
use verter_semantic::analysis::types::VueApiCallSite;

/// Wrap a hand-built analysis snapshot as a child that inherits NOTHING.
///
/// Correct for every test below that is not about attribute fallthrough: with
/// an empty inherited surface the lint's answer depends only on the declared
/// props, which is what those tests characterize. The fallthrough behaviour
/// itself is NOT tested through this helper — a hand-written surface would
/// just be the assertion restated as a fixture. It is driven from a real
/// `VerterHost`, through the production resolution closure, at the bottom of
/// this file.
fn no_fallthrough(analysis: FileAnalysisSnapshot) -> ResolvedChildComponent {
    ResolvedChildComponent {
        analysis,
        inherited_attrs: HashSet::new(),
    }
}

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
        bindings: vec![],
        events: vec![],
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));

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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
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
            make_prop("notAnAttr"),
        ],
    )]);
    // The child MUST declare a prop: `check_component_props` returns early on
    // `defined_props.is_empty()`, so a props-less child never reaches the
    // `BUILTIN_ATTRS` check at all and this assertion would hold no matter what
    // that check did.
    let child = make_child_with_props(&["label"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    // Negative: no class or style in unknowns
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "class"),
        "class should not be unknown; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );
    assert!(
        !unknowns.iter().any(|u| u.prop_name == "style"),
        "style should not be unknown; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );
    // CONTROL: the same usage carries an ordinary undeclared attribute, which
    // this child (it inherits nothing) DOES reject. Without it, a
    // `check_component_props` that silently stopped reporting anything would
    // still satisfy the two assertions above.
    assert_eq!(
        unknowns
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["notAnAttr"],
        "the lint must still be REPORTING on this usage — otherwise the \
         class/style assertions above prove nothing"
    );
}

/// `useAttrs()` is the ONE remaining suppressor: a child that reads `$attrs`
/// programmatically may give meaning to any attribute, so this lint cannot
/// prove one wrong. Fails OPEN about the component's own code — which is a
/// different question from inheritance.
///
/// The child must declare a prop: `check_component_props` returns early when
/// `defined_props` is empty, so a props-less child is never checked at all and
/// this assertion would hold no matter what the suppressor did.
#[test]
fn use_attrs_suppresses_all_unknown_props() {
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("label"), make_prop("unknown1")],
    )]);
    let mut child = make_child_with_props(&["label"]);
    child.vue_api_calls = (vec![VueApiCallSite {
        api: VueApiClassification::UseAttrs,
        span: verter_span::Span::new(30, 42),
        arg_value: None,
        has_type_params: false,
        is_async_callback: false,
        callback_params: vec![],
    }])
    .into();

    // Control: WITHOUT the useAttrs call the same usage IS reported, so the
    // emptiness below is the suppressor and not the early return.
    let control = make_child_with_props(&["label"]);
    assert_eq!(
        find_unknown_props(&parent, &|_| Some(no_fallthrough(control.clone())))
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["unknown1"],
        "control: without useAttrs() the undeclared prop must be reported"
    );

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "useAttrs() should suppress all unknown prop diagnostics, got {:?}",
        unknowns
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>()
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
        bindings: vec![],
        events: vec![],
        span: verter_span::Span::new(0, 50),
    }]);
    // The child declares a prop so `check_component_props` does NOT take its
    // `defined_props.is_empty()` early return: with a props-less child the
    // result is empty whether or not the spread guard exists, and this
    // assertion would characterize nothing. `extra` is undeclared and would be
    // reported if the spread guard were removed.
    let child = make_child_with_props(&["label"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "v-bind spread should skip component prop checking; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );

    // CONTROL: the identical usage WITHOUT the spread reports `extra`. This is
    // what proves the emptiness above came from the spread guard.
    let without_spread = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("extra")],
    )]);
    let control = find_unknown_props(&without_spread, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        control
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["extra"],
        "control: the same undeclared prop IS reported when there is no spread"
    );
}

#[test]
fn dynamic_component_skipped() {
    // <component :is="comp" :foo="bar" /> → no diagnostics.
    //
    // The usage carries a REAL `import_source` and the resolver below returns a
    // child that declares a prop, so every later guard
    // (`import_source.as_deref()?`, `defined_props.is_empty()`) is satisfied and
    // `foo` WOULD be reported. The `is_dynamic` guard is the only thing
    // stopping it — which is what this test is about.
    let parent = make_parent_analysis(vec![TemplateComponentUsage {
        name: "component".to_string(),
        import_source: Some("./Child.vue".to_string()),
        is_dynamic: true,
        props: vec![make_prop("foo")],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        bindings: vec![],
        events: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    let child = make_child_with_props(&["label"]);
    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "dynamic components should be skipped entirely; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );

    // CONTROL: the identical usage that is NOT dynamic reports `foo`. This is
    // what proves the emptiness above came from the `is_dynamic` guard rather
    // than from an unresolvable child or an empty declared-prop set.
    let static_usage = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("foo")],
    )]);
    let control = find_unknown_props(&static_usage, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        control
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["foo"],
        "control: the same undeclared prop IS reported on a static usage"
    );
}

#[test]
fn event_prop_not_flagged_when_emit_defined() {
    // Events (@save) are skipped during template extraction and never appear
    // in `TemplateComponentUsage::props`, so an events-only usage must produce
    // nothing. The child DECLARES a prop and the usage carries one real
    // (declared) prop entry alongside, so the usage is genuinely checked —
    // without both, `check_component_props` returns early and this assertion
    // would hold regardless of how events are classified.
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        // `@save` is extracted as an event handler, not a prop, so it is
        // absent here by construction; `label` is what makes the loop run.
        vec![make_prop("label")],
    )]);
    let child = make_child_with_props(&["label"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "no false positives when component only has events; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );

    // CONTROL: an undeclared entry on the SAME shape IS reported, so the
    // emptiness above is a statement about this usage rather than about the
    // lint having stopped working.
    let with_unknown = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![make_prop("label"), make_prop("onSaveTypo")],
    )]);
    let control = find_unknown_props(&with_unknown, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        control
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["onSaveTypo"],
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
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
        bindings: vec![],
        events: vec![],
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
        "./ChildA.vue" => Some(no_fallthrough(child_a.clone())),
        "./ChildB.vue" => Some(no_fallthrough(child_b.clone())),
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

    let diags = component_usage_diagnostics(&parent, &line_index, &|_| {
        Some(no_fallthrough(child.clone()))
    });

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
    // Spread prop entries (`from_spread: true`) are never flagged.
    //
    // The entry deliberately carries a NAME the child does not declare, so the
    // `from_spread` guard in `is_unknown_prop` is the only thing suppressing
    // it — a nameless entry on a props-less child is suppressed by the
    // `defined_props.is_empty()` early return long before that guard runs, and
    // characterizes nothing.
    let spread_entry = TemplatePropUsage {
        name: "notDeclared".to_string(),
        is_bound: true,
        expression: None,
        constness: PropValueConstness::Unknown,
        referenced_bindings: vec![],
        from_spread: true,
        span: verter_span::Span::new(0, 0),
        name_span: verter_span::Span::new(0, 0),
        is_shorthand: false,
    };
    let parent = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![spread_entry.clone()],
    )]);
    let child = make_child_with_props(&["label"]);

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "spread prop entries should be ignored; got {:?}",
        unknowns.iter().map(|u| &u.prop_name).collect::<Vec<_>>()
    );

    // CONTROL: the SAME name, not from a spread, IS reported.
    let authored = make_parent_analysis(vec![make_component(
        "Child",
        "./Child.vue",
        vec![TemplatePropUsage {
            from_spread: false,
            ..spread_entry
        }],
    )]);
    let control = find_unknown_props(&authored, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        control
            .iter()
            .map(|u| u.prop_name.as_str())
            .collect::<Vec<_>>(),
        vec!["notDeclared"],
        "control: the same undeclared name IS reported when it is authored"
    );
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
        bindings: vec![],
        events: vec![],
        span: verter_span::Span::new(0, 50),
    }
}

fn make_child_with_models(model_names: &[Option<&str>]) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        macros: model_names
            .iter()
            .map(|name| AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineModel,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
                parsed_type_argument_scope: None,
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

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    // Positive: modelValue is known
    assert!(
        unknowns.is_empty(),
        "default v-model should match defineModel()"
    );

    // Negative: without defineModel, it should be unknown
    let child_empty = FileAnalysisSnapshot::default();
    let unknowns2 = find_unknown_models(&parent, &|_| Some(no_fallthrough(child_empty.clone())));
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

    let diags = component_usage_diagnostics(&parent, &line_index, &|_| {
        Some(no_fallthrough(child.clone()))
    });

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
        bindings: vec![],
        events: vec![],
        span: verter_span::Span::new(0, 50),
    }]);

    let unknowns = find_unknown_models(&parent, &|_| None);
    assert!(
        unknowns.is_empty(),
        "dynamic components should be skipped for v-model checks"
    );
}

/// Helper to build a child that declares a v-model the classic way: a
/// `defineProps` field plus the matching `defineEmits` `update:` event.
fn make_child_with_prop_emit_pair(props: &[&str], emits: &[&str]) -> FileAnalysisSnapshot {
    fn macro_shell(kind: AnalyzedMacroKind) -> AnalyzedMacro {
        AnalyzedMacro {
            kind,
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
            is_type_based: true,
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
            parsed_type_argument_scope: None,
            span: verter_span::Span::new(0, 30),
        }
    }

    let mut define_props = macro_shell(AnalyzedMacroKind::DefineProps);
    define_props.prop_fields = props
        .iter()
        .map(|name| verter_semantic::analysis::AnalyzedPropField {
            name: (*name).to_string(),
            span: verter_span::Span::new(0, 0),
            type_annotation: Some("string".into()),
            is_optional: false,
            description: None,
            tags: vec![],
            resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
            resolution_error: None,
            payload: None,
            type_expr_scope: None,
            declared_in_macro_type_arg: true,
        })
        .collect();

    let mut define_emits = macro_shell(AnalyzedMacroKind::DefineEmits);
    define_emits.emit_fields = emits
        .iter()
        .map(|name| verter_semantic::analysis::AnalyzedEmitField {
            producer_identity: Default::default(),
            name: (*name).to_string(),
            span: verter_span::Span::new(0, 0),
            payload_type: None,
            description: None,
            tags: vec![],
            payload: None,
            payload_expr_scope: None,
        })
        .collect();

    FileAnalysisSnapshot {
        macros: vec![define_props, define_emits].into(),
        ..Default::default()
    }
}

#[test]
fn default_vmodel_declared_as_prop_and_emit_pair_no_diagnostic() {
    // <Child v-model="val" />; Child:
    //   defineProps<{ modelValue: string }>()
    //   defineEmits<{ 'update:modelValue': [v: string] }>()
    // `defineModel` is sugar over exactly this pair, so the pair must be
    // recognised as a declared model.
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "modelValue".to_string(),
            span: verter_span::Span::new(10, 25),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["modelValue"], &["update:modelValue"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "prop + update: emit pair must count as a declared v-model, got {:?}",
        unknowns.iter().map(|u| &u.model_name).collect::<Vec<_>>()
    );
}

#[test]
fn named_vmodel_declared_as_prop_and_emit_pair_no_diagnostic() {
    // <Child v-model:title="val" />; Child:
    //   defineProps<{ title: string }>()
    //   defineEmits<{ 'update:title': [v: string] }>()
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["title"], &["update:title"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "named prop + update: emit pair must count as a declared v-model, got {:?}",
        unknowns.iter().map(|u| &u.model_name).collect::<Vec<_>>()
    );
}

#[test]
fn kebab_named_vmodel_matches_camel_case_pair_no_diagnostic() {
    // <Child v-model:my-value="val" />; Child:
    //   defineProps<{ myValue: string }>()
    //   defineEmits<{ 'update:myValue': [v: string] }>()
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "my-value".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["myValue"], &["update:myValue"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        unknowns.is_empty(),
        "kebab v-model arg must match the camelCase prop + emit pair, got {:?}",
        unknowns.iter().map(|u| &u.model_name).collect::<Vec<_>>()
    );
}

#[test]
fn vmodel_with_neither_prop_nor_emit_still_warns() {
    // CONTROL — the rule must stay discriminating. Child declares a full
    // `other` model pair but nothing for `bar`, so `v-model:bar` still warns.
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "bar".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["other"], &["update:other"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(unknowns.len(), 1, "unknown v-model must still be reported");
    assert_eq!(unknowns[0].model_name, "bar");
    // Negative: the declared `other` model is not flagged.
    assert!(!unknowns.iter().any(|u| u.model_name == "other"));
}

#[test]
fn vmodel_with_prop_but_no_update_emit_still_warns() {
    // CONTROL — half a model is not a model: prop `title` with no
    // `update:title` emit is not a writable v-model target.
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["title"], &["change"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        unknowns.len(),
        1,
        "prop without the matching update: emit must still warn"
    );
    assert_eq!(unknowns[0].model_name, "title");
}

#[test]
fn vmodel_with_update_emit_but_no_prop_still_warns() {
    // CONTROL — the other half: `update:title` emit with no `title` prop.
    let parent = make_parent_analysis(vec![make_component_with_vmodels(
        "Child",
        "./Child.vue",
        vec![TemplateComponentVModel {
            binding_name: "title".to_string(),
            span: verter_span::Span::new(10, 30),
        }],
    )]);
    let child = make_child_with_prop_emit_pair(&["other"], &["update:title"]);

    let unknowns = find_unknown_models(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert_eq!(
        unknowns.len(),
        1,
        "update: emit without the matching prop must still warn"
    );
    assert_eq!(unknowns[0].model_name, "title");
}

// ── Attribute fallthrough ────────────────────────────────────────
//
// https://github.com/pikax/verter/issues/97
//
// These are driven from a REAL `VerterHost` through the production
// resolution closure (`server_utils::resolve_child_component_for`), NOT from a
// hand-written inherited surface. The mechanism under test IS "what does the
// inheritance resolver say", so a fixture that supplied that answer would only
// restate the assertion.
//
// The harness mirrors the LSP's real two-host split, because the two halves of
// this lint come from DIFFERENT hosts in production
// (`server_utils::publish_diagnostics` -> `Documents::get_analysis`):
//
// * the PARENT's analysis — which carries the template component usages the
//   lint iterates — comes from the background SEMANTIC host
//   (`AnalysisScope::LSP`, `documents/analysis.rs`);
// * the CHILD resolution runs `server_utils::resolve_child_component_for`
//   against the PROJECTION host (`AnalysisScope::BUILD`, `main.rs`), and that
//   is the call that reaches the inheritance resolver.
//
// Running the child half on BUILD is deliberate and load-bearing. `BUILD`
// carries no template flag, so the resolver obtains root reachability through
// its own request-scoped demand; a fix that only worked under a full analysis
// scope would be invisible in the editor.

use std::sync::Arc;
use verter_semantic::analysis::AnalysisScope;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};

fn load_host(scope: AnalysisScope, files: &[(&str, &str)]) -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_scope: Some(scope),
        ..HostConfig::default()
    });
    for (id, source) in files {
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: None,
                input_id: (*id).to_string(),
                source: Arc::from(*source),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("upsert");
    }
    host
}

/// The LSP's two hosts, loaded with the same `files`: the background semantic
/// host the parent analysis comes from, and the projection host the child
/// resolution runs against.
struct LintHosts {
    semantic: VerterHost,
    projection: VerterHost,
}

fn lint_host(files: &[(&str, &str)]) -> LintHosts {
    LintHosts {
        semantic: load_host(AnalysisScope::LSP, files),
        projection: load_host(AnalysisScope::BUILD, files),
    }
}

/// Run the lint exactly as the LSP does: the parent's own analysis from the
/// semantic host, children resolved through the production closure against the
/// projection host.
fn lint_unknown_props(hosts: &LintHosts, parent: &str) -> Vec<UnknownPropInfo> {
    let analysis = hosts
        .semantic
        .get_analysis(parent)
        .unwrap_or_else(|| panic!("analysis for {parent}"));
    assert!(
        analysis
            .template
            .as_ref()
            .is_some_and(|t| !t.components.is_empty()),
        "precondition: the parent's template component usages must be analysed \
         under this scope, otherwise the lint sees nothing and every assertion \
         below passes vacuously"
    );
    // PRECONDITION on the other half: the projection host must carry NO template
    // analysis of its own, so a green result below proves the resolver obtained
    // root reachability through its request-scoped demand rather than through a
    // template flag someone put back on `BUILD`.
    assert!(
        hosts
            .projection
            .get_analysis(parent)
            .is_some_and(|a| a.template.is_none()),
        "precondition: the projection host's scope must stay template-free, or \
         the child half stops testing the on-demand path"
    );
    find_unknown_props(&analysis, &|import_source| {
        crate::server::server_utils::resolve_child_component_for(
            &hosts.projection,
            parent,
            import_source,
        )
    })
}

fn flagged(unknowns: &[UnknownPropInfo]) -> Vec<&str> {
    unknowns.iter().map(|u| u.prop_name.as_str()).collect()
}

const CHILD_ONE_PROP_DIV_ROOT: &str = "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><div>{{ label }}</div></template>\n";

/// REGRESSION for issue #97. `title` is a global HTML attribute; the child
/// declares only `label` and renders a single native `<div>` root, so Vue
/// forwards `title` through `$attrs` onto that div and it reaches the DOM.
///
/// FAILS against `main`: `is_fallthrough_attr` accepted only
/// `class`/`style`/`data-*`/`aria-*` by string prefix, so every other global
/// HTML attribute was reported as an unknown prop.
#[test]
fn undeclared_global_html_attr_falling_through_to_native_root_is_not_unknown() {
    let host = lint_host(&[
        ("/src/Child.vue", CHILD_ONE_PROP_DIV_ROOT),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" title=\"hi\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert!(
        unknowns.is_empty(),
        "`title` is inherited by the child's single native <div> root and reaches \
         the DOM, so it must not be reported; got {:?}",
        flagged(&unknowns)
    );
}

/// NEGATIVE half. A name no element accepts must STILL be reported after the
/// surface is widened — the widening is the resolved member set, not a blanket
/// allowance.
#[test]
fn attr_no_native_element_accepts_is_still_unknown_on_a_native_root() {
    let host = lint_host(&[
        ("/src/Child.vue", CHILD_ONE_PROP_DIV_ROOT),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" notARealThing=\"x\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert_eq!(
        flagged(&unknowns),
        vec!["notARealThing"],
        "`notARealThing` is neither declared nor an attribute the root <div> \
         accepts"
    );
}

/// NEGATIVE half. A fragment has no single root to inherit into, so Vue does
/// not forward and the attribute is genuinely wrong.
#[test]
fn global_html_attr_on_a_fragment_child_is_still_unknown() {
    let host = lint_host(&[
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><div/><span/></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" title=\"hi\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert_eq!(
        flagged(&unknowns),
        vec!["title"],
        "a multi-root child inherits nothing; Vue warns at runtime"
    );
}

/// NEGATIVE half, and the INVERSION this fix had to undo.
///
/// `inheritAttrs: false` used to suppress EVERY unknown-prop diagnostic
/// (`child_suppresses_prop_checks` returned `true` on the flag), which is the
/// exact inverse of the Fallthrough / Root Inheritance rule: no inherited
/// surface means an undeclared attribute reaches NOTHING, so it is more wrong
/// there, not less. FAILS against `main`, which reported nothing here.
#[test]
fn inherit_attrs_false_reports_undeclared_attrs_instead_of_suppressing_them() {
    let host = lint_host(&[
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\ndefineOptions({ inheritAttrs: false })\ndefineProps<{ label: string }>()\n</script>\n<template><div>{{ label }}</div></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" title=\"hi\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert_eq!(
        flagged(&unknowns),
        vec!["title"],
        "`inheritAttrs: false` means NO inherited surface — `title` reaches \
         nothing and must be reported, not suppressed"
    );
}

/// `class` / `style` reach every component through `AllowedComponentProps`.
/// Asserted on a child that inherits NOTHING, which is what proves they are
/// not being supplied by the fallthrough widening.
#[test]
fn class_and_style_are_accepted_even_when_nothing_is_inherited() {
    let host = lint_host(&[
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\ndefineOptions({ inheritAttrs: false })\ndefineProps<{ label: string }>()\n</script>\n<template><div>{{ label }}</div></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" class=\"x\" style=\"color:red\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert!(
        unknowns.is_empty(),
        "class/style are merged through AllowedComponentProps and stay accepted \
         under `inheritAttrs: false`; got {:?}",
        flagged(&unknowns)
    );
}

/// An attribute the child's root element does not accept, even though some
/// OTHER element would — the widening is keyed on the resolved root element,
/// not a generic HTML surface.
#[test]
fn anchor_only_attr_is_unknown_on_a_div_root_child() {
    let host = lint_host(&[
        ("/src/Child.vue", CHILD_ONE_PROP_DIV_ROOT),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" href=\"/x\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert_eq!(
        flagged(&unknowns),
        vec!["href"],
        "`href` is an <a> attribute; this child's root is a <div>"
    );
}

/// The same attribute on a child whose root IS an `<a>` is accepted — the
/// discriminating pair for the test above.
#[test]
fn anchor_only_attr_is_accepted_on_an_anchor_root_child() {
    let host = lint_host(&[
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><a>{{ label }}</a></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" href=\"/x\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert!(
        unknowns.is_empty(),
        "`href` reaches the child's <a> root; got {:?}",
        flagged(&unknowns)
    );
}

/// `aria-*` is on the resolved surface (Vue types AriaAttributes members
/// explicitly), so it is accepted for a structural reason rather than by a
/// `"aria-"` prefix check.
#[test]
fn aria_attr_is_accepted_on_a_native_root_child() {
    let host = lint_host(&[
        ("/src/Child.vue", CHILD_ONE_PROP_DIV_ROOT),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" aria-label=\"x\" /></template>\n",
        ),
    ]);

    assert!(
        lint_unknown_props(&host, "/src/App.vue").is_empty(),
        "aria-label is a member of the root <div>'s attribute surface"
    );
}

/// `data-*` has no member in any element's props type, so it survives on the
/// separate non-identifier rule — and ONLY while something inherits.
#[test]
fn data_attr_is_accepted_on_a_native_root_child() {
    let host = lint_host(&[
        ("/src/Child.vue", CHILD_ONE_PROP_DIV_ROOT),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" data-test=\"x\" /></template>\n",
        ),
    ]);

    assert!(
        lint_unknown_props(&host, "/src/App.vue").is_empty(),
        "data-* reaches the root element and the generated carrier cannot check \
         it either (TypeScript skips excess-property checks on non-identifier \
         JSX attribute names)"
    );
}

/// …and is still reported on a fragment child, where it reaches nothing.
#[test]
fn data_attr_is_still_unknown_on_a_fragment_child() {
    let host = lint_host(&[
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\ndefineProps<{ label: string }>()\n</script>\n<template><div/><span/></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" data-test=\"x\" /></template>\n",
        ),
    ]);

    assert_eq!(
        flagged(&lint_unknown_props(&host, "/src/App.vue")),
        vec!["data-test"],
        "a fragment inherits nothing, so data-* reaches nothing"
    );
}

/// Recursive propagation: the child's root is a COMPONENT whose own root is a
/// native `<div>`, so `title` still reaches the DOM.
#[test]
fn attr_reaching_the_dom_through_a_component_root_is_not_unknown() {
    let host = lint_host(&[
        (
            "/src/Grandchild.vue",
            "<script setup lang=\"ts\"></script>\n<template><div>gc</div></template>\n",
        ),
        (
            "/src/Child.vue",
            "<script setup lang=\"ts\">\nimport Grandchild from './Grandchild.vue'\ndefineProps<{ label: string }>()\n</script>\n<template><Grandchild/></template>\n",
        ),
        (
            "/src/App.vue",
            "<script setup lang=\"ts\">\nimport Child from './Child.vue'\n</script>\n<template><Child label=\"a\" title=\"hi\" /></template>\n",
        ),
    ]);

    let unknowns = lint_unknown_props(&host, "/src/App.vue");
    assert!(
        unknowns.is_empty(),
        "`title` falls through Child into Grandchild's <div> root; got {:?}",
        flagged(&unknowns)
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
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
                    payload: None,
                    type_expr_scope: None,
                    declared_in_macro_type_arg: false,
                })
                .collect(),
            emit_fields: vec![],
            slot_fields: vec![],
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));

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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));

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
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
                parsed_type_argument_scope: None,
                span: verter_span::Span::new(0, 50),
            },
            AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
                        payload: None,
                        type_expr_scope: None,
                        declared_in_macro_type_arg: false,
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
                        payload: None,
                        type_expr_scope: None,
                        declared_in_macro_type_arg: false,
                    },
                ],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                parsed_type_argument: None,
                parsed_type_argument_scope: None,
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let unknowns = find_unknown_props(&parent, &|_| Some(no_fallthrough(child.clone())));

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
        bindings: vec![],
        events: vec![],
        span: verter_span::Span::new(0, 50),
    }
}

fn make_child_with_required_slots(slot_names: &[(&str, bool)]) -> FileAnalysisSnapshot {
    FileAnalysisSnapshot {
        macros: vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineSlots,
            owner: verter_type_expr::TopLevelOwnerId::instance(0),
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
                        payload: None,
                        return_expr_scope: None,
                    },
                )
                .collect(),
            default_keys: vec![],
            expose_fields: vec![],
            default_values: Vec::new(),
            resolved_local_types: Vec::new(),
            parsed_type_argument: None,
            parsed_type_argument_scope: None,
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

    let missing = find_missing_required_slots(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let missing = find_missing_required_slots(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let missing = find_missing_required_slots(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let missing = find_missing_required_slots(&parent, &|_| Some(no_fallthrough(child.clone())));
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

    let missing = find_missing_required_slots(&parent, &|_| Some(no_fallthrough(child.clone())));
    assert!(
        missing.is_empty(),
        "no defineSlots should not report missing required slots"
    );
}
