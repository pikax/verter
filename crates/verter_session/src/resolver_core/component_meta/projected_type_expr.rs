use rustc_hash::FxHashSet;
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, TypeExpr};

use crate::resolver_core::project_macro_surfaces;
use crate::resolver_core::surface_projector::ProjectedMacroSurfaces;

pub fn resolved_elements_to_type_expr_via_type_text(
    resolved: &ResolvedElements,
) -> verter_type_expr::TypeExpr {
    projected_macro_surfaces_to_type_expr(
        AnalyzedMacroKind::DefineProps,
        &project_macro_surfaces(None, AnalyzedMacroKind::DefineProps, resolved),
    )
}

pub fn projected_macro_surfaces_to_type_expr(
    macro_kind: AnalyzedMacroKind,
    projected: &ProjectedMacroSurfaces,
) -> verter_type_expr::TypeExpr {
    let prop_properties = projected.props.iter().map(|prop| {
        let ty = prop
            .type_annotation
            .as_deref()
            .map(verter_type_expr_oxc::parse_type_annotation)
            .unwrap_or(TypeExpr::Unknown {
                raw: "unknown".to_string(),
            });
        ObjectMember::Property(ObjectProperty {
            name: prop.name.clone(),
            ty,
            optional: prop.is_optional,
            readonly: false,
        })
    });

    let emit_properties = projected.emits.iter().map(|emit| {
        let ty = emit
            .payload_type
            .as_deref()
            .map(verter_type_expr_oxc::parse_type_annotation)
            .unwrap_or(TypeExpr::Unknown {
                raw: "unknown".to_string(),
            });
        ObjectMember::Property(ObjectProperty {
            name: emit.name.clone(),
            ty,
            optional: false,
            readonly: false,
        })
    });

    let slot_properties = projected.slots.iter().map(|slot| {
        let return_type = slot.return_type.as_deref().unwrap_or("any");
        let signature = if slot.bindings.is_empty() {
            format!("() => {return_type}")
        } else {
            let bindings = slot
                .bindings
                .iter()
                .map(|binding| {
                    format!(
                        "{}: {}",
                        binding.name,
                        binding.type_annotation.as_deref().unwrap_or("unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("(props: {{ {bindings} }}) => {return_type}")
        };

        ObjectMember::Property(ObjectProperty {
            name: slot.name.clone(),
            ty: verter_type_expr_oxc::parse_type_annotation(&signature),
            optional: !slot.is_required,
            readonly: false,
        })
    });

    let properties = match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => prop_properties.collect(),
        AnalyzedMacroKind::DefineEmits => emit_properties.collect(),
        AnalyzedMacroKind::DefineSlots => slot_properties.collect(),
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => Vec::new(),
    };

    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
}

pub(crate) fn project_macro_surfaces_from_expanded_shape(
    macro_kind: AnalyzedMacroKind,
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> ProjectedMacroSurfaces {
    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: shape
                .properties
                .iter()
                .map(|property| verter_semantic::analysis::AnalyzedPropField {
                    name: property.name.clone(),
                    is_optional: property.optional,
                    span: verter_span::Span::default(),
                    type_annotation: render_type_expr_for_projected_surface(&property.ty),
                    description: None,
                    tags: Vec::new(),
                    resolution_source: verter_semantic::analysis::types::TypeResolutionSource::Rust,
                    resolution_error: None,
                })
                .collect(),
            emits: Vec::new(),
            slots: Vec::new(),
        },
        AnalyzedMacroKind::DefineEmits => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: Vec::new(),
            emits: projected_emit_fields_from_shape(shape),
            slots: Vec::new(),
        },
        AnalyzedMacroKind::DefineSlots => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: Vec::new(),
            emits: Vec::new(),
            slots: projected_slot_fields_from_shape(shape),
        },
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
            ProjectedMacroSurfaces::default()
        }
    }
}

