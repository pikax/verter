//! Sibling tests for `surface_projector`.
//!
//! Extracted from the inline `#[cfg(test)] mod tests` block per the
//! CLAUDE.md "Rust test file organization" rule (~400 line cap on
//! inline test modules). The parent's `#[cfg(test)] mod
//! surface_projector_tests` declaration gates compilation; no inner
//! `#![cfg(test)]` is needed.

use super::surface_projector::*;
use verter_compiler::utils::oxc::vue::resolve_type::{
    ResolvedElements, ResolvedEmit, ResolvedEmitSignature, ResolvedMemberVisibility, ResolvedProp,
};
use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_semantic::analysis::TypeResolutionSource;
use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr, TypeExprScope,
};

fn prop(
    name: &str,
    optional: bool,
    visibility: ResolvedMemberVisibility,
    type_text: Option<&str>,
    span_start: u32,
) -> ResolvedProp {
    ResolvedProp {
        span: verter_span::Span::new(span_start, span_start + 8),
        key: verter_span::Span::new(span_start, span_start + 3),
        key_name: Some(name.to_string()),
        optional,
        types: Vec::new(),
        visibility,
        type_span: None,
        type_text: type_text.map(str::to_string),
        map_local: false,
        span_is_absolute: true,
        type_expr: None,
        type_expr_scope: None,
        declared_in_macro_type_arg: false,
    }
}

fn prop_with_type_span(
    name: &str,
    optional: bool,
    visibility: ResolvedMemberVisibility,
    type_text: Option<&str>,
    span: verter_span::Span,
    key: verter_span::Span,
    type_span: verter_span::Span,
) -> ResolvedProp {
    ResolvedProp {
        span,
        key,
        key_name: Some(name.to_string()),
        optional,
        types: Vec::new(),
        visibility,
        type_span: Some(type_span),
        type_text: type_text.map(str::to_string),
        map_local: false,
        span_is_absolute: true,
        type_expr: None,
        type_expr_scope: None,
        declared_in_macro_type_arg: false,
    }
}

#[test]
fn project_define_props_filters_non_public_members() {
    let elements = ResolvedElements {
        props: vec![
            prop(
                "label",
                false,
                ResolvedMemberVisibility::Public,
                Some("string"),
                0,
            ),
            prop(
                "secret",
                true,
                ResolvedMemberVisibility::Private,
                Some("number"),
                10,
            ),
        ],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

    assert_eq!(projected.native_props.len(), 2);
    assert_eq!(projected.props.len(), 1);
    assert_eq!(projected.props[0].name, "label");
    assert_eq!(
        projected.props[0].resolution_source,
        TypeResolutionSource::Rust
    );
}

#[test]
fn project_define_emits_formats_payloads() {
    let elements = ResolvedElements {
        emits: vec![
            ResolvedEmit {
                span: verter_span::Span::new(0, 5),
                name: "save".to_string(),
                name_span: None,
                signature: ResolvedEmitSignature::Call {
                    params_text: "value: string".to_string(),
                },
                map_local: false,
                span_is_absolute: true,
                type_expr: None,
                type_expr_scope: None,
            },
            ResolvedEmit {
                span: verter_span::Span::new(6, 12),
                name: "cancel".to_string(),
                name_span: None,
                signature: ResolvedEmitSignature::Tuple {
                    tuple_text: "[reason: number]".to_string(),
                },
                map_local: false,
                span_is_absolute: true,
                type_expr: None,
                type_expr_scope: None,
            },
        ],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineEmits, &elements);

    assert_eq!(projected.emits.len(), 2);
    assert_eq!(
        projected.emits[0].payload_type.as_deref(),
        Some("[value: string]")
    );
    assert_eq!(
        projected.emits[1].payload_type.as_deref(),
        Some("[reason: number]")
    );
}

#[test]
fn project_define_props_prefers_raw_source_type_span_text() {
    let source = "interface Props { type?: SingleOrMultipleType }";
    let type_start = source.find("SingleOrMultipleType").unwrap() as u32;
    let prop_start = source.find("type?").unwrap() as u32;
    let elements = ResolvedElements {
        props: vec![prop_with_type_span(
            "type",
            true,
            ResolvedMemberVisibility::Public,
            None,
            verter_span::Span::new(prop_start, source.len() as u32 - 2),
            verter_span::Span::new(prop_start, prop_start + 4),
            verter_span::Span::new(type_start, type_start + "SingleOrMultipleType".len() as u32),
        )],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineProps, &elements);

    assert_eq!(
        projected.props[0].type_annotation.as_deref(),
        Some("SingleOrMultipleType")
    );
}

#[test]
fn project_define_props_prefers_pre_resolved_cross_file_type_text_over_source_span() {
    let source = r#"export interface ButtonProps {
  /**
   * @defaultValue 'md'
   */
  size?: Button['variants']['size']
}"#;
    let type_start = source.find("'md'").unwrap() as u32;
    let prop_start = source.find("size?").unwrap() as u32;
    let elements = ResolvedElements {
        props: vec![prop_with_type_span(
            "size",
            true,
            ResolvedMemberVisibility::Public,
            Some("Button['variants']['size']"),
            verter_span::Span::new(prop_start, source.len() as u32 - 2),
            verter_span::Span::new(prop_start, prop_start + 4),
            verter_span::Span::new(type_start, type_start + 4),
        )],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineProps, &elements);

    assert_eq!(
        projected.props[0].type_annotation.as_deref(),
        Some("Button['variants']['size']")
    );
}

