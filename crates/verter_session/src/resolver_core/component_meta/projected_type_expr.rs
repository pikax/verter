use rustc_hash::FxHashSet;
use verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements;
use verter_semantic::analysis::types::AnalyzedMacroKind;
use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr, TypeExprScope,
};

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

/// Build a `TypeExpr::Object` (or `TypeExpr::Function`-bearing object) from a
/// projected macro surface using the typed sidecars populated by the producer
/// (`surface_projector::project_macro_surfaces` and the W1.1b bridge).
///
/// The fast path consumes `ProjectedMacroSurfaces.{props,emits,slots}_expr`
/// when the producer was able to synthesise the aggregate (every per-field
/// `*_expr` populated AND the owner canonical was supplied). When the
/// aggregate is unavailable the function falls back to walking the per-field
/// typed sidecars (`type_expr` / `payload_expr` / `return_expr` /
/// `binding_expr`) and constructs `TypeExpr` nodes directly. No raw-text
/// reparse path remains.
pub fn projected_macro_surfaces_to_type_expr(
    macro_kind: AnalyzedMacroKind,
    projected: &ProjectedMacroSurfaces,
) -> verter_type_expr::TypeExpr {
    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => {
            if let Some(aggregate) = projected.props_expr.clone() {
                return aggregate;
            }
            let properties = projected
                .props
                .iter()
                .map(|prop| {
                    // W0.2 invariant: AnalyzedPropField.type_expr is populated
                    // by the analyzer / surface projector. A None here is a
                    // producer-chain bug; panic loudly rather than corrupting
                    // the published surface with TypeExpr::Unknown.
                    let ty = prop.type_expr.clone().expect(
                        "AnalyzedPropField.type_expr populated by analyzer (W0.2 invariant)",
                    );
                    ObjectMember::Property(ObjectProperty {
                        name: prop.name.clone(),
                        ty,
                        optional: prop.is_optional,
                        readonly: false,
                    })
                })
                .collect::<Vec<_>>();
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
        }
        AnalyzedMacroKind::DefineEmits => {
            if let Some(aggregate) = projected.emits_expr.clone() {
                return aggregate;
            }
            let properties = projected
                .emits
                .iter()
                .map(|emit| {
                    let ty = emit.payload_expr.clone().expect(
                        "AnalyzedEmitField.payload_expr populated by analyzer (W0.2 invariant)",
                    );
                    ObjectMember::Property(ObjectProperty {
                        name: emit.name.clone(),
                        ty,
                        optional: false,
                        readonly: false,
                    })
                })
                .collect::<Vec<_>>();
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
        }
        AnalyzedMacroKind::DefineSlots => {
            if let Some(aggregate) = projected.slots_expr.clone() {
                return aggregate;
            }
            let properties = projected
                .slots
                .iter()
                .map(|slot| {
                    let return_ty = slot
                        .return_expr
                        .clone()
                        .expect("AnalyzedSlotField.return_expr populated by analyzer (W0.2 invariant)");
                    let binding_props = slot
                        .bindings
                        .iter()
                        .map(|binding| {
                            let ty = binding
                                .binding_expr
                                .clone()
                                .expect("AnalyzedSlotFieldBinding.binding_expr populated by analyzer (W0.2 invariant)");
                            ObjectMember::Property(ObjectProperty {
                                name: binding.name.clone(),
                                ty,
                                optional: false,
                                readonly: false,
                            })
                        })
                        .collect::<Vec<_>>();
                    let parameters = if binding_props.is_empty() {
                        Vec::new()
                    } else {
                        vec![FunctionParam {
                            name: Some("props".to_string()),
                            ty: TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                                properties: binding_props,
                            })),
                            optional: false,
                            rest: false,
                        }]
                    };
                    let function = TypeExpr::Function(std::sync::Arc::new(FunctionExpr {
                        parameters,
                        return_type: Some(std::sync::Arc::new(return_ty)),
                        type_parameters: Vec::new(),
                    }));
                    ObjectMember::Property(ObjectProperty {
                        name: slot.name.clone(),
                        ty: function,
                        optional: !slot.is_required,
                        readonly: false,
                    })
                })
                .collect::<Vec<_>>();
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
        }
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                properties: Vec::new(),
            }))
        }
    }
}

