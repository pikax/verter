use super::*;
use crate::type_expr::PrimitiveName;
use crate::types::AnalyzedExposeField;

fn empty_input(macros: &[AnalyzedMacro]) -> ComponentMetaInput<'_> {
    ComponentMetaInput {
        macros,
        resolved_macros: &[],
        resolved_type_registry: &[],
        bindings: &[],
        imports: &[],
        template: None,
        options_api: None,
        analysis_flags: crate::types::AnalysisFlags::default(),
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
    let evaluated = crate::type_expand::ExpandedComponentTypes {
        props: vec![crate::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            optional: false,
            completeness: crate::type_expand::ExpansionCompleteness::Exact,
            diagnostics: Vec::new(),
        }],
        define_props: Vec::new(),
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 1);
    assert_eq!(
        result.props[0].type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "should prefer evaluated type over raw annotation"
    );
    assert_eq!(
        result.props[0]
            .type_expansion
            .as_ref()
            .map(|meta| meta.completeness),
        Some(crate::type_expand::ExpansionCompleteness::Exact)
    );
}

#[test]
fn props_preserve_expansion_metadata_when_available() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("Missing"),
        false,
    )])];
    let evaluated = crate::type_expand::ExpandedComponentTypes {
        props: vec![crate::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::named("Missing"),
            optional: false,
            completeness: crate::type_expand::ExpansionCompleteness::Partial,
            diagnostics: vec![crate::type_expand::ExpansionDiagnostic {
                reason: crate::type_expand::ExpansionStopReason::UnresolvedReference,
                context: "unresolved type reference 'Missing'".to_string(),
                property_name: None,
            }],
        }],
        define_props: Vec::new(),
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let expansion = result.props[0]
        .type_expansion
        .as_ref()
        .expect("expansion metadata should be preserved");

    assert_eq!(
        expansion.completeness,
        crate::type_expand::ExpansionCompleteness::Partial
    );
    assert!(expansion
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason
            == crate::type_expand::ExpansionStopReason::UnresolvedReference));
}

#[test]
fn evaluated_types_are_used_when_supplied() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("MyType"),
        false,
    )])];
    let evaluated = crate::type_expand::ExpandedComponentTypes {
        props: vec![crate::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            optional: false,
            completeness: crate::type_expand::ExpansionCompleteness::Exact,
            diagnostics: Vec::new(),
        }],
        define_props: Vec::new(),
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    assert_eq!(
        result.props[0].type_expr,
        TypeExpr::Primitive(PrimitiveName::String),
        "projection should use host-supplied evaluated types without a local expansion flag"
    );
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
    let evaluated = crate::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: vec![crate::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::type_expand::ExpansionResult::exact(
                crate::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::type_expand::ExpandedProperty {
                            name: "x".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                        },
                        crate::type_expand::ExpandedProperty {
                            name: "y".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        emits: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 2);
    assert_eq!(result.props[0].name, "x");
    assert!(result.props[0].required);
    assert_eq!(result.props[1].name, "y");
    assert!(!result.props[1].required);
    assert!(result.props.iter().all(|prop| prop.raw_type.is_none()));
}

#[test]
fn resolved_macro_projection_merges_all_entries_for_one_macro_index() {
    let macros = vec![make_define_props(Vec::new())];
    let resolved = vec![
        crate::component_meta::ResolvedMacroInput {
            macro_index: 0,
            props: vec![make_prop("x", Some("string"), false)],
            emits: Vec::new(),
            slots: Vec::new(),
        },
        crate::component_meta::ResolvedMacroInput {
            macro_index: 0,
            props: vec![make_prop("y", Some("number"), true)],
            emits: Vec::new(),
            slots: Vec::new(),
        },
    ];

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved;

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 2);
    assert_eq!(
        result
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect::<Vec<_>>(),
        vec!["x", "y"]
    );
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

#[test]
fn runtime_define_props_defaults_are_preserved() {
    let define_props = AnalyzedMacro {
        default_keys: vec!["hello".to_string()],
        default_values: vec![crate::types::AnalyzedDefaultValue {
            key: "hello".to_string(),
            value: "\"Hello\"".to_string(),
            span: verter_span::Span::default(),
        }],
        prop_fields: vec![make_prop("hello", Some("string"), false)],
        ..make_define_props(vec![])
    };
    let macros = vec![define_props];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.props.len(), 1);
    let hello = result.props.iter().find(|p| p.name == "hello").unwrap();
    assert!(
        hello.has_default,
        "runtime defineProps default should be preserved"
    );
    assert!(
        !hello.required,
        "runtime defineProps default should make the prop optional in compat metadata"
    );
    assert_eq!(hello.default_value.as_deref(), Some("\"Hello\""));
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

#[test]
fn resolved_macros_supply_imported_metadata_without_snapshot_mutation() {
    let macros = vec![make_define_props(Vec::new())];
    let resolved_fields = vec![
        make_prop("imported", Some("string"), false),
        make_prop("optionalImported", Some("number"), true),
    ];
    let resolved_macros = vec![ResolvedMacroInput {
        macro_index: 0,
        props: resolved_fields,
        emits: Vec::new(),
        slots: Vec::new(),
    }];

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;

    let result = extract_component_meta(input);
    let names: Vec<&str> = result.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        names.contains(&"imported"),
        "component_meta should project host-supplied imported props without mutating macros: {:?}",
        names
    );
    assert!(
        names.contains(&"optionalImported"),
        "component_meta should project all supplied resolved props: {:?}",
        names
    );
}