#[test]
fn project_define_emits_prefers_raw_source_tuple_payload_text() {
    let source =
            "type Emits = { 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]; }";
    let emit_start = source.find("'update:modelValue'").unwrap() as u32;
    let emit_end = source[emit_start as usize..].find(';').unwrap() as u32 + emit_start;
    let elements = ResolvedElements {
        emits: vec![ResolvedEmit {
            span: verter_span::Span::new(emit_start, emit_end),
            name: "update:modelValue".to_string(),
            name_span: None,
            signature: ResolvedEmitSignature::Tuple {
                tuple_text: "[value: string | string[] | undefined]".to_string(),
            },
            map_local: false,
            span_is_absolute: true,
            type_expr: None,
            type_expr_scope: None,
        }],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineEmits, &elements);

    assert_eq!(
        projected.emits[0].payload_type.as_deref(),
        Some("[value: (T extends 'single' ? string : string[]) | undefined]")
    );
}

/// Build a `TypeExpr::Function` representing `(props: { props_obj }) => return_ty`
/// for slot-test fixtures.
fn slot_function_type(props: Vec<ObjectMember>, return_ty: TypeExpr) -> TypeExpr {
    TypeExpr::Function(std::sync::Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("props".to_string()),
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties: props })),
            false,
            false,
        )],
        Some(std::sync::Arc::new(return_ty)),
        Vec::new(),
    )))
}

fn slot_object_property(name: &str, optional: bool, ty: TypeExpr) -> ObjectMember {
    ObjectMember::Property(ObjectProperty::synthetic(
        name.to_string(),
        ty,
        optional,
        false,
    ))
}

#[test]
fn project_define_slots_extracts_bindings_and_return_type() {
    // W3.2 typed-IR-only: slot info derives from the prop's lowered
    // `TypeExpr::Function` form. The pre-W3.2 path read the source-text
    // and reparsed via OXC; that source-walker is deleted.
    let return_ty = TypeExpr::Array {
        element: std::sync::Arc::new(TypeExpr::Ref {
            name: std::sync::Arc::from("VNode"),
            type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
        }),
        readonly: false,
    };
    let function_ty = slot_function_type(
        vec![
            slot_object_property(
                "foo",
                false,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            ),
            slot_object_property(
                "bar",
                true,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            ),
        ],
        return_ty,
    );
    let slot_prop = typed_prop("default", false, function_ty, "/sfc/Slots.vue");
    let elements = ResolvedElements {
        props: vec![slot_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

    assert_eq!(projected.slots.len(), 1);
    assert_eq!(projected.slots[0].name, "default");
    assert_eq!(projected.slots[0].bindings.len(), 2);
    assert_eq!(projected.slots[0].bindings[0].name, "foo");
    assert_eq!(projected.slots[0].bindings[1].name, "bar");
    // Display: rendered from typed return-type (`Ref<VNode>[]`).
    assert_eq!(projected.slots[0].return_type.as_deref(), Some("VNode[]"));
    // Discriminator: pre-W3.2 the source-walker filled `binding_expr: None`
    // for the synthetic-declaration path; post-W3.2 the typed walker emits
    // `binding_expr: Some(<typed value>)` for every binding.
    assert_eq!(
        projected.slots[0].bindings[0].binding_expr.as_ref(),
        Some(&TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::String
        ))
    );
    assert_eq!(
        projected.slots[0].bindings[1].binding_expr.as_ref(),
        Some(&TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Number
        ))
    );
}

