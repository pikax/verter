use oxc_allocator::Allocator;
use verter_analysis::jsdoc::extract_jsdoc_near_offset;
use verter_analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_core::utils::oxc::vue::resolve_type::{
    resolve_external_type, ResolvedElements, ResolvedEmitSignature, ResolvedMemberVisibility,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedNativeProp {
    pub name: String,
    pub is_optional: bool,
    pub type_annotation: Option<String>,
    pub visibility: ResolvedMemberVisibility,
    pub span: verter_span::Span,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectedMacroSurfaces {
    pub native_props: Vec<ResolvedNativeProp>,
    pub props: Vec<AnalyzedPropField>,
    pub emits: Vec<AnalyzedEmitField>,
    pub slots: Vec<AnalyzedSlotField>,
}

pub fn project_macro_surfaces(
    source: Option<&str>,
    macro_kind: AnalyzedMacroKind,
    elements: &ResolvedElements,
) -> ProjectedMacroSurfaces {
    let native_props = collect_native_props(elements);

    match macro_kind {
        AnalyzedMacroKind::DefineProps
        | AnalyzedMacroKind::WithDefaults
        | AnalyzedMacroKind::DefineModel => {
            let props = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .map(|prop| {
                    let (description, tags) = member_jsdoc(source, prop.span);
                    AnalyzedPropField {
                        name: prop
                            .key_name
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        is_optional: prop.optional,
                        span: verter_span::Span::default(),
                        type_annotation: prop.type_text.clone(),
                        description,
                        tags,
                        resolution_source: verter_analysis::TypeResolutionSource::Rust,
                        resolution_error: None,
                    }
                })
                .collect();

            ProjectedMacroSurfaces {
                native_props,
                props,
                emits: Vec::new(),
                slots: Vec::new(),
            }
        }
        AnalyzedMacroKind::DefineEmits => {
            let emits = elements
                .emits
                .iter()
                .map(|emit| {
                    let (description, tags) = member_jsdoc(source, emit.span);
                    let payload_type = match &emit.signature {
                        ResolvedEmitSignature::Call { params_text } => {
                            if params_text.is_empty() {
                                None
                            } else {
                                Some(format!("[{}]", params_text))
                            }
                        }
                        ResolvedEmitSignature::Tuple { tuple_text } => Some(tuple_text.clone()),
                    };
                    AnalyzedEmitField {
                        name: emit.name.clone(),
                        span: verter_span::Span::default(),
                        payload_type,
                        description,
                        tags,
                    }
                })
                .collect();

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits,
                slots: Vec::new(),
            }
        }
        AnalyzedMacroKind::DefineSlots => {
            let slots = elements
                .props
                .iter()
                .filter(|prop| prop.visibility.is_public())
                .map(|prop| {
                    let name = prop
                        .key_name
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let (description, tags) = member_jsdoc(source, prop.span);
                    let (bindings, return_type) =
                        extract_slot_info_from_type_text(prop.type_text.as_deref());
                    AnalyzedSlotField {
                        name,
                        is_required: !prop.optional,
                        span: verter_span::Span::default(),
                        bindings,
                        return_type,
                        description,
                        tags,
                    }
                })
                .collect();

            ProjectedMacroSurfaces {
                native_props,
                props: Vec::new(),
                emits: Vec::new(),
                slots,
            }
        }
        _ => ProjectedMacroSurfaces {
            native_props,
            props: Vec::new(),
            emits: Vec::new(),
            slots: Vec::new(),
        },
    }
}

pub fn extract_slot_info_from_type_text(
    type_text: Option<&str>,
) -> (Vec<AnalyzedSlotFieldBinding>, Option<String>) {
    let Some(text) = type_text else {
        return (Vec::new(), None);
    };

    let return_type = if let Some(arrow_pos) = text.find("=>") {
        let ret = text[arrow_pos + 2..].trim();
        if ret.is_empty() {
            None
        } else {
            Some(ret.to_string())
        }
    } else if let Some(colon_pos) = text.rfind("):") {
        let ret = text[colon_pos + 2..].trim();
        if ret.is_empty() {
            None
        } else {
            Some(ret.to_string())
        }
    } else {
        None
    };

    let Some(obj_start) = text.find('{') else {
        return (Vec::new(), return_type);
    };

    let mut depth = 0;
    let mut obj_end = obj_start;
    for (index, ch) in text[obj_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    obj_end = obj_start + index + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return (Vec::new(), return_type);
    }

    let alloc = Allocator::new();
    let Some(resolved) = resolve_external_type(
        "_Bindings",
        &format!("export interface _Bindings {}", &text[obj_start..obj_end]),
        &alloc,
    ) else {
        return (Vec::new(), return_type);
    };

    let bindings = resolved
        .props
        .iter()
        .filter_map(|prop| {
            let name = prop.key_name.as_ref()?.clone();
            Some(AnalyzedSlotFieldBinding {
                name,
                type_annotation: prop.type_text.clone(),
                span: verter_span::Span::default(),
            })
        })
        .collect();

    (bindings, return_type)
}

fn collect_native_props(elements: &ResolvedElements) -> Vec<ResolvedNativeProp> {
    elements
        .props
        .iter()
        .map(|prop| ResolvedNativeProp {
            name: prop
                .key_name
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            is_optional: prop.optional,
            type_annotation: prop.type_text.clone(),
            visibility: prop.visibility,
            span: verter_span::Span::new(prop.span.start, prop.span.end),
        })
        .collect()
}

fn member_jsdoc(source: Option<&str>, span: verter_span::Span) -> (Option<String>, Vec<JsdocTag>) {
    let Some(source) = source else {
        return (None, Vec::new());
    };
    extract_jsdoc_near_offset(source, span.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::TypeResolutionSource;
    use verter_core::utils::oxc::vue::resolve_type::{ResolvedEmit, ResolvedProp};

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
    fn project_define_slots_extracts_bindings_and_return_type() {
        let elements = ResolvedElements {
            props: vec![prop(
                "default",
                false,
                ResolvedMemberVisibility::Public,
                Some("(props: { foo: string; bar?: number }) => VNode[]"),
                0,
            )],
            ..ResolvedElements::default()
        };

        let projected = project_macro_surfaces(None, AnalyzedMacroKind::DefineSlots, &elements);

        assert_eq!(projected.slots.len(), 1);
        assert_eq!(projected.slots[0].name, "default");
        assert_eq!(projected.slots[0].bindings.len(), 2);
        assert_eq!(projected.slots[0].bindings[0].name, "foo");
        assert_eq!(projected.slots[0].bindings[1].name, "bar");
        assert_eq!(projected.slots[0].return_type.as_deref(), Some("VNode[]"));
    }
}
