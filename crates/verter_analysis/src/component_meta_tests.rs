use super::*;
use crate::type_expr::PrimitiveName;
use crate::types::AnalyzedExposeField;

fn empty_input(macros: &[AnalyzedMacro]) -> ComponentMetaInput<'_> {
    ComponentMetaInput {
        macros,
        bindings: &[],
        imports: &[],
        template: None,
        options_api: None,
        analysis_flags: crate::types::AnalysisFlags::default(),
        features: ComponentMetaFeatures::default(),
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
    }
}

fn make_define_props(fields: Vec<AnalyzedPropField>) -> AnalyzedMacro {
    AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineProps,
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: fields,
        emit_fields: Vec::new(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        span: verter_span::Span::default(),
    }
}

fn make_prop(name: &str, type_ann: Option<&str>, optional: bool) -> AnalyzedPropField {
    AnalyzedPropField {
        name: name.to_string(),
        is_optional: optional,
        span: verter_span::Span::default(),
        type_annotation: type_ann.map(|s| s.to_string()),
        description: None,
        tags: Vec::new(),
        resolution_source: crate::types::TypeResolutionSource::Rust,
        resolution_error: None,
    }
}

// ---------------------------------------------------------------------------
// Basic prop extraction
// ---------------------------------------------------------------------------

#[test]
fn extracts_props_from_define_props_macro() {
    let macros = vec![make_define_props(vec![
        make_prop("label", Some("string"), false),
        make_prop("count", Some("number"), true),
    ])];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.props.len(), 2, "should extract 2 props");
    assert_eq!(result.props[0].name, "label");
    assert!(
        result.props[0].required,
        "non-optional prop without default should be required"
    );
    assert_eq!(result.props[1].name, "count");
    assert!(
        !result.props[1].required,
        "optional prop should not be required"
    );

    // Negative: no events/slots/models/exposed
    assert!(
        result.events.is_empty(),
        "no events should be extracted from defineProps"
    );
    assert!(
        result.slots.is_empty(),
        "no slots should be extracted from defineProps"
    );
    assert!(
        result.models.is_empty(),
        "no models should be extracted from defineProps"
    );
}

#[test]
fn props_use_evaluated_type_when_available() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("string"),
        false,
    )])];
    let evaluated = crate::type_eval_build::EvaluatedComponentTypes {
        props: vec![crate::type_eval_build::EvaluatedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            optional: false,
        }],
        define_props: Vec::new(),
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.features.expanded_types = true;
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 1);
    assert_eq!(
        result.props[0].type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "should prefer evaluated type over raw annotation"
    );
}

#[test]
fn expanded_types_are_disabled_without_feature_selection() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("MyType"),
        false,
    )])];
    let evaluated = crate::type_eval_build::EvaluatedComponentTypes {
        props: vec![crate::type_eval_build::EvaluatedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            optional: false,
        }],
        define_props: Vec::new(),
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    match &result.props[0].type_expr {
        TypeExpr::Unknown { raw } => assert_eq!(raw, "MyType"),
        other => panic!("expected Unknown(\"MyType\") when expansion is disabled, got {other:?}"),
    }
}

#[test]
fn props_fall_back_to_unknown_when_no_evaluated_type() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("MyType"),
        false,
    )])];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.props.len(), 1);
    match &result.props[0].type_expr {
        TypeExpr::Unknown { raw } => assert_eq!(raw, "MyType"),
        other => panic!("expected Unknown(\"MyType\"), got {other:?}"),
    }
    assert_eq!(
        result.props[0].raw_type.as_deref(),
        Some("MyType"),
        "raw_type should preserve the annotation text"
    );
}