#[test]
fn project_define_slots_preserves_symbolic_binding_types_for_pick_params() {
    // W3.2 typed-IR-only: `Pick<X, K>` arrives as
    // `TypeExpr::Ref { name: "Pick", type_arguments: [Ref<X>, Literal::String("K")] }`
    // — no source-text walk. The typed walker emits one binding per key,
    // with `binding_expr = IndexedAccess { object: Ref<X>, index: Literal::String("K") }`
    // and a symbolic `X['K']` display string for parity.
    let calendar_ref = TypeExpr::Ref {
        name: std::sync::Arc::from("CalendarCellTriggerProps"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
    };
    let pick_ty = TypeExpr::Ref {
        name: std::sync::Arc::from("Pick"),
        type_arguments: std::sync::Arc::from(vec![
            calendar_ref.clone(),
            TypeExpr::Literal(verter_type_expr::LiteralValue::String("day".to_string())),
        ]),
    };
    let function_ty = TypeExpr::Function(std::sync::Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("props".to_string()),
            pick_ty,
            false,
            false,
        )],
        Some(std::sync::Arc::new(TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Any,
        ))),
        Vec::new(),
    )));
    let slot_prop = typed_prop("day", true, function_ty, "/sfc/Calendar.vue");
    let elements = ResolvedElements {
        props: vec![slot_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

    assert_eq!(projected.slots.len(), 1);
    assert_eq!(projected.slots[0].bindings.len(), 1);
    assert_eq!(projected.slots[0].bindings[0].name, "day");
    assert_eq!(
        projected.slots[0].bindings[0].type_annotation.as_deref(),
        Some("CalendarCellTriggerProps['day']")
    );
    // Discriminator: the typed walker must emit an `IndexedAccess` typed
    // form so consumers can navigate without re-parsing the symbolic
    // display string.
    let expected_binding_expr = TypeExpr::IndexedAccess {
        object: std::sync::Arc::new(calendar_ref.clone()),
        index: std::sync::Arc::new(TypeExpr::Literal(verter_type_expr::LiteralValue::String(
            "day".to_string(),
        ))),
    };
    assert_eq!(
        projected.slots[0].bindings[0].binding_expr.as_ref(),
        Some(&expected_binding_expr)
    );
}

// the
// `project_expanded_text_define_emits_preserves_conditional_payload_text`
// and `project_local_source_define_slots_preserves_symbolic_pick_binding`
// unit tests were attached to the (now-deleted) text-based
// projector helpers. Their behaviour contracts are now covered
// by integration tests in `meta_resolve_tests` and
// `component_meta_audit`.

#[test]
fn project_define_slots_ignores_non_callable_helper_members() {
    // W3.2: only typed function shapes survive as slots. Non-callable
    // helper members (plain `TypeExpr::Object`) leave `bindings: empty`
    // and `return_type: None`; the `resolved_as_slot` flag (driven by
    // `RuntimeType::Function`) gates the second-level filter. Helper
    // members with neither typed function form nor `RuntimeType::Function`
    // are dropped.
    let default_function_ty = slot_function_type(
        vec![slot_object_property(
            "item",
            false,
            TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        )],
        TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any),
    );
    let app_config_object_ty = TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
        properties: vec![slot_object_property(
            "ui",
            true,
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                properties: vec![slot_object_property(
                    "variant",
                    false,
                    TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                )],
            })),
        )],
    }));
    let slots_object_ty = TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
        properties: vec![
            slot_object_property(
                "leading",
                true,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            ),
            slot_object_property(
                "trailing",
                true,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
            ),
        ],
    }));
    let elements = ResolvedElements {
        props: vec![
            {
                let mut p = typed_prop("default", false, default_function_ty, "/sfc/Helpers.vue");
                p.types =
                    vec![verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function];
                p
            },
            typed_prop("appConfig", false, app_config_object_ty, "/sfc/Helpers.vue"),
            typed_prop("slots", false, slots_object_ty, "/sfc/Helpers.vue"),
        ],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);
    let names: Vec<_> = projected
        .slots
        .iter()
        .map(|slot| slot.name.as_str())
        .collect();

    assert_eq!(names, vec!["default"]);
}

