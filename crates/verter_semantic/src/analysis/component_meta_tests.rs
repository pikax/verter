use super::*;
use crate::analysis::types::AnalyzedExposeField;
use std::sync::Arc;
use verter_type_expr::PrimitiveName;

fn empty_input(macros: &[AnalyzedMacro]) -> ComponentMetaInput<'_> {
    ComponentMetaInput {
        macros,
        resolved_macros: &[],
        resolved_type_registry: &[],
        bindings: &[],
        imports: &[],
        template: None,
        options_api: None,
        analysis_flags: crate::analysis::types::AnalysisFlags::default(),
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
        canonical_source: None,
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
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        span: verter_span::Span::default(),
    }
}

/// Test helper: simulate the analyzer producer's lowering of a TS type
/// annotation. Production code calls `lower_ts_type` on an OXC `TSType<'_>`
/// AST node; this helper accepts the same source text and uses the OXC
/// text-input adapter to mirror the producer-populated `(*_expr, *_expr_scope)`
/// pairing invariant. Returns `(None, None)` when lowering produces an
/// `Unknown`, matching the producer's contract that unparseable annotations
/// leave the typed field unset.
pub(crate) fn lower_for_test(
    type_ann: Option<&str>,
) -> (
    Option<verter_type_expr::TypeExpr>,
    Option<verter_type_expr::TypeExprScope>,
) {
    let Some(text) = type_ann else {
        return (None, None);
    };
    let lowered = crate::analysis::jsdoc::parse_jsdoc_tag_type_payload(text, None);
    if matches!(lowered, verter_type_expr::TypeExpr::Unknown { .. }) {
        (None, None)
    } else {
        (
            Some(lowered),
            Some(verter_type_expr::TypeExprScope::new("test:fixture")),
        )
    }
}

fn make_prop(name: &str, type_ann: Option<&str>, optional: bool) -> AnalyzedPropField {
    let (type_expr, type_expr_scope) = lower_for_test(type_ann);
    AnalyzedPropField {
        name: name.to_string(),
        is_optional: optional,
        span: verter_span::Span::default(),
        type_annotation: type_ann.map(|s| s.to_string()),
        description: None,
        tags: Vec::new(),
        resolution_source: crate::analysis::types::TypeResolutionSource::Rust,
        resolution_error: None,
        type_expr,
        type_expr_scope,
        declared_in_macro_type_arg: false,
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
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            raw_type: None,
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
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
            .map(|meta| meta.exactness),
        Some(crate::analysis::type_expand::ExpansionExactness::ExactConcrete)
    );
}

#[test]
fn props_preserve_expansion_metadata_when_available() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("Missing"),
        false,
    )])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::named("Missing"),
            raw_type: None,
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: vec![crate::analysis::type_expand::ExpansionDiagnostic {
                reason: crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                context: "unresolved type reference 'Missing'".to_string(),
                property_name: None,
            }],
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
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
        expansion.exactness,
        crate::analysis::type_expand::ExpansionExactness::Incomplete
    );
    assert!(expansion
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.reason
            == crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference));
}

#[test]
fn evaluated_types_are_used_when_supplied() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("MyType"),
        false,
    )])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "label".to_string(),
            r#type: TypeExpr::Primitive(PrimitiveName::String),
            raw_type: None,
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
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
fn props_fall_back_to_parsed_annotation_when_no_evaluated_type() {
    let macros = vec![make_define_props(vec![make_prop(
        "label",
        Some("MyType"),
        false,
    )])];

    let result = extract_component_meta(empty_input(&macros));

    assert_eq!(result.props.len(), 1);
    assert_eq!(result.props[0].type_expr, TypeExpr::named("MyType"));
    assert_eq!(
        result.props[0].raw_type.as_deref(),
        Some("MyType"),
        "raw_type should preserve the annotation text"
    );
}

#[test]
fn define_props_eval_supplements_missing_prop_fields() {
    let macros = vec![make_define_props(Vec::new())];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "x".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "y".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
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
        crate::analysis::component_meta::ResolvedMacroInput {
            macro_index: 0,
            props: vec![make_prop("x", Some("string"), false)],
            emits: Vec::new(),
            slots: Vec::new(),
        },
        crate::analysis::component_meta::ResolvedMacroInput {
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

#[test]
fn resolved_macro_projection_merges_duplicate_prop_metadata() {
    let macros = vec![make_define_props(Vec::new())];
    let mut sparse = make_prop("as", Some("ton"), true);
    sparse.resolution_error = Some("broken expanded display".to_string());

    let mut rich = make_prop("as", Some("any"), true);
    rich.description = Some(
        "The element or component this component should render as when not a link.".to_string(),
    );
    rich.tags = vec![crate::analysis::types::JsdocTag {
        name: "defaultValue".to_string(),
        text: Some("'button'".to_string()),
    }];

    let resolved = vec![
        crate::analysis::component_meta::ResolvedMacroInput {
            macro_index: 0,
            props: vec![sparse],
            emits: Vec::new(),
            slots: Vec::new(),
        },
        crate::analysis::component_meta::ResolvedMacroInput {
            macro_index: 0,
            props: vec![rich],
            emits: Vec::new(),
            slots: Vec::new(),
        },
    ];

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved;

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "as")
        .expect("merged prop should be present");

    assert_eq!(prop.raw_type.as_deref(), Some("any"));
    assert_eq!(
        prop.description.as_deref(),
        Some("The element or component this component should render as when not a link.")
    );
    assert_eq!(prop.tags.len(), 1);
    assert_eq!(prop.tags[0].name, "defaultValue");
    assert_eq!(prop.tags[0].text.as_deref(), Some("'button'"));
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
        default_values: vec![crate::analysis::types::AnalyzedDefaultValue {
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
        default_values: vec![crate::analysis::types::AnalyzedDefaultValue {
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
            crate::analysis::types::AnalyzedEmitField {
                name: "change".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[value: string]".to_string()),
                description: None,
                tags: Vec::new(),
                payload_expr: lower_for_test(Some("[value: string]")).0,
                payload_expr_scope: lower_for_test(Some("[value: string]")).1,
            },
            crate::analysis::types::AnalyzedEmitField {
                name: "close".to_string(),
                span: verter_span::Span::default(),
                payload_type: None,
                description: None,
                tags: Vec::new(),
                payload_expr: None,
                payload_expr_scope: None,
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

#[test]
fn define_emits_eval_supplements_local_tuple_property_events() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        emit_fields: vec![crate::analysis::types::AnalyzedEmitField {
            name: "update:searchTerm".to_string(),
            span: verter_span::Span::default(),
            payload_type: Some("[value: string]".to_string()),
            description: Some("Local update event".to_string()),
            tags: Vec::new(),
            payload_expr: lower_for_test(Some("[value: string]")).0,
            payload_expr_scope: lower_for_test(Some("[value: string]")).1,
        }],
        ..make_define_props(vec![])
    }];
    let resolved_macros = vec![crate::analysis::component_meta::ResolvedMacroInput {
        macro_index: 0,
        props: Vec::new(),
        emits: vec![
            crate::analysis::types::AnalyzedEmitField {
                name: "escapeKeyDown".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[event: KeyboardEvent]".to_string()),
                description: None,
                tags: Vec::new(),
                payload_expr: lower_for_test(Some("[event: KeyboardEvent]")).0,
                payload_expr_scope: lower_for_test(Some("[event: KeyboardEvent]")).1,
            },
            crate::analysis::types::AnalyzedEmitField {
                name: "closeAutoFocus".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[event: Event]".to_string()),
                description: None,
                tags: Vec::new(),
                payload_expr: lower_for_test(Some("[event: Event]")).0,
                payload_expr_scope: lower_for_test(Some("[event: Event]")).1,
            },
        ],
        slots: Vec::new(),
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "escapeKeyDown".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![verter_type_expr::TupleElement {
                                    label: Some("event".to_string()),
                                    ty: TypeExpr::named("KeyboardEvent"),
                                    optional: false,
                                    rest: false,
                                }]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "closeAutoFocus".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![verter_type_expr::TupleElement {
                                    label: Some("event".to_string()),
                                    ty: TypeExpr::named("Event"),
                                    optional: false,
                                    rest: false,
                                }]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "update:searchTerm".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![verter_type_expr::TupleElement {
                                    label: Some("value".to_string()),
                                    ty: TypeExpr::Primitive(PrimitiveName::String),
                                    optional: false,
                                    rest: false,
                                }]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let event_names: Vec<&str> = result
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert!(
        event_names.contains(&"escapeKeyDown")
            && event_names.contains(&"closeAutoFocus")
            && event_names.contains(&"update:searchTerm"),
        "resolved inherited emits plus a local tuple-property event should survive analysis extraction: {:?}",
        event_names
    );
    let local = result
        .events
        .iter()
        .find(|event| event.name == "update:searchTerm")
        .expect("local tuple-property event should be present");
    assert_eq!(
        local.raw_signature.as_deref(),
        Some("[value: string]"),
        "local tuple-property events should preserve raw signature metadata"
    );
}

#[test]
fn define_emits_eval_does_not_resurrect_omitted_imported_events() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        ..make_define_props(vec![])
    }];
    let resolved_macros = vec![crate::analysis::component_meta::ResolvedMacroInput {
        macro_index: 0,
        props: Vec::new(),
        emits: vec![
            crate::analysis::types::AnalyzedEmitField {
                name: "escapeKeyDown".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[event: KeyboardEvent]".to_string()),
                description: None,
                tags: Vec::new(),
                payload_expr: lower_for_test(Some("[event: KeyboardEvent]")).0,
                payload_expr_scope: lower_for_test(Some("[event: KeyboardEvent]")).1,
            },
            crate::analysis::types::AnalyzedEmitField {
                name: "closeAutoFocus".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some("[]".to_string()),
                description: None,
                tags: Vec::new(),
                payload_expr: lower_for_test(Some("[]")).0,
                payload_expr_scope: lower_for_test(Some("[]")).1,
            },
        ],
        slots: Vec::new(),
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "escapeKeyDown".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![verter_type_expr::TupleElement {
                                    label: Some("event".to_string()),
                                    ty: TypeExpr::named("KeyboardEvent"),
                                    optional: false,
                                    rest: false,
                                }]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "closeAutoFocus".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "openAutoFocus".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "entryFocus".to_string(),
                            ty: TypeExpr::Tuple {
                                elements: Arc::from(vec![]),
                                readonly: false,
                            },
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let event_names: Vec<&str> = result
        .events
        .iter()
        .map(|event| event.name.as_str())
        .collect();

    assert_eq!(
        event_names,
        vec!["escapeKeyDown", "closeAutoFocus"],
        "resolved emit membership should remain authoritative when evaluated defineEmits contains omitted imported events"
    );
}

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

#[test]
fn extracts_slots_from_define_slots() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: true,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "item".to_string(),
                type_annotation: Some("string".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("string")).0,
                binding_expr_scope: lower_for_test(Some("string")).1,
            }],
            return_type: None,
            description: None,
            tags: Vec::new(),
            return_expr: None,
            return_expr_scope: None,
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

#[test]
fn define_slots_eval_extracts_bindings_from_optional_function_types() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "leading".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::Function(Arc::new(
                                verter_type_expr::FunctionExpr::synthetic(
                                    vec![verter_type_expr::FunctionParam::synthetic(
                                        Some("props".to_string()),
                                        TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                            properties: vec![
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "item".to_string(),
                                                        TypeExpr::Primitive(PrimitiveName::String),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "open".to_string(),
                                                        TypeExpr::Primitive(PrimitiveName::Boolean),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                            ],
                                        })),
                                        false,
                                        false,
                                    )],
                                    Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                    Vec::new(),
                                ),
                            )),
                            TypeExpr::Primitive(PrimitiveName::Undefined),
                        ]),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted from defineSlots eval");
    let binding_names: Vec<_> = slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(binding_names, vec!["item", "open"]);
}