#[test]
fn define_props_eval_supplements_missing_prop_fields() {
    let macros = vec![make_define_props(Vec::new())];
    let evaluated = crate::type_eval_build::EvaluatedComponentTypes {
        props: Vec::new(),
        define_props: vec![crate::type_eval_build::EvaluatedMacroProps {
            macro_index: 0,
            fields: vec![
                crate::type_eval_build::EvaluatedField {
                    name: "x".to_string(),
                    r#type: TypeExpr::Primitive(PrimitiveName::Number),
                    optional: false,
                },
                crate::type_eval_build::EvaluatedField {
                    name: "y".to_string(),
                    r#type: TypeExpr::Primitive(PrimitiveName::String),
                    optional: true,
                },
            ],
        }],
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.features.expanded_types = true;
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 2);
    assert_eq!(result.props[0].name, "x");
    assert!(result.props[0].required);
    assert_eq!(result.props[1].name, "y");
    assert!(!result.props[1].required);
    assert!(result.props.iter().all(|prop| prop.raw_type.is_none()));
}

// ---------------------------------------------------------------------------
// WithDefaults handling
// ---------------------------------------------------------------------------

#[test]
fn with_defaults_marks_props_as_having_defaults() {
    let define_props = make_define_props(vec![
        make_prop("label", Some("string"), true),
        make_prop("count", Some("number"), true),
    ]);
    let with_defaults = AnalyzedMacro {
        kind: AnalyzedMacroKind::WithDefaults,
        default_keys: vec!["label".to_string()],
        default_values: vec![crate::types::AnalyzedDefaultValue {
            key: "label".to_string(),
            value: "\"hello\"".to_string(),
            span: verter_span::Span::default(),
        }],
        ..make_define_props(vec![])
    };
    let macros = vec![define_props, with_defaults];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.props.len(), 2);
    let label = result.props.iter().find(|p| p.name == "label").unwrap();
    assert!(label.has_default, "label should have a default");
    assert!(!label.required, "prop with default should not be required");
    assert_eq!(label.default_value.as_deref(), Some("\"hello\""));

    let count = result.props.iter().find(|p| p.name == "count").unwrap();
    assert!(!count.has_default, "count should NOT have a default");
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn extracts_events_from_define_emits() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        emit_fields: vec![
            crate::types::AnalyzedEmitField {
                name: "change".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[value: string]".to_string()),
                description: None,
                tags: Vec::new(),
            },
            crate::types::AnalyzedEmitField {
                name: "close".to_string(),
                span: verter_span::Span::default(),
                payload_type: None,
                description: None,
                tags: Vec::new(),
            },
        ],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.events.len(), 2, "should extract 2 events");
    assert_eq!(result.events[0].name, "change");
    assert_eq!(
        result.events[0].raw_signature.as_deref(),
        Some("[value: string]")
    );
    assert_eq!(result.events[1].name, "close");
    assert!(result.events[1].raw_signature.is_none());

    // Negative: no props from defineEmits
    assert!(result.props.is_empty(), "no props from defineEmits");
}

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

#[test]
fn extracts_slots_from_define_slots() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: true,
            span: verter_span::Span::default(),
            bindings: vec![crate::types::AnalyzedSlotFieldBinding {
                name: "item".to_string(),
                type_annotation: Some("string".to_string()),
                span: verter_span::Span::default(),
            }],
            return_type: None,
            description: None,
            tags: Vec::new(),
        }],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.slots.len(), 1);
    assert_eq!(result.slots[0].name, "default");
    assert!(result.slots[0].is_required);
    assert!(
        result.slots[0].is_scoped,
        "slot with bindings should be scoped"
    );
    assert_eq!(result.slots[0].bindings.len(), 1);
    assert_eq!(result.slots[0].bindings[0].name, "item");
}

// ---------------------------------------------------------------------------
// Models
// ---------------------------------------------------------------------------

#[test]
fn extracts_model_from_define_model() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineModel,
        model_name: None, // default model → "modelValue"
        prop_fields: vec![make_prop("modelValue", Some("string"), true)],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].name, "modelValue");
    assert_eq!(
        result.props.len(),
        1,
        "defineModel should synthesize a model prop"
    );
    assert_eq!(result.props[0].name, "modelValue");
    assert_eq!(
        result.events.len(),
        1,
        "defineModel should synthesize an update event"
    );
    assert_eq!(result.events[0].name, "update:modelValue");
}