// the `project_local_source_define_props_*` tests
// exercised the (now-deleted) source-typed projector. The
// behaviour contracts they covered (heritage resolution, JSDoc
// through `@vue-ignore`-annotated `Omit<>`) are covered by
// integration tests in `meta_resolve_tests` and `meta_tests`.

// ── W1.1b: parser typed sidecar → analyzer surface bridge ──
//
// The projector reads `ResolvedProp.type_expr` /
// `ResolvedEmit.type_expr` (populated by the parser via
// `lower_ts_type` at every OXC visit point) and stamps them onto
// the analyzer-side `AnalyzedPropField.type_expr` /
// `AnalyzedEmitField.payload_expr` plus paired `*_scope` fields.
// The aggregate `ProjectedMacroSurfaces.{props,emits,slots}_expr`
// synthesises a `TypeExpr::Object` from those typed sidecars.

fn typed_prop(
    name: &str,
    optional: bool,
    type_expr: TypeExpr,
    scope_canonical: &str,
) -> verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp {
    verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp {
        span: verter_span::Span::new(0, 8),
        key: verter_span::Span::new(0, 3),
        key_name: Some(name.to_string()),
        optional,
        types: Vec::new(),
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: Some("…".to_string()),
        map_local: false,
        span_is_absolute: true,
        type_expr: Some(type_expr),
        type_expr_scope: Some(TypeExprScope::new(scope_canonical)),
        declared_in_macro_type_arg: false,
    }
}

fn typed_emit(
    name: &str,
    type_expr: TypeExpr,
    scope_canonical: &str,
) -> verter_compiler::utils::oxc::vue::resolve_type::ResolvedEmit {
    verter_compiler::utils::oxc::vue::resolve_type::ResolvedEmit {
        span: verter_span::Span::new(0, 8),
        name: name.to_string(),
        name_span: None,
        signature: ResolvedEmitSignature::Tuple {
            tuple_text: "[]".to_string(),
        },
        map_local: false,
        span_is_absolute: true,
        type_expr: Some(type_expr),
        type_expr_scope: Some(TypeExprScope::new(scope_canonical)),
    }
}