/// A `defineSlots` member whose value is a bare CONSTRUCTOR type
/// (`new (props: { item: string }) => any`) must extract its first
/// parameter's object members as slot bindings — exactly as the
/// function-typed equivalent does. A constructor type carries the same
/// `FunctionExpr` payload as a function type.
///
/// Discriminator: `collect_slot_binding_param_types` ran on analyzer IR
/// (the lowered slot type, BEFORE any dispatch collapse). Pre-fix it
/// matched only `TypeExpr::Function` and fell through `_ => {}` for a
/// `ConstructorType`, so the first parameter was never collected and the
/// slot published ZERO bindings.
#[test]
fn define_slots_eval_extracts_bindings_from_constructor_type_member() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "leading".to_string(),
                        // `new (props: { item: string; open: boolean }) => any`
                        ty: TypeExpr::ConstructorType(Arc::new(
                            verter_type_expr::FunctionExpr::synthetic(
                                vec![verter_type_expr::FunctionParam::synthetic(
                                    Some("props".to_string()),
                                    TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                        properties: vec![
                                            verter_type_expr::ObjectMember::Property(
                                                verter_type_expr::ObjectProperty::synthetic_public(
                                                    "item".to_string(),
                                                    TypeExpr::Primitive(PrimitiveName::String),
                                                    false,
                                                    false,
                                                ),
                                            ),
                                            verter_type_expr::ObjectMember::Property(
                                                verter_type_expr::ObjectProperty::synthetic_public(
                                                    "open".to_string(),
                                                    TypeExpr::Primitive(PrimitiveName::Boolean),
                                                    false,
                                                    false,
                                                ),
                                            ),
                                        ],
                                    })),
                                    false,
                                    false,
                                )],
                                Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                Vec::new(),
                            ),
                        )),
                        optional: false,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "leading")
        .expect("leading slot should be extracted from a constructor-typed defineSlots member");
    let binding_names: Vec<_> = slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(
        binding_names,
        vec!["item", "open"],
        "a constructor-typed slot member must surface its first parameter's object members as \
         bindings, identically to the function-typed equivalent"
    );
}

#[test]
fn huge_partial_slot_binding_expansions_fall_back_to_symbolic_source_type() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "day".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "day".to_string(),
                type_annotation: Some("CalendarCellTriggerProps['day']".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("CalendarCellTriggerProps['day']")).0,
                binding_expr_scope: lower_for_test(Some("CalendarCellTriggerProps['day']")).1,
            }],
            return_type: Some("VNode[]".to_string()),
            description: None,
            tags: Vec::new(),
            return_expr: lower_for_test(Some("VNode[]")).0,
            return_expr_scope: lower_for_test(Some("VNode[]")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::partial(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "day".to_string(),
                        ty: TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::synthetic(vec![verter_type_expr::FunctionParam::synthetic(Some("props".to_string()), TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                    properties: vec![
                                        verter_type_expr::ObjectMember::Property(
                                            verter_type_expr::ObjectProperty::synthetic_public("day".to_string(), TypeExpr::Object(Arc::new(
                                                    verter_type_expr::ObjectExpr {
                                                        properties: (0..512)
                                                            .map(|index| {
                                                                verter_type_expr::ObjectMember::Property(
                                                                    verter_type_expr::ObjectProperty::synthetic_public(format!("field{index}"), TypeExpr::Primitive(
                                                                            PrimitiveName::String,
                                                                        ), true, false),
                                                                )
                                                            })
                                                            .collect(),
                                                    },
                                                )), false, false),
                                        ),
                                    ],
                                })), false, false)], Some(Arc::new(TypeExpr::Primitive(
                                PrimitiveName::Any,
                            ))), Vec::new()))),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                vec![crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                    context: "symbolic work limit reached".to_string(),
                    property_name: Some("day".to_string()),
                }],
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("day slot should be extracted");
    let binding = slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day binding should be extracted");

    assert_eq!(
        binding.type_expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("CalendarCellTriggerProps")),
            index: Arc::new(TypeExpr::string_literal("day")),
        },
        "huge partial slot binding expansions should keep the symbolic source contract"
    );
    assert_eq!(
        binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']")
    );
}

#[test]
fn small_partial_helper_slot_binding_expansions_fall_back_to_symbolic_indexed_access() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "ui".to_string(),
                type_annotation: Some("Button['ui']".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("Button['ui']")).0,
                binding_expr_scope: lower_for_test(Some("Button['ui']")).1,
            }],
            return_type: Some("any".to_string()),
            description: None,
            tags: Vec::new(),
            return_expr: lower_for_test(Some("any")).0,
            return_expr_scope: lower_for_test(Some("any")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: vec![crate::analysis::type_expand::ExpandedField {
            name: "default.ui".to_string(),
            r#type: TypeExpr::Ref {
                name: Arc::from("ComponentUI"),
                type_arguments: Arc::from(vec![TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path: vec!["theme".to_string()],
                })]),
            },
            raw_type: Some("Button['ui']".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: vec![
                crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                    context: "unresolved type reference 'ComponentUI'".to_string(),
                    property_name: Some("default.ui".to_string()),
                },
                crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
                    context: "typeof theme was preserved symbolically".to_string(),
                    property_name: Some("default.ui".to_string()),
                },
            ],
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let binding = slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("ui binding should be extracted");

    assert_eq!(
        binding.type_expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Button")),
            index: Arc::new(TypeExpr::string_literal("ui")),
        },
        "partial helper slot bindings should keep the symbolic indexed-access source contract"
    );
    assert_eq!(binding.raw_type.as_deref(), Some("Button['ui']"));
}

