use oxc_allocator::Allocator;
use verter_analysis::jsdoc::extract_jsdoc_near_offset;
use verter_analysis::types::{
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField,
    AnalyzedSlotFieldBinding, JsdocTag,
};
use verter_compiler::utils::oxc::vue::resolve_type::{
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
                        type_annotation: raw_prop_type_text(source, prop),
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
                    let payload_type = raw_emit_payload_text(source, emit);
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
                .filter_map(|prop| {
                    let name = prop
                        .key_name
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    let (description, tags) = member_jsdoc(source, prop.span);
                    let raw_type_text = raw_prop_type_text(source, prop);
                    let (bindings, return_type) =
                        extract_slot_info_from_type_text(source, raw_type_text.as_deref());
                    let resolved_as_slot = prop.types.iter().any(|runtime| {
                        matches!(
                            runtime,
                            verter_compiler::utils::oxc::vue::resolve_type::RuntimeType::Function
                        )
                    });
                    if bindings.is_empty() && return_type.is_none() && !resolved_as_slot {
                        return None;
                    }
                    Some(AnalyzedSlotField {
                        name,
                        is_required: !prop.optional,
                        span: verter_span::Span::default(),
                        bindings,
                        return_type,
                        description,
                        tags,
                    })
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

pub fn project_macro_surfaces_from_expanded_text(
    macro_kind: AnalyzedMacroKind,
    expanded_text: &str,
) -> Option<ProjectedMacroSurfaces> {
    let expanded_text = expanded_text.trim();
    if expanded_text.is_empty() {
        return None;
    }

    let synthetic = format!("export type __VerterMacro = {expanded_text}");
    let alloc = Allocator::new();
    let resolved = resolve_external_type("__VerterMacro", &synthetic, &alloc)?;
    Some(project_macro_surfaces(None, macro_kind, &resolved))
}

pub fn project_macro_surfaces_from_source_type_name(
    source: &str,
    macro_kind: AnalyzedMacroKind,
    type_name: &str,
) -> Option<ProjectedMacroSurfaces> {
    let source = source.trim();
    if source.is_empty() {
        return None;
    }

    let alloc = Allocator::new();
    let resolved = resolve_external_type(type_name, source, &alloc)?;
    Some(project_macro_surfaces(Some(source), macro_kind, &resolved))
}

pub fn extract_slot_info_from_type_text(
    source: Option<&str>,
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

    let Some(binding_type_text) = extract_first_slot_param_type_text(text) else {
        return (Vec::new(), return_type);
    };

    let binding_type_text = binding_type_text.trim();
    if let Some(bindings) = extract_pick_slot_bindings(binding_type_text) {
        return (bindings, return_type);
    }

    let binding_declaration = if binding_type_text.starts_with('{') {
        format!("export interface _Bindings {binding_type_text}")
    } else {
        format!("export type _Bindings = {binding_type_text}")
    };
    let synthetic = source
        .filter(|source| !source.trim().is_empty())
        .map(|source| format!("{source}\n{binding_declaration}"))
        .unwrap_or(binding_declaration);

    let alloc = Allocator::new();
    let Some(resolved) = resolve_external_type("_Bindings", &synthetic, &alloc) else {
        return (Vec::new(), return_type);
    };

    let bindings = resolved
        .props
        .iter()
        .filter_map(|prop| {
            let name = prop.key_name.as_ref()?.clone();
            let type_annotation = if binding_type_text.starts_with('{') {
                prop.type_text.clone()
            } else {
                Some(symbolic_slot_binding_type_text(
                    binding_type_text,
                    name.as_str(),
                ))
            };
            Some(AnalyzedSlotFieldBinding {
                name,
                type_annotation,
                span: verter_span::Span::default(),
            })
        })
        .collect();

    (bindings, return_type)
}

fn extract_first_slot_param_type_text(text: &str) -> Option<&str> {
    let open = text.find('(')?;
    let close = find_matching_delimiter(text, open, '(', ')')?;
    let params = split_top_level_segments(&text[open + 1..close], ',');
    let first = params.first()?.trim();
    let colon = find_top_level_char(first, ':')?;
    let ty = first[colon + 1..].trim();
    (!ty.is_empty()).then_some(ty)
}

fn symbolic_slot_binding_type_text(binding_type_text: &str, binding_name: &str) -> String {
    simplify_pick_slot_binding_type_text(binding_type_text, binding_name)
        .unwrap_or_else(|| format!("{binding_type_text}['{binding_name}']"))
}

fn extract_pick_slot_bindings(binding_type_text: &str) -> Option<Vec<AnalyzedSlotFieldBinding>> {
    let text = binding_type_text.trim();
    if !text.starts_with("Pick<") || !text.ends_with('>') {
        return None;
    }

    let args = split_top_level_segments(&text["Pick<".len()..text.len() - 1], ',');
    if args.len() != 2 {
        return None;
    }

    let object = args[0].trim();
    let keys = split_top_level_segments(args[1].trim(), '|');
    let mut bindings = Vec::new();
    for key in keys {
        let key = key.trim();
        let Some(name) = extract_string_literal_name(key) else {
            return None;
        };
        bindings.push(AnalyzedSlotFieldBinding {
            name,
            type_annotation: Some(format!("{object}[{key}]")),
            span: verter_span::Span::default(),
        });
    }

    (!bindings.is_empty()).then_some(bindings)
}

fn simplify_pick_slot_binding_type_text(
    binding_type_text: &str,
    binding_name: &str,
) -> Option<String> {
    let text = binding_type_text.trim();
    if !text.starts_with("Pick<") || !text.ends_with('>') {
        return None;
    }

    let args = split_top_level_segments(&text["Pick<".len()..text.len() - 1], ',');
    if args.len() != 2 {
        return None;
    }

    let object = args[0].trim();
    let key = args[1].trim();
    let single_quoted = format!("'{binding_name}'");
    let double_quoted = format!("\"{binding_name}\"");
    if key == single_quoted || key == double_quoted {
        return Some(format!("{object}[{key}]"));
    }

    None
}

fn extract_string_literal_name(text: &str) -> Option<String> {
    let text = text.trim();
    if text.len() >= 2
        && ((text.starts_with('\'') && text.ends_with('\''))
            || (text.starts_with('"') && text.ends_with('"')))
    {
        return Some(text[1..text.len() - 1].to_string());
    }
    None
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
            type_annotation: raw_prop_type_text(None, prop),
            visibility: prop.visibility,
            span: verter_span::Span::new(prop.span.start, prop.span.end),
        })
        .collect()
}

fn raw_prop_type_text(
    source: Option<&str>,
    prop: &verter_compiler::utils::oxc::vue::resolve_type::ResolvedProp,
) -> Option<String> {
    prop.type_span
        .and_then(|span| slice_source_span(source, span))
        .or_else(|| prop.type_text.clone())
}

fn raw_emit_payload_text(
    source: Option<&str>,
    emit: &verter_compiler::utils::oxc::vue::resolve_type::ResolvedEmit,
) -> Option<String> {
    slice_source_span(source, emit.span)
        .and_then(|text| raw_emit_payload_text_from_source(&text, &emit.signature))
        .or_else(|| match &emit.signature {
            ResolvedEmitSignature::Call { params_text } => {
                if params_text.is_empty() {
                    None
                } else {
                    Some(format!("[{}]", params_text))
                }
            }
            ResolvedEmitSignature::Tuple { tuple_text } => Some(tuple_text.clone()),
        })
}

fn raw_emit_payload_text_from_source(
    signature_text: &str,
    signature: &ResolvedEmitSignature,
) -> Option<String> {
    match signature {
        ResolvedEmitSignature::Tuple { .. } => {
            let colon = find_top_level_char(signature_text, ':')?;
            Some(trim_trailing_type_text(&signature_text[colon + 1..]))
        }
        ResolvedEmitSignature::Call { .. } => {
            let open = signature_text.find('(')?;
            let close = find_matching_delimiter(signature_text, open, '(', ')')?;
            let params = split_top_level_segments(&signature_text[open + 1..close], ',');
            let payload_params: Vec<_> = params
                .into_iter()
                .skip(1)
                .map(|param| param.trim().to_string())
                .filter(|param| !param.is_empty())
                .collect();
            Some(format!("[{}]", payload_params.join(", ")))
        }
    }
}

fn slice_source_span(source: Option<&str>, span: verter_span::Span) -> Option<String> {
    let source = source?;
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let text = trim_trailing_type_text(&source[start..end]);
    (!text.is_empty()).then_some(text)
}

fn trim_trailing_type_text(text: &str) -> String {
    text.trim()
        .trim_end_matches(|ch: char| ch == ';' || ch == ',')
        .trim()
        .to_string()
}

fn find_top_level_char(text: &str, needle: char) -> Option<usize> {
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut angle_depth = 0i32;
    let mut in_string = false;
    let mut string_delim = '\0';
    let mut escape = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = true;
                string_delim = ch;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '<' => angle_depth += 1,
            '>' => angle_depth -= 1,
            _ if ch == needle
                && paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0
                && angle_depth == 0 =>
            {
                return Some(index);
            }
            _ => {}
        }
    }

    None
}