#[test]
fn project_define_props_bridges_per_field_typed_sidecar_from_resolved_prop() {
    let prop_type = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let elements = ResolvedElements {
        props: vec![typed_prop("label", false, prop_type.clone(), "/sfc/A.vue")],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

    // Discriminator: pre-W1.1b this field is None (the bridge is missing).
    assert_eq!(projected.props.len(), 1);
    assert_eq!(projected.props[0].type_expr.as_ref(), Some(&prop_type));
    assert_eq!(
        projected.props[0]
            .type_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/A.vue")
    );
    // Pairing invariant: scope present iff expr present.
    assert_eq!(
        projected.props[0].type_expr.is_some(),
        projected.props[0].type_expr_scope.is_some()
    );
}

#[test]
fn project_define_emits_bridges_per_field_payload_expr_from_resolved_emit() {
    let payload = TypeExpr::Tuple {
        elements: std::sync::Arc::from(vec![verter_type_expr::TupleElement {
            label: Some("id".to_string()),
            ty: TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
            optional: false,
            rest: false,
        }]),
        readonly: false,
    };
    let elements = ResolvedElements {
        emits: vec![typed_emit("save", payload.clone(), "/sfc/B.vue")],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineEmits, &elements);

    // Discriminator: pre-W1.1b this field is None.
    assert_eq!(projected.emits.len(), 1);
    assert_eq!(projected.emits[0].payload_expr.as_ref(), Some(&payload));
    assert_eq!(
        projected.emits[0]
            .payload_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/B.vue")
    );
}

#[test]
fn project_define_emits_bridges_property_style_payload_expr() {
    // Property-style emits (mapped into `elements.props` rather than
    // `elements.emits`) must also bridge the typed sidecar.
    let payload = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let elements = ResolvedElements {
        props: vec![typed_prop("change", false, payload.clone(), "/sfc/C.vue")],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineEmits, &elements);

    assert_eq!(projected.emits.len(), 1);
    assert_eq!(projected.emits[0].name, "change");
    assert_eq!(projected.emits[0].payload_expr.as_ref(), Some(&payload));
    assert_eq!(
        projected.emits[0]
            .payload_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/C.vue")
    );
}

#[test]
fn project_macro_surfaces_synthesises_aggregate_props_expr_object_from_typed_inputs() {
    let prop_a = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let prop_b = TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number);
    let elements = ResolvedElements {
        props: vec![
            typed_prop("label", false, prop_a.clone(), "/sfc/A.vue"),
            typed_prop("count", true, prop_b.clone(), "/sfc/A.vue"),
        ],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces_with_owner(
        None,
        Some("/sfc/A.vue"),
        AnalyzedMacroKind::DefineProps,
        &elements,
    );

    // Discriminator: pre-W1.1b `props_expr` is None (no aggregate
    // synthesis); post-W1.1b it carries the typed Object directly.
    let aggregate = projected
        .props_expr
        .as_ref()
        .expect("aggregate props_expr populated when every prop has typed sidecar");
    let TypeExpr::Object(object) = aggregate else {
        panic!("expected aggregate props_expr to be TypeExpr::Object, got {aggregate:?}");
    };
    let names: Vec<&str> = object
        .properties
        .iter()
        .filter_map(|m| match m {
            ObjectMember::Property(p) => Some(p.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["label", "count"]);
    // The typed prop body is preserved byte-for-byte (no reparsing).
    let label_ty = match &object.properties[0] {
        ObjectMember::Property(p) => &p.ty,
        _ => panic!("expected property"),
    };
    assert_eq!(label_ty, &prop_a);
    let count_member = match &object.properties[1] {
        ObjectMember::Property(p) => p,
        _ => panic!("expected property"),
    };
    assert_eq!(&count_member.ty, &prop_b);
    assert!(count_member.optional, "optional flag preserved");
    // Pairing invariant.
    assert_eq!(
        projected
            .props_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/A.vue")
    );
}

#[test]
fn project_macro_surfaces_aggregate_props_expr_is_none_when_owner_canonical_missing() {
    let prop_a = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let elements = ResolvedElements {
        props: vec![typed_prop("label", false, prop_a, "/sfc/A.vue")],
        ..ResolvedElements::default()
    };

    // Without `owner_canonical`, the aggregate has no scope to attach;
    // pairing invariant forces both to be None.
    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

    assert!(projected.props_expr.is_none());
    assert!(projected.props_expr_scope.is_none());
}

#[test]
fn project_macro_surfaces_aggregate_props_expr_is_none_when_any_prop_lacks_typed_sidecar() {
    // One prop with typed sidecar, one without — aggregate cannot be
    // synthesised because the missing prop would show up as Unknown,
    // collapsing the typed-IR contract for the surface.
    let prop_a = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let untyped = verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp {
        span: verter_span::Span::new(0, 8),
        key: verter_span::Span::new(0, 3),
        key_name: Some("count".to_string()),
        optional: false,
        types: Vec::new(),
        visibility: ResolvedMemberVisibility::Public,
        type_span: None,
        type_text: None,
        map_local: false,
        span_is_absolute: true,
        type_expr: None,
        type_expr_scope: None,
        declared_in_macro_type_arg: false,
    };
    let elements = ResolvedElements {
        props: vec![typed_prop("label", false, prop_a, "/sfc/A.vue"), untyped],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces_with_owner(
        None,
        Some("/sfc/A.vue"),
        AnalyzedMacroKind::DefineProps,
        &elements,
    );

    assert!(
        projected.props_expr.is_none(),
        "aggregate props_expr requires every prop to have a typed sidecar"
    );
    assert!(projected.props_expr_scope.is_none());
}

#[test]
fn project_define_slots_bridges_return_expr_from_function_type() {
    // A slot prop typed as `(props: T) => R` — the projector pulls out
    // the function's return type for the slot's `return_expr`, with
    // scope inherited from the slot prop's `type_expr_scope`.
    let return_ty = TypeExpr::Ref {
        name: std::sync::Arc::from("VNode"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
    };
    let function_ty = TypeExpr::Function(std::sync::Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("props".to_string()),
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty::synthetic(
                    "item".to_string(),
                    TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
                    false,
                    false,
                ))],
            })),
            false,
            false,
        )],
        Some(std::sync::Arc::new(return_ty.clone())),
        Vec::new(),
    )));
    let mut slot_prop = typed_prop("default", false, function_ty, "/sfc/D.vue");
    slot_prop.types = vec![verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function];
    let elements = ResolvedElements {
        props: vec![slot_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

    assert_eq!(projected.slots.len(), 1);
    // Discriminator: pre-W1.1b `return_expr` is None.
    assert_eq!(projected.slots[0].return_expr.as_ref(), Some(&return_ty));
    assert_eq!(
        projected.slots[0]
            .return_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/D.vue")
    );
}

// ── Inline slot bindings derive from the slot prop's typed function param ──
//
// The slot prop is typed as `(props: { x: number; y: string }) => R`. The
// projector walks the function's first param's typed `Object { properties }`
// and populates each `AnalyzedSlotFieldBinding.binding_expr` (+ paired scope)
// from the matching property's typed value. W3.2 made this the single source
// of truth for inline slot bindings — there is no source-text reparse path.

#[test]
fn project_define_slots_populates_inline_binding_exprs_from_typed_function_param() {
    let item_ty = TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number);
    let label_ty = TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    let return_ty = TypeExpr::Ref {
        name: std::sync::Arc::from("VNode"),
        type_arguments: std::sync::Arc::from(Vec::<TypeExpr>::new()),
    };
    let function_ty = TypeExpr::Function(std::sync::Arc::new(FunctionExpr::synthetic(
        vec![FunctionParam::synthetic(
            Some("props".to_string()),
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty::synthetic(
                        "x".to_string(),
                        item_ty.clone(),
                        false,
                        false,
                    )),
                    ObjectMember::Property(ObjectProperty::synthetic(
                        "y".to_string(),
                        label_ty.clone(),
                        false,
                        false,
                    )),
                ],
            })),
            false,
            false,
        )],
        Some(std::sync::Arc::new(return_ty)),
        Vec::new(),
    )));
    let mut slot_prop = typed_prop("default", false, function_ty, "/sfc/E.vue");
    slot_prop.types = vec![verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function];
    let elements = ResolvedElements {
        props: vec![slot_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

    assert_eq!(projected.slots.len(), 1);
    let slot = &projected.slots[0];
    let names: Vec<&str> = slot.bindings.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["x", "y"],
        "slot bindings should be extracted from the typed function param's object literal"
    );
    // Discriminator: every binding's `binding_expr` must mirror the
    // matching `ObjectProperty.ty`. Pre-W3.2 the source-walker would
    // reparse the type-text and produce `binding_expr: None` for
    // non-Pick shapes; post-W3.2 the typed walker emits the typed value
    // directly.
    assert_eq!(
        slot.bindings[0].binding_expr.as_ref(),
        Some(&item_ty),
        "binding_expr for `x` must be the function-param object property's typed value"
    );
    assert_eq!(
        slot.bindings[1].binding_expr.as_ref(),
        Some(&label_ty),
        "binding_expr for `y` must be the function-param object property's typed value"
    );
    // Pairing invariant: scope present iff expr present.
    assert!(
        slot.bindings[0].binding_expr_scope.is_some(),
        "binding_expr_scope must be populated when binding_expr is Some"
    );
    assert_eq!(
        slot.bindings[0]
            .binding_expr_scope
            .as_ref()
            .map(TypeExprScope::as_str),
        Some("/sfc/E.vue"),
        "scope must be inherited from the slot prop's type_expr_scope"
    );
    assert!(slot.bindings[1].binding_expr_scope.is_some());
}