#[test]
fn define_slots_prefer_concrete_evaluated_slot_bindings_over_symbolic_direct_bindings() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "ui".to_string(),
                type_annotation: Some("Button['ui']".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("Button['ui']")).0,
                binding_expr_scope: lower_for_test(Some("Button['ui']")).1,
            }],
            return_type: Some("any".to_string()),
            description: None,
            tags: Vec::new(),
            return_expr: lower_for_test(Some("any")).0,
            return_expr_scope: lower_for_test(Some("any")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "default".to_string(),
                        ty: TypeExpr::Function(Arc::new(
                            verter_type_expr::FunctionExpr::synthetic(
                                vec![verter_type_expr::FunctionParam::synthetic(
                                    Some("props".to_string()),
                                    TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                        properties: vec![verter_type_expr::ObjectMember::Property(
                                            verter_type_expr::ObjectProperty::synthetic_public(
                                                "ui".to_string(),
                                                TypeExpr::Ref {
                                                    name: Arc::from("ComponentUI"),
                                                    type_arguments: Arc::from(vec![
                                                        TypeExpr::TypeOf(
                                                            verter_type_expr::ValueRef {
                                                                path: vec!["theme".to_string()],
                                                            },
                                                        ),
                                                    ]),
                                                },
                                                false,
                                                false,
                                            ),
                                        )],
                                    })),
                                    false,
                                    false,
                                )],
                                Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                Vec::new(),
                            ),
                        )),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: vec![crate::analysis::type_expand::ExpandedField {
            name: "default.ui".to_string(),
            r#type: TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                properties: vec![verter_type_expr::ObjectMember::Property(
                    verter_type_expr::ObjectProperty::synthetic_public(
                        "base".to_string(),
                        TypeExpr::Primitive(PrimitiveName::String),
                        false,
                        false,
                    ),
                )],
            })),
            raw_type: Some("Button['ui']".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let binding = slot
        .bindings
        .iter()
        .find(|binding| binding.name == "ui")
        .expect("ui binding should be extracted");

    let TypeExpr::Object(object) = &binding.type_expr else {
        panic!(
            "rescued evaluated slot binding should materialize to an object, got {:?}",
            binding.type_expr
        );
    };
    assert!(
        object.properties.iter().any(|member| {
            matches!(
                member,
                verter_type_expr::ObjectMember::Property(property)
                    if property.name == "base"
            )
        }),
        "rescued evaluated slot binding should expose concrete members, got {:?}",
        binding.type_expr
    );
    assert_eq!(binding.raw_type.as_deref(), Some("Button['ui']"));
}

#[test]
fn define_slots_keep_source_bindings_when_expanded_slot_bindings_are_empty() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "day".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "day".to_string(),
                type_annotation: Some("CalendarCellTriggerProps['day']".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("CalendarCellTriggerProps['day']")).0,
                binding_expr_scope: lower_for_test(Some("CalendarCellTriggerProps['day']")).1,
            }],
            return_type: Some("any".to_string()),
            description: None,
            tags: Vec::new(),
            return_expr: lower_for_test(Some("any")).0,
            return_expr_scope: lower_for_test(Some("any")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "day".to_string(),
                        ty: TypeExpr::Function(Arc::new(
                            verter_type_expr::FunctionExpr::synthetic(
                                vec![verter_type_expr::FunctionParam::synthetic(
                                    Some("props".to_string()),
                                    TypeExpr::IndexedAccess {
                                        object: Arc::new(TypeExpr::named("CalendarSlotProps")),
                                        index: Arc::new(TypeExpr::string_literal("day")),
                                    },
                                    false,
                                    false,
                                )],
                                Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                Vec::new(),
                            ),
                        )),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "day")
        .expect("day slot should be extracted");
    let binding = slot
        .bindings
        .iter()
        .find(|binding| binding.name == "day")
        .expect("day binding should fall back to the source slot binding");

    assert_eq!(
        binding.type_expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("CalendarCellTriggerProps")),
            index: Arc::new(TypeExpr::string_literal("day")),
        }
    );
    assert_eq!(
        binding.raw_type.as_deref(),
        Some("CalendarCellTriggerProps['day']")
    );
}

#[test]
fn define_slots_extract_bindings_from_call_signature_object_types() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "content".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: Vec::new(),
            return_type: Some("any".to_string()),
            description: None,
            tags: Vec::new(),
            return_expr: lower_for_test(Some("any")).0,
            return_expr_scope: lower_for_test(Some("any")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "content".to_string(),
                        ty: TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                            properties: vec![verter_type_expr::ObjectMember::CallSignature(
                                verter_type_expr::FunctionExpr::synthetic(
                                    vec![verter_type_expr::FunctionParam::synthetic(
                                        Some("props".to_string()),
                                        TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                            properties: vec![
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "item".to_string(),
                                                        TypeExpr::named("T"),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "index".to_string(),
                                                        TypeExpr::Primitive(PrimitiveName::Number),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "ui".to_string(),
                                                        TypeExpr::IndexedAccess {
                                                            object: Arc::new(TypeExpr::named(
                                                                "Tabs",
                                                            )),
                                                            index: Arc::new(
                                                                TypeExpr::string_literal("ui"),
                                                            ),
                                                        },
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                            ],
                                        })),
                                        false,
                                        false,
                                    )],
                                    Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                    Vec::new(),
                                ),
                            )],
                        })),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "content")
        .expect("content slot should be extracted");
    let binding_names: Vec<_> = slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(binding_names, vec!["item", "index", "ui"]);
}

#[test]
fn source_prop_raw_type_beats_expanded_backend_display_when_it_preserves_macro_contract() {
    let macros = vec![make_define_props(vec![make_prop(
        "ui",
        Some("Accordion['slots']"),
        true,
    )])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "ui".to_string(),
            r#type: TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ]),
            raw_type: Some("{ root?: string } | undefined".to_string()),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "ui".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::Primitive(PrimitiveName::String),
                            TypeExpr::Primitive(PrimitiveName::Undefined),
                        ]),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should be extracted");

    assert_eq!(prop.raw_type.as_deref(), Some("Accordion['slots']"));
}

#[test]
fn optional_prop_raw_type_prefers_source_annotation_without_adding_undefined() {
    let macros = vec![make_define_props(vec![make_prop(
        "modelValue",
        Some("string | string[]"),
        true,
    )])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "modelValue".to_string(),
            r#type: TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Array {
                    element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                    readonly: false,
                },
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ]),
            raw_type: Some("string | string[] | undefined".to_string()),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "modelValue".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::Primitive(PrimitiveName::String),
                            TypeExpr::Array {
                                element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                                readonly: false,
                            },
                            TypeExpr::Primitive(PrimitiveName::Undefined),
                        ]),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "modelValue")
        .expect("modelValue prop should be extracted");

    assert_eq!(prop.raw_type.as_deref(), Some("string | string[]"));
}

#[test]
fn placeholder_evaluated_prop_raw_type_falls_back_to_meaningful_source_annotation() {
    let macros = vec![make_define_props(vec![
        make_prop("labelKey", Some("GetItemKeys<T>"), true),
        make_prop("trailingIcon", Some("IconProps['name']"), true),
    ])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            crate::analysis::type_expand::ExpandedField {
                name: "labelKey".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::Any),
                raw_type: Some("any".to_string()),
                optional: true,
                exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
            crate::analysis::type_expand::ExpandedField {
                name: "trailingIcon".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::Any),
                raw_type: Some("any".to_string()),
                optional: true,
                exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
        ],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "labelKey".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Any),
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "trailingIcon".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Any),
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let label_key = result
        .props
        .iter()
        .find(|prop| prop.name == "labelKey")
        .expect("labelKey prop should be extracted");
    let trailing_icon = result
        .props
        .iter()
        .find(|prop| prop.name == "trailingIcon")
        .expect("trailingIcon prop should be extracted");

    assert_eq!(label_key.raw_type.as_deref(), Some("GetItemKeys<T>"));
    assert_eq!(trailing_icon.raw_type.as_deref(), Some("IconProps['name']"));
}