pub(crate) fn project_macro_surfaces_from_expanded_shape(
    macro_kind: AnalyzedMacroKind,
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
    owner_canonical: Option<&str>,
) -> ProjectedMacroSurfaces {
    let owner_scope = owner_canonical.map(TypeExprScope::new);
    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: shape
                .properties
                .iter()
                .map(|property| {
                    let type_expr = Some(property.ty.clone());
                    let type_expr_scope = owner_scope.clone();
                    debug_assert_eq!(
                        type_expr.is_some(),
                        type_expr_scope.is_some(),
                        "AnalyzedPropField (expanded-shape) type_expr/type_expr_scope pairing violated for prop `{}`",
                        property.name,
                    );
                    verter_semantic::analysis::AnalyzedPropField {
                        name: property.name.clone(),
                        is_optional: property.optional,
                        span: verter_span::Span::default(),
                        type_annotation: render_type_expr_for_projected_surface(&property.ty),
                        description: None,
                        tags: Vec::new(),
                        resolution_source:
                            verter_semantic::analysis::types::TypeResolutionSource::Rust,
                        resolution_error: None,
                        type_expr,
                        type_expr_scope,
                        declared_in_macro_type_arg: property.declared_in_macro_type_arg,
                    }
                })
                .collect(),
            emits: Vec::new(),
            slots: Vec::new(),
            ..Default::default()
        },
        AnalyzedMacroKind::DefineEmits => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: Vec::new(),
            emits: projected_emit_fields_from_shape(shape, owner_scope.as_ref()),
            slots: Vec::new(),
            ..Default::default()
        },
        AnalyzedMacroKind::DefineSlots => ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: Vec::new(),
            emits: Vec::new(),
            slots: projected_slot_fields_from_shape(shape, owner_scope.as_ref()),
            ..Default::default()
        },
        AnalyzedMacroKind::DefineExpose | AnalyzedMacroKind::DefineOptions => {
            ProjectedMacroSurfaces::default()
        }
    }
}