#[test]
fn project_define_slots_emits_no_bindings_when_prop_lacks_typed_function_form() {
    // W3.2 typed-IR-only: the projector relies entirely on the slot
    // prop's lowered `type_expr` for binding extraction. When the parser
    // side leaves `type_expr: None` (e.g. an Options-API path where no
    // OXC `TSType` was lowered), no bindings are emitted — the
    // architecture rejects source-text reparse fallbacks.
    //
    // The slot itself still surfaces because `resolved_as_slot` (driven
    // by `RuntimeType::Function`) gates slot emission independently.
    // Discriminator: pre-W3.2 the source-walker reparsed
    // `"(props: { x: number }) => any"` from `type_text` and emitted the
    // `x` binding with `binding_expr: None`; post-W3.2 no bindings are
    // emitted at all.
    let mut slot_prop = prop(
        "default",
        false,
        ResolvedMemberVisibility::Public,
        Some("(props: { x: number }) => any"),
        0,
    );
    slot_prop.type_expr = None;
    slot_prop.type_expr_scope = None;
    slot_prop.types = vec![verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function];
    let elements = ResolvedElements {
        props: vec![slot_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

    assert_eq!(projected.slots.len(), 1);
    let slot = &projected.slots[0];
    assert!(
        slot.bindings.is_empty(),
        "no bindings should be emitted when the slot prop has no typed function form"
    );
    assert!(
        slot.return_type.is_none(),
        "no display return type without a typed function form"
    );
}

/// R21-F1 c3 discriminating test for the surface_projector
/// `ResolvedProp → AnalyzedPropField` propagation.
///
/// Constructs a `ResolvedElements` with two props: one marked as
/// `declared_in_macro_type_arg = true` (the parser-side fact for an
/// own-body member of the macro T) and one with `false` (a member
/// reached via heritage / Omit / intersection from outside the macro
/// T body).
///
/// Asserts the resulting `AnalyzedPropField` entries carry the SAME
/// per-prop fact through the projector. If the projector site at
/// `surface_projector::project_macro_surfaces_with_owner` is reverted
/// to hardcode `declared_in_macro_type_arg: false`, both props collapse
/// to `false` and the `kept_own_body` assertion FAILS — the test
/// discriminates the fix.
#[test]
fn r21_c3_surface_projector_propagates_declared_in_macro_type_arg_per_prop() {
    let mut own_body_prop = prop(
        "own_body",
        false,
        ResolvedMemberVisibility::Public,
        Some("string"),
        0,
    );
    own_body_prop.declared_in_macro_type_arg = true;

    let mut heritage_prop = prop(
        "heritage",
        false,
        ResolvedMemberVisibility::Public,
        Some("number"),
        16,
    );
    heritage_prop.declared_in_macro_type_arg = false;

    let elements = ResolvedElements {
        props: vec![own_body_prop, heritage_prop],
        ..ResolvedElements::default()
    };

    let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, &elements);

    assert_eq!(
        projected.props.len(),
        2,
        "both public props must reach the projected surface"
    );
    let own_body_field = projected
        .props
        .iter()
        .find(|f| f.name == "own_body")
        .expect("own_body field present");
    let heritage_field = projected
        .props
        .iter()
        .find(|f| f.name == "heritage")
        .expect("heritage field present");

    assert!(
        own_body_field.declared_in_macro_type_arg,
        "own_body member's declared_in_macro_type_arg=true MUST propagate \
         from ResolvedProp through surface_projector to AnalyzedPropField. \
         Discriminator: reverting surface_projector to hardcode `false` \
         flips this to false."
    );
    assert!(
        !heritage_field.declared_in_macro_type_arg,
        "heritage member's declared_in_macro_type_arg=false MUST propagate \
         unchanged — the projector must not synthesize true for non-own-body \
         members."
    );
}