#[test]
fn small_partial_placeholder_prop_expansions_fall_back_to_symbolic_source_type() {
    let macros = vec![make_define_props(vec![
        make_prop("to", Some("RouteLocationRaw"), true),
        make_prop("href", Some("NuxtLinkProps['to']"), true),
    ])];
    let placeholder = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: Vec::new(),
    }));
    let diagnostics = vec![crate::analysis::type_expand::ExpansionDiagnostic {
        reason: crate::analysis::type_expand::ExpansionStopReason::IndeterminateConditional,
        context: "conditional type could not be resolved".to_string(),
        property_name: Some("to".to_string()),
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            crate::analysis::type_expand::ExpandedField {
                name: "to".to_string(),
                r#type: placeholder.clone(),
                raw_type: Some("RouteLocationRaw".to_string()),
                optional: true,
                exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: diagnostics.clone(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
            crate::analysis::type_expand::ExpandedField {
                name: "href".to_string(),
                r#type: placeholder.clone(),
                raw_type: Some("NuxtLinkProps['to']".to_string()),
                optional: true,
                exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: vec![crate::analysis::type_expand::ExpansionDiagnostic {
                    reason:
                        crate::analysis::type_expand::ExpansionStopReason::IndeterminateConditional,
                    context: "conditional type could not be resolved".to_string(),
                    property_name: Some("href".to_string()),
                }],
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
        ],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::partial(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "to".to_string(),
                            ty: placeholder.clone(),
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "href".to_string(),
                            ty: placeholder,
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                diagnostics,
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let to = result
        .props
        .iter()
        .find(|prop| prop.name == "to")
        .expect("to prop should be extracted");
    let href = result
        .props
        .iter()
        .find(|prop| prop.name == "href")
        .expect("href prop should be extracted");

    assert_eq!(
        to.type_expr,
        TypeExpr::named("RouteLocationRaw"),
        "small partial placeholder props should keep the symbolic source contract",
    );
    assert_eq!(
        href.type_expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("NuxtLinkProps")),
            index: Arc::new(TypeExpr::string_literal("to")),
        },
        "small partial indexed-access props should keep the symbolic source contract",
    );
    assert_eq!(to.raw_type.as_deref(), Some("RouteLocationRaw"));
    assert_eq!(href.raw_type.as_deref(), Some("NuxtLinkProps['to']"));
}

#[test]
fn suspicious_partial_identifier_props_fall_back_to_source_any() {
    let macros = vec![make_define_props(Vec::new())];
    let mut imported = make_prop("as", Some("any"), true);
    imported.description = Some(
        "The element or component this component should render as when not a link.".to_string(),
    );
    imported.tags = vec![crate::analysis::types::JsdocTag {
        name: "defaultValue".to_string(),
        text: Some("'button'".to_string()),
    }];
    let resolved_macros = vec![ResolvedMacroInput {
        macro_index: 0,
        props: vec![imported],
        emits: Vec::new(),
        slots: Vec::new(),
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "as".to_string(),
            r#type: TypeExpr::named("ton"),
            raw_type: Some("ton".to_string()),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::partial(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "as".to_string(),
                        ty: TypeExpr::named("ton"),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                Vec::new(),
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "as")
        .expect("as prop should be extracted");

    assert_eq!(
        prop.type_expr,
        TypeExpr::Primitive(PrimitiveName::Any),
        "partial suspicious identifiers should fall back to the richer source annotation",
    );
    assert_eq!(prop.raw_type.as_deref(), Some("any"));
    assert_eq!(prop.tags.len(), 1);
}

#[test]
fn small_partial_undefined_object_props_fall_back_to_symbolic_source_type() {
    let macros = vec![make_define_props(vec![make_prop(
        "ui",
        Some("Button['slots']"),
        true,
    )])];
    let degraded = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: vec![
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "base".to_string(),
                    TypeExpr::Primitive(PrimitiveName::Undefined),
                    false,
                    false,
                ),
            ),
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    "label".to_string(),
                    TypeExpr::Primitive(PrimitiveName::Undefined),
                    false,
                    false,
                ),
            ),
        ],
    }));
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "ui".to_string(),
            r#type: degraded.clone(),
            raw_type: Some("Button['slots']".to_string()),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: vec![crate::analysis::type_expand::ExpansionDiagnostic {
                reason: crate::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
                context: "indexed access was preserved symbolically".to_string(),
                property_name: Some("ui".to_string()),
            }],
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::partial(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "ui".to_string(),
                        ty: degraded,
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                vec![crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
                    context: "indexed access was preserved symbolically".to_string(),
                    property_name: Some("ui".to_string()),
                }],
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "ui")
        .expect("ui prop should be extracted");

    assert_eq!(
        prop.type_expr,
        TypeExpr::IndexedAccess {
            object: Arc::new(TypeExpr::named("Button")),
            index: Arc::new(TypeExpr::string_literal("slots")),
        },
        "partial degraded objects should keep the symbolic indexed-access contract",
    );
    assert_eq!(prop.raw_type.as_deref(), Some("Button['slots']"));
}

#[test]
fn huge_partial_prop_expansions_fall_back_to_symbolic_source_type() {
    let macros = vec![make_define_props(vec![make_prop(
        "mention",
        Some("boolean | Partial<Omit<MentionOptions, 'suggestion' | 'suggestions'>>"),
        true,
    )])];
    let huge_members = (0..512)
        .map(|index| {
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public(
                    format!("field{index}"),
                    TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                        properties: vec![
                            verter_type_expr::ObjectMember::Property(
                                verter_type_expr::ObjectProperty::synthetic_public(
                                    "enabled".to_string(),
                                    TypeExpr::Primitive(PrimitiveName::Boolean),
                                    false,
                                    false,
                                ),
                            ),
                            verter_type_expr::ObjectMember::Property(
                                verter_type_expr::ObjectProperty::synthetic_public(
                                    "label".to_string(),
                                    TypeExpr::Primitive(PrimitiveName::String),
                                    true,
                                    false,
                                ),
                            ),
                        ],
                    })),
                    true,
                    false,
                ),
            )
        })
        .collect();
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "mention".to_string(),
            r#type: TypeExpr::Union(Arc::from(vec![
                TypeExpr::Primitive(PrimitiveName::Boolean),
                TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                    properties: huge_members,
                })),
            ])),
            raw_type: Some(
                "boolean | Partial<Omit<MentionOptions, 'suggestion' | 'suggestions'>>".to_string(),
            ),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: vec![
                crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                    context: "symbolic work limit reached".to_string(),
                    property_name: Some("mention".to_string()),
                },
                crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                    context: "unresolved type reference 'MentionOptions'".to_string(),
                    property_name: Some("mention".to_string()),
                },
            ],
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::partial(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "mention".to_string(),
                        ty: TypeExpr::Union(Arc::from(vec![
                            TypeExpr::Primitive(PrimitiveName::Boolean),
                            TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                properties: (0..512)
                                    .map(|index| {
                                        verter_type_expr::ObjectMember::Property(
                                            verter_type_expr::ObjectProperty::synthetic_public(
                                                format!("field{index}"),
                                                TypeExpr::Primitive(PrimitiveName::String),
                                                true,
                                                false,
                                            ),
                                        )
                                    })
                                    .collect(),
                            })),
                        ])),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                vec![crate::analysis::type_expand::ExpansionDiagnostic {
                    reason: crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                    context: "symbolic work limit reached".to_string(),
                    property_name: Some("mention".to_string()),
                }],
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let prop = result
        .props
        .iter()
        .find(|prop| prop.name == "mention")
        .expect("mention prop should be extracted");

    assert_eq!(
        prop.type_expr,
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::Boolean),
            TypeExpr::named_with_args(
                "Partial",
                vec![TypeExpr::named_with_args(
                    "Omit",
                    vec![
                        TypeExpr::named("MentionOptions"),
                        TypeExpr::union(vec![
                            TypeExpr::string_literal("suggestion"),
                            TypeExpr::string_literal("suggestions"),
                        ]),
                    ],
                )],
            ),
        ]),
        "huge partial evaluated prop expansions should keep the symbolic source contract"
    );
    assert_eq!(
        prop.raw_type.as_deref(),
        Some("boolean | Partial<Omit<MentionOptions, 'suggestion' | 'suggestions'>>")
    );
}