#[test]
fn resolved_macros_merge_with_local_prop_fields_for_mixed_type_sources() {
    let macros = vec![make_define_props(vec![make_prop(
        "localOnly",
        Some("string"),
        false,
    )])];
    let resolved_macros = vec![ResolvedMacroInput {
        macro_index: 0,
        props: vec![make_prop("importedOnly", Some("number"), true)],
        emits: Vec::new(),
        slots: Vec::new(),
    }];

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;

    let result = extract_component_meta(input);
    let names: Vec<&str> = result.props.iter().map(|prop| prop.name.as_str()).collect();

    assert!(
        names.contains(&"localOnly"),
        "local prop fields should still be projected: {:?}",
        names
    );
    assert!(
        names.contains(&"importedOnly"),
        "host-resolved imported fields should merge with local fields for mixed type sources: {:?}",
        names
    );
}

#[test]
fn type_registry_comes_from_resolved_inputs_not_macro_local_types() {
    let macros = vec![make_define_props(Vec::new())];
    let registry = vec![ResolvedTypeAnalysis {
        name: "ImportedProps".to_string(),
        type_expr: TypeExpr::Primitive(PrimitiveName::String),
        type_expansion: None,
    }];

    let mut input = empty_input(&macros);
    input.resolved_type_registry = &registry;

    let result = extract_component_meta(input);

    assert_eq!(
        result.type_registry.len(),
        1,
        "resolved type registry should be projected"
    );
    assert_eq!(result.type_registry[0].name, "ImportedProps");
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
        resolved_macros: &[],
        resolved_type_registry: &[],
        bindings: &[],
        imports: &[],
        template: None,
        options_api: Some(&opts),
        analysis_flags: crate::types::AnalysisFlags::HAS_OPTIONS_API,
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

#[test]
fn options_api_prop_type_annotation_is_preserved() {
    let opts = AnalyzedOptionsApi {
        props: vec![crate::types::AnalyzedOptionsProp {
            name: "canvas".to_string(),
            type_constructor: Some("Object".to_string()),
            is_required: true,
            has_default: false,
            default_value: None,
            type_annotation: Some("HTMLCanvasElement".to_string()),
            description: None,
            tags: Vec::new(),
            span: verter_span::Span::default(),
        }],
        ..Default::default()
    };
    let input = ComponentMetaInput {
        macros: &[],
        resolved_macros: &[],
        resolved_type_registry: &[],
        bindings: &[],
        imports: &[],
        template: None,
        options_api: Some(&opts),
        analysis_flags: crate::types::AnalysisFlags::HAS_OPTIONS_API,
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
    };

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 1);
    assert_eq!(
        result.props[0].type_expr,
        unknown_type("HTMLCanvasElement".to_string()),
        "PropType<T> annotation should survive Options API extraction"
    );
    assert_eq!(
        result.props[0].raw_type.as_deref(),
        Some("HTMLCanvasElement"),
        "raw_type should preserve the PropType<T> annotation for compat consumers"
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

// ===========================================================================
// Root Reachability Tests
// ===========================================================================

use crate::template::{
    TemplateAnalysisSnapshot, TemplateAttribute, TemplateComponentUsage, TemplateDirective,
    TemplateElement,
};

fn make_flags(inherit_attrs_false: bool) -> ComponentMetaFlags {
    ComponentMetaFlags {
        has_inherit_attrs_false: inherit_attrs_false,
        ..Default::default()
    }
}

fn make_native_root_element(tag: &str) -> TemplateElement {
    TemplateElement {
        tag: tag.to_string(),
        is_component: false,
        parent_index: None,
        parent_tag: None,
        ..Default::default()
    }
}

fn make_component_root_element(tag: &str) -> TemplateElement {
    TemplateElement {
        tag: tag.to_string(),
        is_component: true,
        parent_index: None,
        parent_tag: None,
        ..Default::default()
    }
}

fn make_template_with_elements(elements: Vec<TemplateElement>) -> TemplateAnalysisSnapshot {
    TemplateAnalysisSnapshot {
        elements,
        ..Default::default()
    }
}

fn make_template_with_elements_and_components(
    elements: Vec<TemplateElement>,
    components: Vec<TemplateComponentUsage>,
) -> TemplateAnalysisSnapshot {
    TemplateAnalysisSnapshot {
        elements,
        components,
        ..Default::default()
    }
}

// ── inheritAttrs: false ──────────────────────────────────────────────────

#[test]
fn root_reachability_inherit_attrs_false() {
    let template = make_template_with_elements(vec![make_native_root_element("div")]);
    let flags = make_flags(true);
    let result = extract_root_reachability(Some(&template), &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::InheritAttrsFalse
            }
        ),
        "inheritAttrs: false must yield NoFallthrough, got: {:?}",
        result
    );
}