fn projected_emit_fields_from_shape(
    shape: &verter_semantic::analysis::type_expand::ExpandedObjectShape,
    owner_scope: Option<&TypeExprScope>,
) -> Vec<verter_semantic::analysis::AnalyzedEmitField> {
    use verter_type_expr::{LiteralValue, TupleElement, TypeExpr};

    let mut emits = shape
        .properties
        .iter()
        .map(|property| {
            let payload_expr = Some(property.ty.clone());
            let payload_expr_scope = owner_scope.cloned();
            debug_assert_eq!(
                payload_expr.is_some(),
                payload_expr_scope.is_some(),
                "AnalyzedEmitField (expanded-shape, property-style) payload_expr/payload_expr_scope pairing violated for emit `{}`",
                property.name,
            );
            verter_semantic::analysis::AnalyzedEmitField {
                name: property.name.clone(),
                span: verter_span::Span::default(),
                payload_type: event_payload_raw_signature_from_type_expr_for_projected_surface(
                    &property.ty,
                ),
                description: None,
                tags: Vec::new(),
                payload_expr,
                payload_expr_scope,
            }
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
        let payload_expr_for_signature = Some(payload.clone());
        let payload_expr_scope_for_signature = owner_scope.cloned();
        debug_assert_eq!(
            payload_expr_for_signature.is_some(),
            payload_expr_scope_for_signature.is_some(),
            "AnalyzedEmitField (expanded-shape, call-signature) payload_expr/payload_expr_scope pairing violated",
        );
        match &first.ty {
            TypeExpr::Literal(LiteralValue::String(name)) => {
                emits.push(verter_semantic::analysis::AnalyzedEmitField {
                    name: name.clone(),
                    span: verter_span::Span::default(),
                    payload_type: payload_type.clone(),
                    description: None,
                    tags: Vec::new(),
                    payload_expr: payload_expr_for_signature.clone(),
                    payload_expr_scope: payload_expr_scope_for_signature.clone(),
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
                        payload_expr: payload_expr_for_signature.clone(),
                        payload_expr_scope: payload_expr_scope_for_signature.clone(),
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
    owner_scope: Option<&TypeExprScope>,
) -> Vec<verter_semantic::analysis::AnalyzedSlotField> {
    shape
        .properties
        .iter()
        .filter_map(|property| {
            // Typed-IR-only: walk `property.ty` directly. Each expanded
            // shape's property carries the lowered typed form for a
            // slot's function signature.
            let (bindings, return_type) =
                crate::resolver_core::surface_projector::slot_info_from_type_expr(&property.ty);
            // Populate `return_expr` (typed) from the function-type's typed
            // return form. The shape's `property.ty` is the function-type
            // produced by the upstream expander, so the return type is
            // recoverable from it without reparsing the display string.
            // Pairing invariant:
            // `return_expr.is_some() <=> return_expr_scope.is_some()` —
            // when `return_expr` populates, paint scope from the owner.
            let return_expr =
                crate::resolver_core::surface_projector::slot_return_expr_from_function_type(
                    &property.ty,
                );
            let return_expr_scope = return_expr.as_ref().and(owner_scope.cloned());
            debug_assert_eq!(
                return_expr.is_some(),
                return_expr_scope.is_some(),
                "AnalyzedSlotField (expanded-shape) return_expr/return_expr_scope pairing violated for slot `{}`",
                property.name,
            );
            if bindings.is_empty() && return_type.is_none() && return_expr.is_none() {
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
                return_expr,
                return_expr_scope,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use verter_semantic::analysis::types::{AnalyzedSlotField, AnalyzedSlotFieldBinding};
    use verter_type_expr::{LiteralValue, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName};

    /// Discriminating regression test for the W2.2 cutover: the slot
    /// branch of `projected_macro_surfaces_to_type_expr` constructs
    /// `TypeExpr::Function` directly from the typed sidecars
    /// (`AnalyzedSlotField.return_expr`, `AnalyzedSlotFieldBinding.binding_expr`),
    /// not from a `format!("(props: {{ {bindings} }}) => {return_type}")`
    /// reparse round-trip.
    ///
    /// The fixture forces a deliberate mismatch between the typed
    /// `binding_expr` (Primitive(Number)) and the display
    /// `type_annotation` ("BindingAlias"), and between the typed
    /// `return_expr` (Primitive(Boolean)) and the display
    /// `return_type` ("ReturnAlias"). Pre-W2.2 the function reparsed the
    /// synthesised text and yielded `Ref { name: "BindingAlias" }` /
    /// `Ref { name: "ReturnAlias" }`. Post-W2.2 it reads the typed
    /// sidecars and yields `Primitive(Number)` / `Primitive(Boolean)`.
    ///
    /// The aggregate `slots_expr` is left `None` so the per-field
    /// fallback path runs (i.e. the path the cutover deletes its
    /// text-reparse calls from). This is the discriminator.
    #[test]
    fn projected_slot_function_reads_typed_sidecars_not_synthesised_text() {
        let projected = ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: Vec::new(),
            emits: Vec::new(),
            slots: vec![AnalyzedSlotField {
                name: "default".to_string(),
                is_required: true,
                span: verter_span::Span::default(),
                bindings: vec![AnalyzedSlotFieldBinding {
                    name: "label".to_string(),
                    type_annotation: Some("BindingAlias".to_string()),
                    span: verter_span::Span::default(),
                    binding_expr: Some(TypeExpr::Primitive(PrimitiveName::Number)),
                    binding_expr_scope: None,
                }],
                return_type: Some("ReturnAlias".to_string()),
                return_expr: Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
                return_expr_scope: None,
                description: None,
                tags: Vec::new(),
            }],
            // Aggregate left None to force the per-field typed walk —
            // the path the W2.2 cutover replaces.
            slots_expr: None,
            slots_expr_scope: None,
            ..Default::default()
        };

        let result = projected_macro_surfaces_to_type_expr(
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineSlots,
            &projected,
        );

        let TypeExpr::Object(object) = &result else {
            panic!("expected Object root, got {result:?}");
        };
        assert_eq!(object.properties.len(), 1);
        let ObjectMember::Property(prop) = &object.properties[0] else {
            panic!("expected Property member");
        };
        assert_eq!(prop.name, "default");

        let TypeExpr::Function(function) = &prop.ty else {
            panic!("expected Function ty for slot, got {:?}", prop.ty);
        };
        assert_eq!(function.parameters.len(), 1);
        let param = &function.parameters[0];
        assert_eq!(param.name.as_deref(), Some("props"));

        let TypeExpr::Object(props_object) = &param.ty else {
            panic!("expected Object props parameter, got {:?}", param.ty);
        };
        assert_eq!(props_object.properties.len(), 1);
        let ObjectMember::Property(binding_prop) = &props_object.properties[0] else {
            panic!("expected Property binding");
        };
        assert_eq!(binding_prop.name, "label");
        // POST-W2.2 discriminator: the binding ty is the typed sidecar
        // (Primitive(Number)), NOT the result of parsing
        // type_annotation ("BindingAlias" → Ref { name: "BindingAlias" }).
        assert!(
            matches!(&binding_prop.ty, TypeExpr::Primitive(PrimitiveName::Number)),
            "post-W2.2 binding ty must be the typed sidecar Primitive(Number); got {:?}",
            binding_prop.ty
        );
        // Negative assertion: the previous reparse-from-string would have
        // produced Ref { name: "BindingAlias" }.
        assert!(
            !matches!(
                &binding_prop.ty,
                TypeExpr::Ref { name, .. } if name.as_ref() == "BindingAlias"
            ),
            "pre-W2.2 reparse result must not appear: binding ty {:?}",
            binding_prop.ty
        );

        // POST-W2.2 discriminator: the function return type is the
        // typed return_expr (Primitive(Boolean)), NOT the parsed
        // "ReturnAlias" identifier.
        let return_ty = function
            .return_type
            .as_deref()
            .expect("post-W2.2 function should carry the typed return_expr");
        assert!(
            matches!(return_ty, TypeExpr::Primitive(PrimitiveName::Boolean)),
            "post-W2.2 return ty must be the typed sidecar Primitive(Boolean); got {return_ty:?}"
        );
        assert!(
            !matches!(
                return_ty,
                TypeExpr::Ref { name, .. } if name.as_ref() == "ReturnAlias"
            ),
            "pre-W2.2 reparse result must not appear: return ty {return_ty:?}"
        );
    }

    /// Companion test pinning the props branch of the same cutover:
    /// when the aggregate `props_expr` is None, the per-field walk
    /// reads `AnalyzedPropField.type_expr` directly. Pre-W2.2 reparsed
    /// `type_annotation` ("PropAlias") and produced
    /// `Ref { name: "PropAlias" }`; post-W2.2 reads the typed sidecar
    /// (`Literal(String("done"))`).
    #[test]
    fn projected_props_read_typed_sidecars_not_type_annotation_text() {
        use verter_semantic::analysis::types::AnalyzedPropField;
        use verter_semantic::analysis::TypeResolutionSource;

        let projected = ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: vec![AnalyzedPropField {
                name: "kind".to_string(),
                is_optional: false,
                span: verter_span::Span::default(),
                type_annotation: Some("PropAlias".to_string()),
                description: None,
                tags: Vec::new(),
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                type_expr: Some(TypeExpr::Literal(LiteralValue::String("done".to_string()))),
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emits: Vec::new(),
            slots: Vec::new(),
            props_expr: None,
            props_expr_scope: None,
            ..Default::default()
        };

        let result = projected_macro_surfaces_to_type_expr(
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps,
            &projected,
        );

        let TypeExpr::Object(object) = &result else {
            panic!("expected Object root, got {result:?}");
        };
        assert_eq!(object.properties.len(), 1);
        let ObjectMember::Property(prop) = &object.properties[0] else {
            panic!("expected Property member");
        };
        assert_eq!(prop.name, "kind");
        assert!(
            matches!(
                &prop.ty,
                TypeExpr::Literal(LiteralValue::String(value)) if value == "done"
            ),
            "post-W2.2 prop ty must be the typed sidecar literal 'done'; got {:?}",
            prop.ty
        );
        assert!(
            !matches!(
                &prop.ty,
                TypeExpr::Ref { name, .. } if name.as_ref() == "PropAlias"
            ),
            "pre-W2.2 reparse-from-type_annotation must not appear: prop ty {:?}",
            prop.ty
        );
    }

    /// Aggregate `props_expr`-fast-path test: when the producer has
    /// supplied an aggregate, the function clones it verbatim rather
    /// than rebuilding from per-field walk. Discriminator: the
    /// per-field `type_expr` is deliberately Unknown but the aggregate
    /// is the authoritative shape.
    #[test]
    fn projected_props_aggregate_fast_path_clones_aggregate_verbatim() {
        use verter_semantic::analysis::types::AnalyzedPropField;
        use verter_semantic::analysis::TypeResolutionSource;

        let aggregate = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: "kind".to_string(),
                ty: TypeExpr::Literal(LiteralValue::String("authoritative".to_string())),
                optional: false,
                readonly: false,
            })],
        }));

        let projected = ProjectedMacroSurfaces {
            native_props: Vec::new(),
            props: vec![AnalyzedPropField {
                name: "kind".to_string(),
                is_optional: false,
                span: verter_span::Span::default(),
                type_annotation: None,
                description: None,
                tags: Vec::new(),
                resolution_source: TypeResolutionSource::Rust,
                resolution_error: None,
                // Per-field typed sidecar deliberately Unknown — the
                // aggregate is the authoritative shape.
                type_expr: Some(TypeExpr::Unknown {
                    raw: "unknown".to_string(),
                }),
                type_expr_scope: None,
                declared_in_macro_type_arg: false,
            }],
            emits: Vec::new(),
            slots: Vec::new(),
            props_expr: Some(aggregate.clone()),
            props_expr_scope: None,
            ..Default::default()
        };

        let result = projected_macro_surfaces_to_type_expr(
            verter_semantic::analysis::types::AnalyzedMacroKind::DefineProps,
            &projected,
        );

        assert_eq!(result, aggregate);
    }
}

#[cfg(test)]
mod slot_return_expr_tests {
    use super::*;
    use std::sync::Arc;
    use verter_semantic::analysis::type_expand::{ExpandedObjectShape, ExpandedProperty};
    use verter_semantic::analysis::AnalyzedMacroKind;
    use verter_type_expr::{FunctionExpr, PrimitiveName, TypeExpr};

    /// Discriminating test for M3 — `projected_slot_fields_from_shape`
    /// MUST populate `AnalyzedSlotField.return_expr` from the function
    /// type's typed return form. Pre-M3 fix the field was hardcoded to
    /// `None`, silently discarding the typed return form.
    ///
    /// The fixture constructs an `ExpandedObjectShape` whose slot
    /// property has type `() => Element`. Post-M3 fix the projected
    /// `AnalyzedSlotField.return_expr` is `Some(Ref { name: "Element", ... })`
    /// AND `return_expr_scope` is populated with the owner canonical_id.
    /// Pre-fix `return_expr` is `None` and the assertion FAILS.
    #[test]
    fn projected_slot_fields_populate_return_expr_from_typed_function() {
        let return_ty = TypeExpr::Ref {
            name: Arc::from("Element"),
            type_arguments: Arc::from(Vec::new().as_slice()),
        };
        let func = TypeExpr::Function(Arc::new(FunctionExpr {
            parameters: Vec::new(),
            return_type: Some(Arc::new(return_ty.clone())),
            type_parameters: Vec::new(),
        }));

        let shape = ExpandedObjectShape {
            properties: vec![ExpandedProperty {
                name: "default".to_string(),
                ty: func,
                optional: false,
                readonly: false,
                declared_in_macro_type_arg: false,
                carrier_provenance: None,
            }],
            index_signatures: Vec::new(),
            call_signatures: Vec::new(),
        };

        let projected = project_macro_surfaces_from_expanded_shape(
            AnalyzedMacroKind::DefineSlots,
            &shape,
            Some("/src/App.vue"),
        );

        assert_eq!(projected.slots.len(), 1);
        let slot = &projected.slots[0];
        assert_eq!(slot.name, "default");
        assert_eq!(
            slot.return_expr.as_ref(),
            Some(&return_ty),
            "return_expr must be the typed return form, NOT None — pre-M3 fix \
             hardcoded None, silently discarding the typed return shape."
        );
        assert!(
            slot.return_expr_scope.is_some(),
            "return_expr_scope MUST be populated when return_expr is — pairing invariant"
        );
        assert_eq!(
            slot.return_expr_scope.as_ref().map(|s| s.as_str()),
            Some("/src/App.vue"),
            "scope must reflect the owner canonical_id passed to the projector"
        );
    }

    /// Verify that when the slot property is NOT a function type, the
    /// projector correctly returns `return_expr: None` (the function-type
    /// helper drops to None for non-function inputs), AND that the slot
    /// is filtered out entirely if all three (bindings, return_type,
    /// return_expr) are empty.
    #[test]
    fn projected_slot_fields_skip_non_function_properties() {
        // A non-function property — `default: string` — has no slot
        // signature to extract. The filter_map should drop it.
        let shape = ExpandedObjectShape {
            properties: vec![ExpandedProperty {
                name: "default".to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
                declared_in_macro_type_arg: false,
                carrier_provenance: None,
            }],
            index_signatures: Vec::new(),
            call_signatures: Vec::new(),
        };

        let projected = project_macro_surfaces_from_expanded_shape(
            AnalyzedMacroKind::DefineSlots,
            &shape,
            Some("/src/App.vue"),
        );

        assert!(
            projected.slots.is_empty(),
            "non-function slot property must be filtered out"
        );
    }
}