#[test]
fn source_event_raw_signature_beats_backend_when_backend_widens_macro_payload() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        emit_fields: vec![crate::analysis::types::AnalyzedEmitField {
            name: "update:modelValue".to_string(),
            span: verter_span::Span::default(),
            payload_type: Some(
                "[value: (T extends 'single' ? string : string[]) | undefined]".to_string(),
            ),
            description: None,
            tags: Vec::new(),
            payload_expr: None,
            payload_expr_scope: None,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: vec![crate::analysis::type_expand::ExpandedField {
            name: "update:modelValue".to_string(),
            r#type: TypeExpr::Tuple {
                elements: Arc::from(vec![verter_type_expr::TupleElement {
                    label: Some("value".to_string()),
                    ty: TypeExpr::union(vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::Primitive(PrimitiveName::Undefined),
                    ]),
                    optional: false,
                    rest: false,
                }]),
                readonly: false,
            },
            raw_type: Some("string | undefined".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let event = result
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue should be extracted");

    assert_eq!(
        event.raw_signature.as_deref(),
        Some("[value: (T extends 'single' ? string : string[]) | undefined]")
    );
}

#[test]
fn source_backed_update_events_keep_their_raw_emit_payloads() {
    let macros = vec![
        make_define_props(vec![make_prop(
            "modelValue",
            Some("string | string[]"),
            true,
        )]),
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineEmits,
            emit_fields: vec![crate::analysis::types::AnalyzedEmitField {
                name: "update:modelValue".to_string(),
                span: verter_span::Span::default(),
                payload_type: Some(
                    "[value: (T extends 'single' ? string : string[]) | undefined]".to_string(),
                ),
                description: None,
                tags: Vec::new(),
                payload_expr: None,
                payload_expr_scope: None,
            }],
            ..make_define_props(vec![])
        },
    ];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![crate::analysis::type_expand::ExpandedField {
            name: "modelValue".to_string(),
            r#type: TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Array {
                    element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                    readonly: false,
                },
                TypeExpr::Primitive(PrimitiveName::Undefined),
            ]),
            raw_type: Some("string | string[] | undefined".to_string()),
            optional: true,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "modelValue".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::Primitive(PrimitiveName::String),
                            TypeExpr::Array {
                                element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                                readonly: false,
                            },
                            TypeExpr::Primitive(PrimitiveName::Undefined),
                        ]),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: vec![crate::analysis::type_expand::ExpandedField {
            name: "update:modelValue".to_string(),
            r#type: TypeExpr::Tuple {
                elements: Arc::from(vec![verter_type_expr::TupleElement {
                    label: Some("value".to_string()),
                    ty: TypeExpr::union(vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::Array {
                            element: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
                            readonly: false,
                        },
                        TypeExpr::Primitive(PrimitiveName::Undefined),
                    ]),
                    optional: false,
                    rest: false,
                }]),
                readonly: false,
            },
            raw_type: Some("(T extends 'single' ? string : string[]) | undefined".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let event = result
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue should be extracted");

    assert_eq!(
        event.raw_signature.as_deref(),
        Some("[value: (T extends 'single' ? string : string[]) | undefined]")
    );
}

#[test]
fn evaluated_tuple_event_raw_type_is_not_double_wrapped() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        emit_fields: vec![crate::analysis::types::AnalyzedEmitField {
            name: "update:modelValue".to_string(),
            span: verter_span::Span::default(),
            payload_type: Some("[date: CalendarModelValue<R, M>]".to_string()),
            description: None,
            tags: Vec::new(),
            payload_expr: lower_for_test(Some("[date: CalendarModelValue<R, M>]")).0,
            payload_expr_scope: lower_for_test(Some("[date: CalendarModelValue<R, M>]")).1,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: vec![crate::analysis::type_expand::ExpandedField {
            name: "update:modelValue".to_string(),
            r#type: TypeExpr::Tuple {
                elements: Arc::from(vec![verter_type_expr::TupleElement {
                    label: Some("date".to_string()),
                    ty: TypeExpr::named_with_args(
                        "CalendarModelValue",
                        vec![TypeExpr::named("R"), TypeExpr::named("M")],
                    ),
                    optional: false,
                    rest: false,
                }]),
                readonly: false,
            },
            raw_type: Some("[date: CalendarModelValue<R, M>]".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let event = result
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue should be extracted");

    assert_eq!(
        event.raw_signature.as_deref(),
        Some("[date: CalendarModelValue<R, M>]"),
        "evaluated tuple payload displays should be preserved as-is"
    );
}

#[test]
fn expanded_slot_bindings_preserve_source_binding_order() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![
                crate::analysis::types::AnalyzedSlotFieldBinding {
                    name: "item".to_string(),
                    type_annotation: Some("T".to_string()),
                    span: verter_span::Span::default(),
                    binding_expr: lower_for_test(Some("T")).0,
                    binding_expr_scope: lower_for_test(Some("T")).1,
                },
                crate::analysis::types::AnalyzedSlotFieldBinding {
                    name: "index".to_string(),
                    type_annotation: Some("number".to_string()),
                    span: verter_span::Span::default(),
                    binding_expr: lower_for_test(Some("number")).0,
                    binding_expr_scope: lower_for_test(Some("number")).1,
                },
                crate::analysis::types::AnalyzedSlotFieldBinding {
                    name: "open".to_string(),
                    type_annotation: Some("boolean".to_string()),
                    span: verter_span::Span::default(),
                    binding_expr: lower_for_test(Some("boolean")).0,
                    binding_expr_scope: lower_for_test(Some("boolean")).1,
                },
            ],
            return_type: None,
            description: None,
            tags: Vec::new(),
            return_expr: None,
            return_expr_scope: None,
        }],
        ..make_define_props(vec![])
    }];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "default".to_string(),
                        ty: TypeExpr::union(vec![
                            TypeExpr::Function(Arc::new(
                                verter_type_expr::FunctionExpr::synthetic(
                                    vec![verter_type_expr::FunctionParam::synthetic(
                                        Some("props".to_string()),
                                        TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                                            properties: vec![
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "index".to_string(),
                                                        TypeExpr::Primitive(PrimitiveName::Number),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "item".to_string(),
                                                        TypeExpr::named("T"),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                                verter_type_expr::ObjectMember::Property(
                                                    verter_type_expr::ObjectProperty::synthetic_public(
                                                        "open".to_string(),
                                                        TypeExpr::Primitive(PrimitiveName::Boolean),
                                                        false,
                                                        false,
                                                    ),
                                                ),
                                            ],
                                        })),
                                        false,
                                        false,
                                    )],
                                    Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                                    Vec::new(),
                                ),
                            )),
                            TypeExpr::Primitive(PrimitiveName::Undefined),
                        ]),
                        optional: true,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should be extracted");
    let binding_names: Vec<_> = slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();

    assert_eq!(binding_names, vec!["item", "index", "open"]);
}