// ── No template ──────────────────────────────────────────────────────────

#[test]
fn root_reachability_no_template() {
    let flags = make_flags(false);
    let result = extract_root_reachability(None, &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::NoTemplate
            }
        ),
        "no template must yield NoFallthrough::NoTemplate, got: {:?}",
        result
    );
}

// ── Empty template ──────────────────────────────────────────────────────

#[test]
fn root_reachability_empty_template() {
    let template = make_template_with_elements(vec![]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::EmptyTemplate
            }
        ),
        "empty template must yield NoFallthrough::EmptyTemplate, got: {:?}",
        result
    );
}

// ── Multi-root ──────────────────────────────────────────────────────────

#[test]
fn root_reachability_multi_root() {
    let template = make_template_with_elements(vec![
        make_native_root_element("div"),
        make_native_root_element("span"),
    ]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::MultiRoot
            }
        ),
        "two independent root elements must yield MultiRoot, got: {:?}",
        result
    );
}

// ── Root v-for ──────────────────────────────────────────────────────────

#[test]
fn root_reachability_root_v_for() {
    use crate::template::VForDirective;
    let mut el = make_native_root_element("div");
    el.v_for = Some(VForDirective {
        variable: "item".to_string(),
        index: None,
        iterable: "items".to_string(),
        has_key: false,
        key_expression: None,
        key_uses_index: false,
        span: verter_span::Span::default(),
    });

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::RootVFor
            }
        ),
        "root v-for must yield NoFallthrough::RootVFor, got: {:?}",
        result
    );
}

// ── Single native root ──────────────────────────────────────────────────