fn find_matching_delimiter(
    text: &str,
    open_index: usize,
    open: char,
    close: char,
) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut string_delim = '\0';
    let mut escape = false;

    for (index, ch) in text[open_index..].char_indices() {
        let absolute = open_index + index;
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = true;
                string_delim = ch;
            }
            _ if ch == open => depth += 1,
            _ if ch == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(absolute);
                }
            }
            _ => {}
        }
    }

    None
}

fn split_top_level_segments(text: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;
    let mut brace_depth = 0i32;
    let mut angle_depth = 0i32;
    let mut in_string = false;
    let mut string_delim = '\0';
    let mut escape = false;

    for (index, ch) in text.char_indices() {
        if in_string {
            if escape {
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                continue;
            }
            if ch == string_delim {
                in_string = false;
            }
            continue;
        }

        match ch {
            '\'' | '"' | '`' => {
                in_string = true;
                string_delim = ch;
            }
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '<' => angle_depth += 1,
            '>' => angle_depth -= 1,
            _ if ch == separator
                && paren_depth == 0
                && bracket_depth == 0
                && brace_depth == 0
                && angle_depth == 0 =>
            {
                parts.push(text[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = text[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
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
    use verter_compiler::utils::oxc::vue::resolve_type::{ResolvedEmit, ResolvedProp};

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
    fn project_define_props_prefers_raw_source_type_span_text() {
        let source = "interface Props { type?: SingleOrMultipleType }";
        let type_start = source.find("SingleOrMultipleType").unwrap() as u32;
        let prop_start = source.find("type?").unwrap() as u32;
        let elements = ResolvedElements {
            props: vec![prop_with_type_span(
                "type",
                true,
                ResolvedMemberVisibility::Public,
                Some("SingleOrMultipleType | undefined"),
                verter_span::Span::new(prop_start, source.len() as u32 - 2),
                verter_span::Span::new(prop_start, prop_start + 4),
                verter_span::Span::new(
                    type_start,
                    type_start + "SingleOrMultipleType".len() as u32,
                ),
            )],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineProps, &elements);

        assert_eq!(
            projected.props[0].type_annotation.as_deref(),
            Some("SingleOrMultipleType")
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
            }],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineEmits, &elements);

        assert_eq!(
            projected.emits[0].payload_type.as_deref(),
            Some("[value: (T extends 'single' ? string : string[]) | undefined]")
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

    #[test]
    fn project_define_slots_preserves_symbolic_binding_types_for_pick_params() {
        let source = r#"
type CalendarCellTriggerProps = { day: string; month: number }
export interface Slots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#;
        let elements = ResolvedElements {
            props: vec![prop(
                "day",
                true,
                ResolvedMemberVisibility::Public,
                Some("(props: Pick<CalendarCellTriggerProps, 'day'>) => any"),
                0,
            )],
            ..ResolvedElements::default()
        };

        let projected =
            project_macro_surfaces(Some(source), AnalyzedMacroKind::DefineSlots, &elements);

        assert_eq!(projected.slots.len(), 1);
        assert_eq!(projected.slots[0].bindings.len(), 1);
        assert_eq!(projected.slots[0].bindings[0].name, "day");
        assert_eq!(
            projected.slots[0].bindings[0].type_annotation.as_deref(),
            Some("CalendarCellTriggerProps['day']")
        );
    }

    #[test]
    fn project_expanded_text_define_emits_preserves_conditional_payload_text() {
        let projected = project_macro_surfaces_from_expanded_text(
            AnalyzedMacroKind::DefineEmits,
            "{ 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined] }",
        )
        .expect("expanded emits text should project");

        assert_eq!(projected.emits.len(), 1);
        assert_eq!(
            projected.emits[0].payload_type.as_deref(),
            Some("[value: (T extends 'single' ? string : string[]) | undefined]")
        );
    }

    #[test]
    fn project_local_source_define_slots_preserves_symbolic_pick_binding() {
        let source = r#"
export interface CalendarCellTriggerProps {
  day: Date
  month: number
}

export interface CalendarSlots {
  day?: (props: Pick<CalendarCellTriggerProps, 'day'>) => any
}
"#;

        let projected = project_macro_surfaces_from_source_type_name(
            source,
            AnalyzedMacroKind::DefineSlots,
            "CalendarSlots",
        )
        .expect("local source projection should succeed");

        assert_eq!(projected.slots.len(), 1);
        assert_eq!(projected.slots[0].bindings.len(), 1);
        assert_eq!(
            projected.slots[0].bindings[0].type_annotation.as_deref(),
            Some("CalendarCellTriggerProps['day']")
        );
    }

    #[test]
    fn project_define_slots_ignores_non_callable_helper_members() {
        let elements = ResolvedElements {
            props: vec![
                prop(
                    "default",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("(props: { item: string }) => any"),
                    0,
                ),
                prop(
                    "appConfig",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("{ ui?: { variant: string } }"),
                    0,
                ),
                prop(
                    "slots",
                    false,
                    ResolvedMemberVisibility::Public,
                    Some("{ leading?: string; trailing?: string }"),
                    0,
                ),
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

    #[test]
    fn project_local_source_define_props_does_not_resolve_imported_utility_heritage() {
        let source = r#"
import type { ButtonHTMLAttributes, AnchorHTMLAttributes } from './types/html'

interface RouterLinkProps {
  replace?: boolean
}

interface NuxtLinkProps extends Omit<RouterLinkProps, 'to'> {
  to?: string
  href?: string
}

export interface LinkProps extends NuxtLinkProps, Omit<ButtonHTMLAttributes, 'type' | 'disabled'>, Omit<AnchorHTMLAttributes, 'href' | 'target' | 'rel' | 'type'> {
  as?: any
  type?: ButtonHTMLAttributes['type']
  disabled?: boolean
}
"#;

        let projected = project_macro_surfaces_from_source_type_name(
            source,
            AnalyzedMacroKind::DefineProps,
            "LinkProps",
        )
        .expect("local source projection should succeed");

        let names: Vec<_> = projected
            .props
            .iter()
            .map(|prop| prop.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["as", "type", "disabled", "to", "href", "replace"]
        );
    }

    #[test]
    fn project_local_source_define_props_preserves_jsdoc_and_raw_types_after_vue_ignore_heritage() {
        let source = r#"
interface NuxtLinkProps {
  to?: string
}

interface ButtonHTMLAttributes {
  type?: 'button' | 'submit'
}

interface AnchorHTMLAttributes {
  href?: string
}

export interface LinkProps extends NuxtLinkProps, /** @vue-ignore */ Omit<ButtonHTMLAttributes, 'type'>, /** @vue-ignore */ Omit<AnchorHTMLAttributes, 'href'> {
  /** Force the link to be active independent of the current route. */
  active?: boolean
  /** Class to apply when the link is active */
  activeClass?: string
}
"#;

        let projected = project_macro_surfaces_from_source_type_name(
            source,
            AnalyzedMacroKind::DefineProps,
            "LinkProps",
        )
        .expect("local source projection should succeed");

        let active = projected
            .props
            .iter()
            .find(|prop| prop.name == "active")
            .expect("active prop should be projected");
        assert_eq!(active.type_annotation.as_deref(), Some("boolean"));
        assert_eq!(
            active.description.as_deref(),
            Some("Force the link to be active independent of the current route.")
        );

        let active_class = projected
            .props
            .iter()
            .find(|prop| prop.name == "activeClass")
            .expect("activeClass prop should be projected");
        assert_eq!(active_class.type_annotation.as_deref(), Some("string"));
        assert_eq!(
            active_class.description.as_deref(),
            Some("Class to apply when the link is active")
        );
    }
}