#[test]
fn resolved_slots_merge_local_details_and_append_new_slots() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineSlots,
        slot_fields: vec![crate::analysis::types::AnalyzedSlotField {
            name: "default".to_string(),
            is_required: false,
            span: verter_span::Span::default(),
            bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                name: "item".to_string(),
                type_annotation: Some("string".to_string()),
                span: verter_span::Span::default(),
                binding_expr: lower_for_test(Some("string")).0,
                binding_expr_scope: lower_for_test(Some("string")).1,
            }],
            return_type: None,
            description: None,
            tags: Vec::new(),
            return_expr: None,
            return_expr_scope: None,
        }],
        ..make_define_props(vec![])
    }];
    let resolved_macros = vec![ResolvedMacroInput {
        macro_index: 0,
        props: Vec::new(),
        emits: Vec::new(),
        slots: vec![
            crate::analysis::types::AnalyzedSlotField {
                name: "default".to_string(),
                is_required: true,
                span: verter_span::Span::default(),
                bindings: vec![crate::analysis::types::AnalyzedSlotFieldBinding {
                    name: "row".to_string(),
                    type_annotation: Some("number".to_string()),
                    span: verter_span::Span::default(),
                    binding_expr: lower_for_test(Some("number")).0,
                    binding_expr_scope: lower_for_test(Some("number")).1,
                }],
                return_type: Some("VNode[]".to_string()),
                description: Some("resolved default slot".to_string()),
                tags: Vec::new(),
                return_expr: lower_for_test(Some("VNode[]")).0,
                return_expr_scope: lower_for_test(Some("VNode[]")).1,
            },
            crate::analysis::types::AnalyzedSlotField {
                name: "header".to_string(),
                is_required: false,
                span: verter_span::Span::default(),
                bindings: Vec::new(),
                return_type: Some("any".to_string()),
                description: Some("resolved header slot".to_string()),
                tags: Vec::new(),
                return_expr: lower_for_test(Some("any")).0,
                return_expr_scope: lower_for_test(Some("any")).1,
            },
        ],
    }];

    let mut input = empty_input(&macros);
    input.resolved_macros = &resolved_macros;

    let result = extract_component_meta(input);
    let slot_names: Vec<&str> = result.slots.iter().map(|slot| slot.name.as_str()).collect();
    let default_slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "default")
        .expect("default slot should exist");

    assert!(
        slot_names.contains(&"default") && slot_names.contains(&"header"),
        "resolved-only slots should be appended, got: {slot_names:?}"
    );
    assert!(
        default_slot.is_required,
        "resolved metadata should upgrade required status"
    );
    assert_eq!(
        default_slot.description.as_deref(),
        Some("resolved default slot"),
        "resolved descriptions should fill missing local docs"
    );
    assert_eq!(
        default_slot.return_type.as_deref(),
        Some("VNode[]"),
        "resolved return types should survive slot merging"
    );
    let binding_names: Vec<&str> = default_slot
        .bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect();
    assert_eq!(
        binding_names,
        vec!["item", "row"],
        "resolved bindings should merge without dropping local bindings"
    );
    let header_slot = result
        .slots
        .iter()
        .find(|slot| slot.name == "header")
        .expect("header slot should exist");
    assert_eq!(
        header_slot.return_type.as_deref(),
        Some("any"),
        "resolved-only slots should preserve return types"
    );
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
fn define_model_without_default_stays_optional_in_component_meta() {
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineModel,
        model_name: None, // default model → "modelValue"
        prop_fields: vec![make_prop("modelValue", Some("string"), true)],
        ..make_define_props(vec![])
    }];

    let result = extract_component_meta(empty_input(&macros));
    let model_prop = result
        .props
        .iter()
        .find(|prop| prop.name == "modelValue")
        .expect("defineModel should synthesize a model prop");
    let model_event = result
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("defineModel should synthesize an update event");

    assert!(
        !model_prop.required,
        "defineModel without required/default should keep modelValue optional"
    );
    assert_eq!(
        model_prop.raw_type.as_deref(),
        Some("string"),
        "defineModel should preserve the declared prop raw type"
    );
    assert_eq!(
        model_event.raw_signature.as_deref(),
        Some("[value: string | undefined]"),
        "defineModel should keep the update event raw signature aligned with the declared model payload"
    );
}