fn projected_emit_fields_from_shape(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> Vec<verter_semantic::analysis::AnalyzedEmitField> {
    use verter_type_expr::{LiteralValue, TupleElement, TypeExpr};

    let mut emits = shape
        .properties
        .iter()
        .map(|property| verter_semantic::analysis::AnalyzedEmitField {
            name: property.name.clone(),
            span: verter_span::Span::default(),
            payload_type: event_payload_raw_signature_from_type_expr_for_projected_surface(
                &property.ty,
            ),
            description: None,
            tags: Vec::new(),
        })
        .collect::<Vec<_>>();

    for signature in &shape.call_signatures {
        let Some(first) = signature.parameters.first() else {
            continue;
        };
        let payload = TypeExpr::Tuple {
            elements: std::sync::Arc::from(
                signature
                    .parameters
                    .iter()
                    .skip(1)
                    .map(|parameter| TupleElement {
                        label: (!parameter.name.is_empty()).then(|| parameter.name.clone()),
                        ty: parameter.ty.clone(),
                        optional: parameter.optional,
                        rest: parameter.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: false,
        };
        let payload_type =
            event_payload_raw_signature_from_type_expr_for_projected_surface(&payload);
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => {
                emits.push(verter_semantic::analysis::AnalyzedEmitField {
                    name: name.clone(),
                    span: verter_span::Span::default(),
                    payload_type: payload_type.clone(),
                    description: None,
                    tags: Vec::new(),
                })
            }
            TypeExpr::Union(types) => {
                for ty in types.iter() {
                    let TypeExpr::Literal(LiteralValue::String(name)) = ty else {
                        continue;
                    };
                    emits.push(verter_semantic::analysis::AnalyzedEmitField {
                        name: name.clone(),
                        span: verter_span::Span::default(),
                        payload_type: payload_type.clone(),
                        description: None,
                        tags: Vec::new(),
                    });
                }
            }
            _ => {}
        }
    }

    let mut seen = FxHashSet::default();
    emits.retain(|emit| seen.insert(emit.name.clone()));
    emits
}

fn projected_slot_fields_from_shape(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
) -> Vec<verter_semantic::analysis::AnalyzedSlotField> {
    shape
        .properties
        .iter()
        .filter_map(|property| {
            let rendered = render_type_expr_for_projected_surface(&property.ty);
            let (bindings, return_type) =
                crate::resolver_core::surface_projector::extract_slot_info_from_type_text(
                    None,
                    rendered.as_deref(),
                );
            if bindings.is_empty() && return_type.is_none() {
                return None;
            }
            Some(verter_semantic::analysis::AnalyzedSlotField {
                name: property.name.clone(),
                is_required: !property.optional,
                span: verter_span::Span::default(),
                bindings,
                return_type,
                description: None,
                tags: Vec::new(),
            })
        })
        .collect()
}

fn event_payload_raw_signature_from_type_expr_for_projected_surface(
    payload: &verter_type_expr::TypeExpr,
) -> Option<String> {
    render_type_expr_for_projected_surface(payload).filter(|rendered| rendered.starts_with('['))
}

fn render_type_expr_for_projected_surface(expr: &verter_type_expr::TypeExpr) -> Option<String> {
    use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

    match expr {
        TypeExpr::Primitive(name) => Some(match name {
            PrimitiveName::String => "string".to_string(),
            PrimitiveName::Number => "number".to_string(),
            PrimitiveName::Boolean => "boolean".to_string(),
            PrimitiveName::BigInt => "bigint".to_string(),
            PrimitiveName::Symbol => "symbol".to_string(),
            PrimitiveName::Null => "null".to_string(),
            PrimitiveName::Undefined => "undefined".to_string(),
            PrimitiveName::Void => "void".to_string(),
            PrimitiveName::Any => "any".to_string(),
            PrimitiveName::Unknown => "unknown".to_string(),
            PrimitiveName::Never => "never".to_string(),
            PrimitiveName::Object => "object".to_string(),
        }),
        TypeExpr::Literal(LiteralValue::String(value)) => Some(format!("{value:?}")),
        TypeExpr::Literal(LiteralValue::Number(value)) => Some(value.to_string()),
        TypeExpr::Literal(LiteralValue::Boolean(value)) => Some(value.to_string()),
        TypeExpr::Literal(LiteralValue::BigInt(value)) => Some(value.clone()),
        TypeExpr::Union(types) => Some(
            types
                .iter()
                .map(render_type_expr_for_projected_surface)
                .collect::<Option<Vec<_>>>()?
                .join(" | "),
        ),
        TypeExpr::Intersection(types) => Some(
            types
                .iter()
                .map(render_type_expr_for_projected_surface)
                .collect::<Option<Vec<_>>>()?
                .join(" & "),
        ),
        TypeExpr::Array { element, readonly } => {
            let rendered = render_type_expr_for_projected_surface(element)?;
            Some(if *readonly {
                format!("readonly {rendered}[]")
            } else {
                format!("{rendered}[]")
            })
        }
        TypeExpr::Tuple { elements, readonly } => {
            let rendered = elements
                .iter()
                .map(|element| {
                    let mut rendered = String::new();
                    if let Some(label) = &element.label {
                        rendered.push_str(label);
                        if element.optional {
                            rendered.push('?');
                        }
                        rendered.push_str(": ");
                    }
                    if element.rest {
                        rendered.push_str("...");
                    }
                    rendered.push_str(&render_type_expr_for_projected_surface(&element.ty)?);
                    Some(rendered)
                })
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            Some(if *readonly {
                format!("readonly [{rendered}]")
            } else {
                format!("[{rendered}]")
            })
        }
        TypeExpr::Object(object) => {
            let rendered = object
                .properties
                .iter()
                .map(|member| match member {
                    ObjectMember::Property(property) => Some(format!(
                        "{}{}: {}",
                        property.name,
                        if property.optional { "?" } else { "" },
                        render_type_expr_for_projected_surface(&property.ty)?
                    )),
                    ObjectMember::Method(method) => Some(format!(
                        "{}{}{}",
                        method.name,
                        if method.optional { "?" } else { "" },
                        render_function_type_for_projected_surface(&method.function)?
                            .strip_prefix('(')
                            .unwrap_or("")
                    )),
                    ObjectMember::CallSignature(function) => {
                        render_function_type_for_projected_surface(function)
                    }
                    ObjectMember::ConstructSignature(_) | ObjectMember::IndexSignature(_) => None,
                })
                .collect::<Option<Vec<_>>>()?
                .join("; ");
            Some(format!("{{ {rendered} }}"))
        }
        TypeExpr::Function(function) => render_function_type_for_projected_surface(function),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            if type_arguments.is_empty() {
                Some(name.to_string())
            } else {
                let args = type_arguments
                    .iter()
                    .map(render_type_expr_for_projected_surface)
                    .collect::<Option<Vec<_>>>()?;
                Some(format!("{}<{}>", name, args.join(", ")))
            }
        }
        TypeExpr::Parenthesized(inner) => Some(format!(
            "({})",
            render_type_expr_for_projected_surface(inner)?
        )),
        TypeExpr::Rest(inner) => Some(format!(
            "...{}",
            render_type_expr_for_projected_surface(inner)?
        )),
        _ => None,
    }
}

fn render_function_type_for_projected_surface(
    function: &verter_type_expr::FunctionExpr,
) -> Option<String> {
    let params = function
        .parameters
        .iter()
        .map(|parameter| {
            let mut rendered = String::new();
            if parameter.rest {
                rendered.push_str("...");
            }
            rendered.push_str(parameter.name.as_deref().unwrap_or("_"));
            if parameter.optional {
                rendered.push('?');
            }
            rendered.push_str(": ");
            rendered.push_str(&render_type_expr_for_projected_surface(&parameter.ty)?);
            Some(rendered)
        })
        .collect::<Option<Vec<_>>>()?
        .join(", ");
    let return_type = function
        .return_type
        .as_deref()
        .and_then(render_type_expr_for_projected_surface)
        .unwrap_or_else(|| "void".to_string());
    Some(format!("({params}) => {return_type}"))
}