#[test]
fn root_reachability_single_native_root() {
    let template = make_template_with_elements(vec![make_native_root_element("div")]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1, "single root must produce 1 branch");
            assert_eq!(branches[0].branch_index, 0);
            match &branches[0].target {
                RootTargetRef::NativeElement { tag, .. } => {
                    assert_eq!(tag, "div");
                }
                other => panic!("expected NativeElement, got: {:?}", other),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Component root with usage link ──────────────────────────────────────

#[test]
fn root_reachability_component_usage_link_preserved() {
    let mut comp_el = make_component_root_element("MyComp");
    comp_el.component_usage_index = Some(0);

    let components = vec![TemplateComponentUsage {
        name: "MyComp".to_string(),
        import_source: Some("./MyComp.vue".to_string()),
        is_dynamic: false,
        props: vec![],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::default(),
    }];

    let template = make_template_with_elements_and_components(vec![comp_el], components);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1);
            match &branches[0].target {
                RootTargetRef::ComponentUsage {
                    name,
                    import_source,
                    usage_index,
                    ..
                } => {
                    assert_eq!(name, "MyComp");
                    assert_eq!(import_source.as_deref(), Some("./MyComp.vue"));
                    assert_eq!(*usage_index, 0);
                }
                other => panic!("expected ComponentUsage, got: {:?}", other),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Dynamic component without usage link stays unresolved ───────────────

#[test]
fn root_reachability_dynamic_component_missing_usage_link_is_unresolved_target() {
    let el = TemplateElement {
        tag: "component".to_string(),
        is_component: true,
        parent_index: None,
        parent_tag: None,
        ..Default::default()
    };

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1);
            match &branches[0].target {
                RootTargetRef::UnresolvedTarget { reason, .. } => {
                    assert_eq!(*reason, UnresolvedRootTargetReason::MissingUsageLink);
                }
                other => panic!("expected UnresolvedTarget, got: {:?}", other),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_dynamic_component_static_is_is_not_thrown_away() {
    let mut el = TemplateElement {
        tag: "component".to_string(),
        is_component: true,
        parent_index: None,
        parent_tag: None,
        component_usage_index: Some(0),
        ..Default::default()
    };
    el.directives = vec![TemplateDirective {
        name: "bind".to_string(),
        raw_name: ":is".to_string(),
        argument: Some("is".to_string()),
        modifiers: vec![],
        expression: Some("showNative ? 'div' : Child".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let components = vec![TemplateComponentUsage {
        name: "component".to_string(),
        import_source: None,
        is_dynamic: true,
        props: vec![crate::template::TemplatePropUsage {
            name: "is".to_string(),
            is_bound: true,
            constness: crate::template::PropValueConstness::Dynamic,
            referenced_bindings: vec!["showNative".to_string(), "Child".to_string()],
            expression: Some("showNative ? 'div' : Child".to_string()),
            from_spread: false,
            span: verter_span::Span::default(),
            name_span: verter_span::Span::default(),
            is_shorthand: false,
        }],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::default(),
    }];

    let template = make_template_with_elements_and_components(vec![el], components);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1);
            assert!(
                !matches!(
                    &branches[0].target,
                    RootTargetRef::UnresolvedTarget {
                        reason: UnresolvedRootTargetReason::DynamicComponentIs,
                        ..
                    }
                ),
                "static :is candidates must survive into root reachability facts"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_root_template_vif_single_child_uses_actual_child_target() {
    let wrapper = TemplateElement {
        tag: "template".to_string(),
        is_component: false,
        parent_index: None,
        parent_tag: None,
        has_v_if: true,
        v_if_condition: Some("show".to_string()),
        has_element_children: true,
        ..Default::default()
    };
    let child = TemplateElement {
        tag: "div".to_string(),
        is_component: false,
        parent_index: Some(0),
        parent_tag: Some("template".to_string()),
        ..Default::default()
    };

    let template = make_template_with_elements(vec![wrapper, child]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(
                branches.len(),
                1,
                "wrapper branch should unwrap to one branch"
            );
            assert_eq!(branches[0].condition_text.as_deref(), Some("show"));
            match &branches[0].target {
                RootTargetRef::NativeElement { tag, element_index } => {
                    assert_eq!(
                        tag, "div",
                        "wrapper should normalize to actual child target"
                    );
                    assert_eq!(
                        *element_index, 1,
                        "branch should point at the actual child element index"
                    );
                }
                other => panic!("expected NativeElement div, got: {:?}", other),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_root_template_vif_multi_child_disables_fallthrough() {
    let wrapper = TemplateElement {
        tag: "template".to_string(),
        is_component: false,
        parent_index: None,
        parent_tag: None,
        has_v_if: true,
        v_if_condition: Some("show".to_string()),
        has_element_children: true,
        ..Default::default()
    };
    let child_a = TemplateElement {
        tag: "div".to_string(),
        is_component: false,
        parent_index: Some(0),
        parent_tag: Some("template".to_string()),
        ..Default::default()
    };
    let child_b = TemplateElement {
        tag: "span".to_string(),
        is_component: false,
        parent_index: Some(0),
        parent_tag: Some("template".to_string()),
        ..Default::default()
    };

    let template = make_template_with_elements(vec![wrapper, child_a, child_b]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    assert!(
        matches!(
            result,
            RootReachability::NoFallthrough {
                reason: NoFallthroughReason::BranchNotSingleRoot
            }
        ),
        "root template wrapper with multiple actual child roots must disable fallthrough, got: {:?}",
        result
    );
}

// ── Built-in root is unresolved ────────────────────────────────────

#[test]
fn root_reachability_builtin_root_is_unresolved_target() {
    let el = TemplateElement {
        tag: "Teleport".to_string(),
        is_component: true,
        parent_index: None,
        parent_tag: None,
        ..Default::default()
    };

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1);
            match &branches[0].target {
                RootTargetRef::UnresolvedTarget {
                    reason: UnresolvedRootTargetReason::UnsupportedBuiltin { tag },
                    ..
                } => {
                    assert_eq!(tag, "Teleport");
                }
                other => panic!(
                    "expected UnresolvedTarget::UnsupportedBuiltin, got: {:?}",
                    other
                ),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Slot root is unresolved ────────────────────────────────────────

#[test]
fn root_reachability_slot_root_is_unresolved_target() {
    let el = TemplateElement {
        tag: "slot".to_string(),
        is_component: false,
        parent_index: None,
        parent_tag: None,
        ..Default::default()
    };

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(branches.len(), 1);
            match &branches[0].target {
                RootTargetRef::UnresolvedTarget {
                    reason: UnresolvedRootTargetReason::SlotOutlet,
                    ..
                } => {}
                other => panic!("expected UnresolvedTarget::SlotOutlet, got: {:?}", other),
            }
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Conditional root (v-if / v-else) ────────────────────────────────────

#[test]
fn root_reachability_conditional_native_branches() {
    let mut div = make_native_root_element("div");
    div.has_v_if = true;
    div.v_if_condition = Some("show".to_string());

    let mut span = make_native_root_element("span");
    span.has_v_else = true;

    let template = make_template_with_elements(vec![div, span]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert_eq!(
                branches.len(),
                2,
                "v-if/v-else chain must produce 2 branches"
            );
            // Branch 0: div with condition
            assert_eq!(branches[0].branch_index, 0);
            assert_eq!(branches[0].condition_text.as_deref(), Some("show"));
            assert!(matches!(
                &branches[0].target,
                RootTargetRef::NativeElement { tag, .. } if tag == "div"
            ));
            // Branch 1: span (v-else, no condition)
            assert_eq!(branches[1].branch_index, 1);
            assert!(branches[1].condition_text.is_none());
            assert!(matches!(
                &branches[1].target,
                RootTargetRef::NativeElement { tag, .. } if tag == "span"
            ));
        }
        other => panic!("expected Branches with 2 entries, got: {:?}", other),
    }
}

// ── Consumed attrs and listeners ────────────────────────────────────────

#[test]
fn root_reachability_consumed_attrs_and_listeners() {
    let mut el = make_native_root_element("div");
    el.attributes = vec![
        TemplateAttribute {
            name: "disabled".to_string(),
            value: None,
            is_dynamic: false,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
        TemplateAttribute {
            name: "id".to_string(),
            value: Some("app".to_string()),
            is_dynamic: true,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
    ];
    el.directives = vec![TemplateDirective {
        name: "on".to_string(),
        raw_name: "@click".to_string(),
        argument: Some("click".to_string()),
        modifiers: vec![],
        expression: Some("handler".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"disabled".to_string()),
                "disabled should be in consumed attrs"
            );
            assert!(
                consumed.attrs.contains(&"id".to_string()),
                "id should be in consumed attrs"
            );
            assert!(
                consumed.listeners.contains(&"click".to_string()),
                "@click should produce consumed listener 'click'"
            );
            // Negative: class and style never consumed
            assert!(
                !consumed.attrs.contains(&"class".to_string()),
                "class must not be consumed"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── class and style never consumed ──────────────────────────────────────

#[test]
fn root_reachability_class_style_not_consumed() {
    let mut el = make_native_root_element("div");
    el.attributes = vec![
        TemplateAttribute {
            name: "class".to_string(),
            value: Some("foo".to_string()),
            is_dynamic: false,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
        TemplateAttribute {
            name: "style".to_string(),
            value: Some("color: red".to_string()),
            is_dynamic: false,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
    ];
    // Also add dynamic :class and :style via v-bind
    el.directives = vec![
        TemplateDirective {
            name: "bind".to_string(),
            raw_name: ":class".to_string(),
            argument: Some("class".to_string()),
            modifiers: vec![],
            expression: Some("{ active: true }".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
        TemplateDirective {
            name: "bind".to_string(),
            raw_name: ":style".to_string(),
            argument: Some("style".to_string()),
            modifiers: vec![],
            expression: Some("styles".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
    ];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.is_empty(),
                "class and style must never be consumed, got: {:?}",
                consumed.attrs
            );
            assert!(
                consumed.listeners.is_empty(),
                "no listeners should be consumed, got: {:?}",
                consumed.listeners
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── @click and :onClick normalize to same consumed listener ─────────────

#[test]
fn root_reachability_at_click_and_on_click_normalize_to_same_consumed_name() {
    let mut el = make_native_root_element("div");
    el.directives = vec![
        // @click
        TemplateDirective {
            name: "on".to_string(),
            raw_name: "@click".to_string(),
            argument: Some("click".to_string()),
            modifiers: vec![],
            expression: Some("handler1".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
        // :onClick
        TemplateDirective {
            name: "bind".to_string(),
            raw_name: ":onClick".to_string(),
            argument: Some("onClick".to_string()),
            modifiers: vec![],
            expression: Some("handler2".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
    ];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            // Both should normalize to "click" and be deduped
            assert_eq!(
                consumed.listeners,
                vec!["click"],
                "@click and :onClick must normalize to one 'click' consumed listener"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── v-bind spread marks dynamic listener name ───────────────────────────

#[test]
fn root_reachability_v_bind_spread_marks_dynamic_listener_name() {
    let mut el = make_native_root_element("div");
    el.directives = vec![
        // v-bind="attrs" (spread without argument)
        TemplateDirective {
            name: "bind".to_string(),
            raw_name: "v-bind".to_string(),
            argument: None,
            modifiers: vec![],
            expression: Some("attrs".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
    ];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            assert!(
                branches[0].has_unknown_spread,
                "v-bind without argument must set has_unknown_spread"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── v-model consumes component model members ────────────────────────────

#[test]
fn root_reachability_v_model_consumes_component_model_members() {
    let mut el = make_component_root_element("MyComp");
    el.component_usage_index = Some(0);
    el.directives = vec![
        // v-model:title
        TemplateDirective {
            name: "model".to_string(),
            raw_name: "v-model:title".to_string(),
            argument: Some("title".to_string()),
            modifiers: vec![],
            expression: Some("myTitle".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
    ];

    let components = vec![TemplateComponentUsage {
        name: "MyComp".to_string(),
        import_source: Some("./MyComp.vue".to_string()),
        is_dynamic: false,
        props: vec![],
        has_spread: false,
        slots_used: vec![],
        static_classes: vec![],
        has_dynamic_class: false,
        dynamic_classes: vec![],
        v_models: vec![],
        span: verter_span::Span::default(),
    }];

    let template = make_template_with_elements_and_components(vec![el], components);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"title".to_string()),
                "v-model:title must consume 'title' attr"
            );
            assert!(
                consumed.listeners.contains(&"update:title".to_string()),
                "v-model:title must consume 'update:title' listener"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── v-model consumes native model members ───────────────────────────────

#[test]
fn root_reachability_v_model_consumes_native_model_members() {
    let mut el = make_native_root_element("input");
    el.directives = vec![TemplateDirective {
        name: "model".to_string(),
        raw_name: "v-model".to_string(),
        argument: None,
        modifiers: vec![],
        expression: Some("text".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"value".to_string()),
                "native v-model must consume 'value' attr"
            );
            assert!(
                consumed.listeners.contains(&"input".to_string()),
                "native v-model must consume 'input' listener"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── v-else alone doesn't count as independent root ──────────────────────

#[test]
fn root_reachability_v_else_is_branch_not_independent_root() {
    let mut div = make_native_root_element("div");
    div.has_v_if = true;
    div.v_if_condition = Some("show".to_string());

    let mut span = make_native_root_element("span");
    span.has_v_else = true;

    let template = make_template_with_elements(vec![div, span]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    // Should be Branches (conditional single root), NOT MultiRoot
    assert!(
        matches!(result, RootReachability::Branches { .. }),
        "v-if/v-else chain must be Branches (not MultiRoot), got: {:?}",
        result
    );
}

// ── condition_text is debug-only ────────────────────────────────────────

#[test]
fn root_reachability_condition_text_is_debug_only() {
    let mut div = make_native_root_element("div");
    div.has_v_if = true;
    div.v_if_condition = Some("  show  ".to_string());

    let template = make_template_with_elements(vec![div]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            // condition_text is preserved as-is (including whitespace)
            assert_eq!(branches[0].condition_text.as_deref(), Some("  show  "));
            // branch_index is the stable identity, not condition_text
            assert_eq!(branches[0].branch_index, 0);
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Computed/dynamic attribute names ────────────────────────────────────

#[test]
fn root_reachability_computed_attr_name_marks_dynamic() {
    let mut el = make_native_root_element("div");
    el.attributes = vec![
        // :[key]="value" — computed attr name
        TemplateAttribute {
            name: "[key]".to_string(),
            value: Some("value".to_string()),
            is_dynamic: true,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
        // :disabled="true" — static name, dynamic value
        TemplateAttribute {
            name: "disabled".to_string(),
            value: Some("true".to_string()),
            is_dynamic: true,
            span: verter_span::Span::default(),
            name_end: 0,
            value_span: None,
        },
    ];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            // Positive: has_dynamic_attr_name must be set for :[key]
            assert!(
                consumed.has_dynamic_attr_name,
                "computed attr name :[key] must set has_dynamic_attr_name"
            );
            // Positive: :disabled has a known name — should be in consumed attrs
            assert!(
                consumed.attrs.contains(&"disabled".to_string()),
                ":disabled should be consumed with known name 'disabled'"
            );
            // Negative: [key] should NOT appear in consumed.attrs (it's dynamic)
            assert!(
                !consumed.attrs.contains(&"[key]".to_string()),
                "computed attr [key] must NOT appear in consumed.attrs"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_dynamic_event_name_marks_dynamic_listener() {
    let mut el = make_native_root_element("div");
    el.directives = vec![
        // @[eventName]="handler" — computed listener name
        TemplateDirective {
            name: "on".to_string(),
            raw_name: "@[eventName]".to_string(),
            argument: Some("[eventName]".to_string()),
            modifiers: vec![],
            expression: Some("handler".to_string()),
            span: verter_span::Span::default(),
            name_end: 0,
            arg_span: None,
            expression_span: None,
            modifier_spans: vec![],
        },
    ];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.has_dynamic_listener_name,
                "@[eventName] must set has_dynamic_listener_name"
            );
            // Negative: the computed name should NOT appear as a concrete consumed listener
            assert!(
                consumed.listeners.is_empty(),
                "computed listener name should not be in concrete listeners list"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

// ── Native v-model element-type-aware consumption ───────────────────────

#[test]
fn root_reachability_v_model_checkbox_consumes_checked_and_change() {
    let mut el = make_native_root_element("input");
    el.attributes = vec![TemplateAttribute {
        name: "type".to_string(),
        value: Some("checkbox".to_string()),
        is_dynamic: false,
        span: verter_span::Span::default(),
        name_end: 0,
        value_span: None,
    }];
    el.directives = vec![TemplateDirective {
        name: "model".to_string(),
        raw_name: "v-model".to_string(),
        argument: None,
        modifiers: vec![],
        expression: Some("checked".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"checked".to_string()),
                "checkbox v-model must consume 'checked', got: {:?}",
                consumed.attrs
            );
            assert!(
                consumed.listeners.contains(&"change".to_string()),
                "checkbox v-model must consume 'change' listener, got: {:?}",
                consumed.listeners
            );
            // Negative: must NOT consume 'value' or 'input' for checkbox
            assert!(
                !consumed.attrs.contains(&"value".to_string()),
                "checkbox v-model must NOT consume 'value'"
            );
            assert!(
                !consumed.listeners.contains(&"input".to_string()),
                "checkbox v-model must NOT consume 'input' listener"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_v_model_select_consumes_value_and_change() {
    let mut el = make_native_root_element("select");
    el.directives = vec![TemplateDirective {
        name: "model".to_string(),
        raw_name: "v-model".to_string(),
        argument: None,
        modifiers: vec![],
        expression: Some("selected".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"value".to_string()),
                "select v-model must consume 'value'"
            );
            assert!(
                consumed.listeners.contains(&"change".to_string()),
                "select v-model must consume 'change' listener"
            );
            // Negative: must NOT consume 'input' for select
            assert!(
                !consumed.listeners.contains(&"input".to_string()),
                "select v-model must NOT consume 'input' listener"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}

#[test]
fn root_reachability_v_model_radio_consumes_checked_and_change() {
    let mut el = make_native_root_element("input");
    el.attributes = vec![TemplateAttribute {
        name: "type".to_string(),
        value: Some("radio".to_string()),
        is_dynamic: false,
        span: verter_span::Span::default(),
        name_end: 0,
        value_span: None,
    }];
    el.directives = vec![TemplateDirective {
        name: "model".to_string(),
        raw_name: "v-model".to_string(),
        argument: None,
        modifiers: vec![],
        expression: Some("option".to_string()),
        span: verter_span::Span::default(),
        name_end: 0,
        arg_span: None,
        expression_span: None,
        modifier_spans: vec![],
    }];

    let template = make_template_with_elements(vec![el]);
    let flags = make_flags(false);
    let result = extract_root_reachability(Some(&template), &flags);

    match result {
        RootReachability::Branches { ref branches } => {
            let consumed = &branches[0].consumed;
            assert!(
                consumed.attrs.contains(&"checked".to_string()),
                "radio v-model must consume 'checked'"
            );
            assert!(
                consumed.listeners.contains(&"change".to_string()),
                "radio v-model must consume 'change' listener"
            );
            // Negative: must NOT consume 'value' or 'input' for radio
            assert!(
                !consumed.attrs.contains(&"value".to_string()),
                "radio v-model must NOT consume 'value'"
            );
        }
        other => panic!("expected Branches, got: {:?}", other),
    }
}