#[test]
fn define_model_reconciles_existing_model_value_prop_from_define_props() {
    let macros = vec![
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineProps,
            ..make_define_props(vec![])
        },
        AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineModel,
            model_name: None,
            prop_fields: vec![make_prop("modelValue", Some("string"), true)],
            ..make_define_props(vec![])
        },
    ];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            crate::analysis::type_expand::ExpandedField {
                name: "modelValue".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::String),
                raw_type: Some("string".to_string()),
                optional: false,
                exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
            crate::analysis::type_expand::ExpandedField {
                name: "label".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::String),
                raw_type: Some("string".to_string()),
                optional: false,
                exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
        ],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact_symbolic(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "label".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "modelValue".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        define_emits: Vec::new(),
        emits: vec![crate::analysis::type_expand::ExpandedField {
            name: "update:modelValue".to_string(),
            r#type: TypeExpr::Tuple {
                elements: Arc::from(vec![verter_type_expr::TupleElement {
                    label: Some("value".to_string()),
                    ty: TypeExpr::Primitive(PrimitiveName::String),
                    optional: false,
                    rest: false,
                }]),
                readonly: false,
            },
            raw_type: Some("[value: string]".to_string()),
            optional: false,
            exactness: crate::analysis::type_expand::ExpansionExactness::ExactConcrete,
            execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
            diagnostics: Vec::new(),
            shallow_type_expr: None,
            shallow_type_expr_scope: None,
            declared_in_macro_type_arg: false,
        }],
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);
    let model_prop = result
        .props
        .iter()
        .find(|prop| prop.name == "modelValue")
        .expect("modelValue prop should be present");
    let model_event = result
        .events
        .iter()
        .find(|event| event.name == "update:modelValue")
        .expect("update:modelValue event should be present");

    assert!(
        !model_prop.required,
        "defineModel should reconcile an existing modelValue prop back to optional"
    );
    assert_eq!(
        model_prop.raw_type.as_deref(),
        Some("string"),
        "defineModel should keep the symbolic model raw type on the reconciled prop"
    );
    assert_eq!(
        model_event.raw_signature.as_deref(),
        Some("[value: string | undefined]"),
        "defineModel should keep the reconciled update event signature"
    );
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
        props: vec![crate::analysis::types::AnalyzedOptionsProp {
            name: "color".to_string(),
            type_constructor: Some("String".to_string()),
            is_required: false,
            has_default: true,
            default_value: None,
            type_annotation: None,
            type_expr: None,
            type_expr_scope: None,
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
        analysis_flags: crate::analysis::types::AnalysisFlags::HAS_OPTIONS_API,
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
        canonical_source: None,
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
        props: vec![crate::analysis::types::AnalyzedOptionsProp {
            name: "canvas".to_string(),
            type_constructor: Some("Object".to_string()),
            is_required: true,
            has_default: false,
            default_value: None,
            type_annotation: Some("HTMLCanvasElement".to_string()),
            // Post-M5 fix: the Options API analyzer lowers PropType<T> AST
            // nodes directly via `verter_type_expr_oxc::lower_ts_type` at
            // the producer site. The fixture mints the typed sidecar here
            // to mirror the analyzer-producer output.
            type_expr: Some(TypeExpr::named("HTMLCanvasElement")),
            type_expr_scope: Some(verter_type_expr::TypeExprScope::new("")),
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
        analysis_flags: crate::analysis::types::AnalysisFlags::HAS_OPTIONS_API,
        styles: &[],
        vue_api_calls: &[],
        store_usages: &[],
        evaluated_types: None,
        file_path: "/App.vue",
        canonical_source: None,
    };

    let result = extract_component_meta(input);

    assert_eq!(result.props.len(), 1);
    // Post-M5 fix: the Options API path now surfaces the typed `Ref` shape
    // because `extract_options_props` lowers `PropType<T>` AST nodes via
    // `lower_ts_type` directly at the analyzer-producer site. The published
    // `PropAnalysis.type_expr` is the structured `Ref { name: "HTMLCanvasElement" }`
    // — NOT the previous `Unknown { raw: "..." }` fallback that masked the
    // producer-chain gap.
    assert_eq!(
        result.props[0].type_expr,
        TypeExpr::named("HTMLCanvasElement"),
        "Options API PropType<T> must surface as the typed Ref form, NOT Unknown {{ raw }} — \
         pre-M5 fix the analyzer didn't lower PropType<T> AST nodes",
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
    input.analysis_flags = crate::analysis::types::AnalysisFlags::ASYNC_SETUP
        | crate::analysis::types::AnalysisFlags::HAS_REACTIVE_STATE
        | crate::analysis::types::AnalysisFlags::HAS_COMPUTED
        | crate::analysis::types::AnalysisFlags::HAS_WATCHERS
        | crate::analysis::types::AnalysisFlags::HAS_LIFECYCLE_HOOKS
        | crate::analysis::types::AnalysisFlags::HAS_PROVIDE
        | crate::analysis::types::AnalysisFlags::HAS_INJECT;

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
    let store_usage = crate::analysis::types::StoreUsage {
        binding_name: "userStore".to_string(),
        callee: "useUserStore".to_string(),
        import_source: "@/stores/user".to_string(),
        store_api: crate::analysis::types::StoreApiClassification::StoreComposable,
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

use crate::analysis::template::{
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
    use crate::analysis::template::VForDirective;
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
        bindings: vec![],
        events: vec![],
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
                    assert_eq!(&**name, "MyComp");
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
        props: vec![crate::analysis::template::TemplatePropUsage {
            name: "is".to_string(),
            is_bound: true,
            constness: crate::analysis::template::PropValueConstness::Dynamic,
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
        bindings: vec![],
        events: vec![],
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
        bindings: vec![],
        events: vec![],
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

// ---------------------------------------------------------------------------
// Diagnostic dedup: macro_expansion_diagnostics vs per-field diagnostics
// ---------------------------------------------------------------------------

#[test]
fn macro_wide_diagnostics_split_from_per_field_diagnostics() {
    // Setup: defineProps<{ foo: string; bar: number }> with 3 diagnostics:
    //  - one global (property_name=None, BudgetExceeded)
    //  - one for "foo" (UnresolvedReference)
    //  - one for "bar" (UnsupportedOperator)
    let macros = vec![make_define_props(vec![
        make_prop("foo", Some("string"), false),
        make_prop("bar", Some("number"), false),
    ])];

    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: vec![
            crate::analysis::type_expand::ExpandedField {
                name: "foo".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::String),
                raw_type: None,
                optional: false,
                exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
            crate::analysis::type_expand::ExpandedField {
                name: "bar".to_string(),
                r#type: TypeExpr::Primitive(PrimitiveName::Number),
                raw_type: None,
                optional: false,
                exactness: crate::analysis::type_expand::ExpansionExactness::Incomplete,
                execution_status: crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                diagnostics: Vec::new(),
                shallow_type_expr: None,
                shallow_type_expr_scope: None,
                declared_in_macro_type_arg: false,
            },
        ],
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::incomplete(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "foo".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "bar".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
                crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                vec![
                    // Global diagnostic (no property_name)
                    crate::analysis::type_expand::ExpansionDiagnostic {
                        reason: crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                        context: "global budget exceeded".to_string(),
                        property_name: None,
                    },
                    // Per-field diagnostic for "foo"
                    crate::analysis::type_expand::ExpansionDiagnostic {
                        reason:
                            crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                        context: "unresolved Foo".to_string(),
                        property_name: Some("foo".to_string()),
                    },
                    // Per-field diagnostic for "bar"
                    crate::analysis::type_expand::ExpansionDiagnostic {
                        reason:
                            crate::analysis::type_expand::ExpansionStopReason::UnsupportedOperator,
                        context: "unsupported op in bar".to_string(),
                        property_name: Some("bar".to_string()),
                    },
                ],
            ),
        }],
        define_emits: Vec::new(),
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    // --- macro_expansion_diagnostics ---
    assert_eq!(
        result.macro_expansion_diagnostics.len(),
        1,
        "should have exactly one macro-level diagnostic entry (for defineProps at index 0)"
    );
    let macro_diag = &result.macro_expansion_diagnostics[0];
    assert_eq!(macro_diag.macro_kind, MacroExpansionKind::DefineProps);
    assert_eq!(macro_diag.macro_index, 0);
    assert_eq!(
        macro_diag.diagnostics.len(),
        1,
        "macro-level entry should contain only the global diagnostic (property_name=None)"
    );
    assert_eq!(
        macro_diag.diagnostics[0].reason,
        crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded
    );
    assert!(
        macro_diag.diagnostics[0].property_name.is_none(),
        "macro-level diagnostic must have property_name=None"
    );

    // --- per-field diagnostics for "foo" ---
    let foo_prop = result
        .props
        .iter()
        .find(|p| p.name == "foo")
        .expect("foo prop should exist");
    let foo_expansion = foo_prop
        .type_expansion
        .as_ref()
        .expect("foo should have expansion metadata");
    assert_eq!(
        foo_expansion.diagnostics.len(),
        1,
        "foo should have exactly 1 per-field diagnostic"
    );
    assert_eq!(
        foo_expansion.diagnostics[0].reason,
        crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference
    );
    assert_eq!(
        foo_expansion.diagnostics[0].property_name.as_deref(),
        Some("foo")
    );
    // Negative: foo must NOT contain the global BudgetExceeded diagnostic
    assert!(
        !foo_expansion
            .diagnostics
            .iter()
            .any(|d| d.reason == crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded),
        "per-field diagnostics for foo must NOT contain the global BudgetExceeded diagnostic"
    );

    // --- per-field diagnostics for "bar" ---
    let bar_prop = result
        .props
        .iter()
        .find(|p| p.name == "bar")
        .expect("bar prop should exist");
    let bar_expansion = bar_prop
        .type_expansion
        .as_ref()
        .expect("bar should have expansion metadata");
    assert_eq!(
        bar_expansion.diagnostics.len(),
        1,
        "bar should have exactly 1 per-field diagnostic"
    );
    assert_eq!(
        bar_expansion.diagnostics[0].reason,
        crate::analysis::type_expand::ExpansionStopReason::UnsupportedOperator
    );
    // Negative: bar must NOT contain the global diagnostic
    assert!(
        !bar_expansion
            .diagnostics
            .iter()
            .any(|d| d.property_name.is_none()),
        "per-field diagnostics for bar must NOT contain any global (property_name=None) diagnostics"
    );
}

#[test]
fn define_emits_call_signature_events_get_empty_diagnostics_not_global_clones() {
    // Setup: defineEmits with call-signature style (e.g. (e: 'change', value: string) => void)
    // with 2 diagnostics: one global (property_name=None), one for property "change"
    let macros = vec![AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineEmits,
        ..make_define_props(vec![])
    }];

    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        props: Vec::new(),
        define_props: Vec::new(),
        define_emits: vec![crate::analysis::type_expand::ExpandedMacroObjectShape {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::incomplete(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: Vec::new(),
                    index_signatures: Vec::new(),
                    call_signatures: vec![crate::analysis::type_expand::ExpandedCallSignature {
                        parameters: vec![
                            // First param is the event name literal
                            crate::analysis::type_expand::ExpandedParameter {
                                name: "e".to_string(),
                                ty: TypeExpr::Literal(verter_type_expr::LiteralValue::String(
                                    "change".to_string(),
                                )),
                                optional: false,
                                rest: false,
                            },
                            // Second param is the payload
                            crate::analysis::type_expand::ExpandedParameter {
                                name: "value".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                rest: false,
                            },
                        ],
                        return_type: TypeExpr::Primitive(PrimitiveName::Void),
                        type_parameters: Vec::new(),
                    }],
                },
                crate::analysis::type_expand::ExpansionExecutionStatus::Completed,
                vec![
                    // Global diagnostic
                    crate::analysis::type_expand::ExpansionDiagnostic {
                        reason: crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded,
                        context: "emits budget exceeded".to_string(),
                        property_name: None,
                    },
                    // Per-property diagnostic for "change"
                    crate::analysis::type_expand::ExpansionDiagnostic {
                        reason:
                            crate::analysis::type_expand::ExpansionStopReason::UnresolvedReference,
                        context: "unresolved in change handler".to_string(),
                        property_name: Some("change".to_string()),
                    },
                ],
            ),
        }],
        emits: Vec::new(),
        define_slots: Vec::new(),
        slot_bindings: Vec::new(),
        bindings: Vec::new(),
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);

    let result = extract_component_meta(input);

    // --- macro_expansion_diagnostics should capture the global diagnostic ---
    assert_eq!(
        result.macro_expansion_diagnostics.len(),
        1,
        "should have one macro-level diagnostic entry for defineEmits"
    );
    let macro_diag = &result.macro_expansion_diagnostics[0];
    assert_eq!(macro_diag.macro_kind, MacroExpansionKind::DefineEmits);
    assert_eq!(macro_diag.macro_index, 0);
    assert_eq!(
        macro_diag.diagnostics.len(),
        1,
        "macro-level entry should contain only the global diagnostic"
    );
    assert_eq!(
        macro_diag.diagnostics[0].reason,
        crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded
    );

    // --- call-signature event "change" should have empty diagnostics ---
    let change_event = result
        .events
        .iter()
        .find(|e| e.name == "change")
        .expect("change event should be extracted from call signature");
    let change_expansion = change_event
        .payload_expansion
        .as_ref()
        .expect("change event should have payload_expansion metadata");
    assert!(
        change_expansion.diagnostics.is_empty(),
        "call-signature events must get empty diagnostics, not cloned macro-wide diagnostics; \
         found {} diagnostics: {:?}",
        change_expansion.diagnostics.len(),
        change_expansion.diagnostics
    );
    // Negative: must not contain the global BudgetExceeded diagnostic
    assert!(
        !change_expansion
            .diagnostics
            .iter()
            .any(|d| d.reason == crate::analysis::type_expand::ExpansionStopReason::BudgetExceeded),
        "call-signature event diagnostics must NOT contain the global BudgetExceeded diagnostic"
    );
    // Negative: must not contain the per-property "change" diagnostic either
    // (call-signature events always get empty diagnostics)
    assert!(
        !change_expansion
            .diagnostics
            .iter()
            .any(|d| d.property_name.as_deref() == Some("change")),
        "call-signature event diagnostics must NOT contain per-property diagnostics"
    );
}

// ---------------------------------------------------------------------------
// @defaultValue tag synthesis from withDefaults default_value
// ---------------------------------------------------------------------------

#[test]
fn synthesizes_default_value_tag_from_with_defaults() {
    let define_props = make_define_props(vec![make_prop("label", Some("string"), true)]);
    let with_defaults = AnalyzedMacro {
        kind: AnalyzedMacroKind::WithDefaults,
        default_keys: vec!["label".to_string()],
        default_values: vec![crate::analysis::types::AnalyzedDefaultValue {
            key: "label".to_string(),
            value: "\"hello\"".to_string(),
            span: verter_span::Span::default(),
        }],
        ..make_define_props(vec![])
    };
    let macros = vec![define_props, with_defaults];

    let result = extract_component_meta(empty_input(&macros));

    let label = result.props.iter().find(|p| p.name == "label").unwrap();
    assert_eq!(label.default_value.as_deref(), Some("\"hello\""));
    let default_tag = label.tags.iter().find(|t| t.name == "defaultValue");
    assert!(
        default_tag.is_some(),
        "should synthesize @defaultValue tag from withDefaults"
    );
    assert_eq!(default_tag.unwrap().text.as_deref(), Some("\"hello\""));
}

#[test]
fn does_not_duplicate_existing_default_value_tag() {
    // Source JSDoc already supplied an @defaultValue tag; withDefaults provides
    // a different runtime default. Source JSDoc must win — synthesis must not
    // duplicate or overwrite.
    let mut field = make_prop("as", Some("string"), true);
    field.tags = vec![JsdocTag {
        name: "defaultValue".to_string(),
        text: Some("'button'".to_string()),
    }];
    let define_props = make_define_props(vec![field]);
    let with_defaults = AnalyzedMacro {
        kind: AnalyzedMacroKind::WithDefaults,
        default_keys: vec!["as".to_string()],
        default_values: vec![crate::analysis::types::AnalyzedDefaultValue {
            key: "as".to_string(),
            value: "\"div\"".to_string(),
            span: verter_span::Span::default(),
        }],
        ..make_define_props(vec![])
    };
    let macros = vec![define_props, with_defaults];

    let result = extract_component_meta(empty_input(&macros));

    let prop = result.props.iter().find(|p| p.name == "as").unwrap();
    let default_tags: Vec<_> = prop
        .tags
        .iter()
        .filter(|t| t.name == "defaultValue")
        .collect();
    assert_eq!(
        default_tags.len(),
        1,
        "should not duplicate existing @defaultValue tag"
    );
    assert_eq!(
        default_tags[0].text.as_deref(),
        Some("'button'"),
        "source JSDoc default must be preserved"
    );
}

#[test]
fn no_default_value_means_no_synthesized_tag() {
    let macros = vec![make_define_props(vec![make_prop(
        "name",
        Some("string"),
        false,
    )])];

    let result = extract_component_meta(empty_input(&macros));

    let name = result.props.iter().find(|p| p.name == "name").unwrap();
    assert!(name.default_value.is_none());
    assert!(
        !name.tags.iter().any(|t| t.name == "defaultValue"),
        "no @defaultValue tag should be synthesized when default_value is None"
    );
}

#[test]
fn synthesizes_default_value_tag_for_runtime_define_props() {
    // Runtime defineProps({ msg: { default: 'hi' } }) stores defaults on the
    // DefineProps macro itself, not on a wrapping WithDefaults.
    let define_props = AnalyzedMacro {
        default_keys: vec!["msg".to_string()],
        default_values: vec![crate::analysis::types::AnalyzedDefaultValue {
            key: "msg".to_string(),
            value: "'hi'".to_string(),
            span: verter_span::Span::default(),
        }],
        ..make_define_props(vec![make_prop("msg", Some("string"), true)])
    };
    let macros = vec![define_props];

    let result = extract_component_meta(empty_input(&macros));

    let msg = result.props.iter().find(|p| p.name == "msg").unwrap();
    let tag = msg
        .tags
        .iter()
        .find(|t| t.name == "defaultValue")
        .expect("runtime defineProps default should produce a @defaultValue tag");
    assert_eq!(tag.text.as_deref(), Some("'hi'"));
}

// ---------------------------------------------------------------------------
// Expanded-only props inherit JSDoc by name from canonical_source
// ---------------------------------------------------------------------------

#[test]
fn expanded_props_without_source_field_inherit_jsdoc_from_canonical_source() {
    // Macro has no prop_fields; expansion produced two props "foo" and "bar".
    // canonical_source carries JSDoc for both — extractor should populate
    // description/tags by property name.
    let macros = vec![make_define_props(Vec::new())];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "foo".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "bar".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: true,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        ..crate::analysis::type_expand::ExpandedComponentTypes::default()
    };
    let source = "interface Inner {\n    /** Doc for foo */\n    foo: string;\n    /** Doc for bar.\n     * @deprecated use baz\n     */\n    bar?: number;\n}\n";

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);
    input.canonical_source = Some(source);

    let result = extract_component_meta(input);

    let foo = result
        .props
        .iter()
        .find(|p| p.name == "foo")
        .expect("foo should be present");
    assert_eq!(
        foo.description.as_deref(),
        Some("Doc for foo"),
        "expanded-only prop foo should inherit JSDoc description from source"
    );

    let bar = result
        .props
        .iter()
        .find(|p| p.name == "bar")
        .expect("bar should be present");
    assert_eq!(bar.description.as_deref(), Some("Doc for bar."));
    assert!(
        bar.tags.iter().any(|t| t.name == "deprecated"),
        "expanded-only prop bar should inherit @deprecated tag"
    );
}

#[test]
fn expanded_only_props_in_third_branch_inherit_jsdoc_from_canonical_source() {
    // Third-branch path: prop_fields contains "x" but expansion adds "y" too.
    // The first branch only fires when eval_fields is non-empty AND the macro is
    // type-based; here we drive the third branch by giving prop_fields one entry
    // and an empty define_props expansion plus a non-empty `props` extension.
    //
    // Simpler reproduction: provide prop_fields = [x] and define_props expansion
    // with both x and y. The first branch fires and iterates eval_fields; since
    // y has no source_field, that triggers the same loss point. Use that to
    // assert the helper kicks in.
    let macros = vec![make_define_props(vec![make_prop(
        "x",
        Some("string"),
        false,
    )])];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "x".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                        crate::analysis::type_expand::ExpandedProperty {
                            name: "y".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::Number),
                            optional: false,
                            readonly: false,
                            visibility: verter_type_expr::MemberVisibility::Public,
                            declared_in_macro_type_arg: false,
                        },
                    ],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        ..crate::analysis::type_expand::ExpandedComponentTypes::default()
    };
    let source = "interface Spread {\n    x: string;\n    /** Doc for y */\n    y: number;\n}\n";

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);
    input.canonical_source = Some(source);

    let result = extract_component_meta(input);

    let y = result.props.iter().find(|p| p.name == "y").expect("y");
    assert_eq!(
        y.description.as_deref(),
        Some("Doc for y"),
        "expanded-only prop y should inherit JSDoc description from canonical_source"
    );
}

#[test]
fn expanded_only_props_without_canonical_source_have_no_jsdoc() {
    let macros = vec![make_define_props(Vec::new())];
    let evaluated = crate::analysis::type_expand::ExpandedComponentTypes {
        define_props: vec![crate::analysis::type_expand::ExpandedMacroProps {
            macro_index: 0,
            result: crate::analysis::type_expand::ExpansionResult::exact(
                crate::analysis::type_expand::ExpandedObjectShape {
                    properties: vec![crate::analysis::type_expand::ExpandedProperty {
                        name: "foo".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: false,
                        readonly: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        declared_in_macro_type_arg: false,
                    }],
                    index_signatures: Vec::new(),
                    call_signatures: Vec::new(),
                },
            ),
        }],
        ..crate::analysis::type_expand::ExpandedComponentTypes::default()
    };

    let mut input = empty_input(&macros);
    input.evaluated_types = Some(&evaluated);
    // canonical_source intentionally None

    let result = extract_component_meta(input);

    let foo = result.props.iter().find(|p| p.name == "foo").unwrap();
    assert!(foo.description.is_none(), "no source — no description");
    assert!(foo.tags.is_empty(), "no source — no tags");
}