#[test]
fn extracts_named_model() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineModel,
        model_name: Some("title".to_string()),
        prop_fields: vec![make_prop("title", Some("string"), true)],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.models.len(), 1);
    assert_eq!(result.models[0].name, "title");
}

// ---------------------------------------------------------------------------
// Exposed
// ---------------------------------------------------------------------------

#[test]
fn extracts_exposed_from_define_expose() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineExpose,
        expose_fields: vec![AnalyzedExposeField {
            name: "focus".to_string(),
            span: verter_span::Span::default(),
        }],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.exposed.len(), 1);
    assert_eq!(result.exposed[0].name, "focus");
}

// ---------------------------------------------------------------------------
// Options API fallback
// ---------------------------------------------------------------------------

#[test]
fn options_api_props_used_when_no_composition_props() {
    let opts = AnalyzedOptionsApi {
        props: vec![crate::types::AnalyzedOptionsProp {
            name: "color".to_string(),
            type_constructor: Some("String".to_string()),
            is_required: false,
            has_default: true,
            default_value: None,
            type_annotation: None,
            description: None,
            tags: Vec::new(),
            span: verter_span::Span::default(),
        }],
        ..Default::default()
    };
    let input = ComponentMetaInput {
        macros: &[],
        bindings: &[],
        imports: &[],
        template: None,
        options_api: Some(&opts),
        analysis_flags: crate::types::AnalysisFlags::HAS_OPTIONS_API,
        features: ComponentMetaFeatures::default(),
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
    };

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 1);
    assert_eq!(result.props[0].name, "color");
    assert!(result.props[0].has_default);
    assert!(result.options_api, "options_api flag should be true");
    assert_eq!(
        result.props[0].type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "String runtime type should map to Primitive(String)"
    );
}

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

#[test]
fn inherit_attrs_false_flag_is_set() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineOptions,
        has_inherit_attrs_false: true,
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));

    assert!(result.flags.has_inherit_attrs_false);
}

#[test]
fn analysis_flags_drive_component_flags() {
    let mut input = empty_input(&[]);
    input.analysis_flags = crate::types::AnalysisFlags::ASYNC_SETUP
        | crate::types::AnalysisFlags::HAS_REACTIVE_STATE
        | crate::types::AnalysisFlags::HAS_COMPUTED
        | crate::types::AnalysisFlags::HAS_WATCHERS
        | crate::types::AnalysisFlags::HAS_LIFECYCLE_HOOKS
        | crate::types::AnalysisFlags::HAS_PROVIDE
        | crate::types::AnalysisFlags::HAS_INJECT;

    let result = extract_component_meta(input);

    assert!(result.flags.async_setup);
    assert!(result.flags.has_reactive_state);
    assert!(result.flags.has_computed);
    assert!(result.flags.has_watchers);
    assert!(result.flags.has_lifecycle_hooks);
    assert!(result.flags.has_provide);
    assert!(result.flags.has_inject);
}

#[test]
fn store_usage_flag_is_set_from_input() {
    let store_usage = crate::types::StoreUsage {
        binding_name: "userStore".to_string(),
        callee: "useUserStore".to_string(),
        import_source: "@/stores/user".to_string(),
        store_api: crate::types::StoreApiClassification::StoreComposable,
        span: verter_span::Span::default(),
        has_store_to_refs: false,
        destructured_props: Vec::new(),
        destructured_without_store_to_refs: false,
    };
    let mut input = empty_input(&[]);
    input.store_usages = std::slice::from_ref(&store_usage);

    let result = extract_component_meta(input);

    assert!(result.flags.has_store_usage);
}

// ---------------------------------------------------------------------------
// Source order preservation
// ---------------------------------------------------------------------------

#[test]
fn preserves_source_order_of_props() {
    let macros = vec![make_define_props(vec![
        make_prop("zebra", Some("string"), false),
        make_prop("alpha", Some("number"), false),
        make_prop("middle", Some("boolean"), false),
    ])];

    let result = extract_component_meta(empty_input(&macros));

    let names: Vec<&str> = result.props.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["zebra", "alpha", "middle"],
        "props must preserve source order, not be sorted"
    );
}
