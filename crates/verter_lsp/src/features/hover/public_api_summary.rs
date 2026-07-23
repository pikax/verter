use super::*;

pub(super) fn parse_public_api_handler_props(code: &str) -> HashMap<String, String> {
    parse_public_api_fields(code)
        .into_iter()
        .filter_map(|(name, value)| {
            if name.starts_with("on") && value.contains("=>") {
                Some((name, value))
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn parse_public_api_props(code: &str) -> HashMap<String, String> {
    parse_public_api_fields(code)
        .into_iter()
        .filter_map(|(name, value)| {
            if name.starts_with("on") || name.starts_with('$') {
                return None;
            }
            Some((name, value))
        })
        .collect()
}

fn parse_public_api_fields(code: &str) -> Vec<(String, String)> {
    let Some(props_start) = code.find("$props:") else {
        return Vec::new();
    };
    let props_slice = &code[props_start + "$props:".len()..];
    let mut fields = Vec::new();
    let mut brace_cursor = 0usize;
    while let Some(rel) = props_slice[brace_cursor..].find('{') {
        let open = brace_cursor + rel;
        let Some(close) = find_matching_delimiter(props_slice, open, '{', '}') else {
            break;
        };
        let block = &props_slice[open + 1..close];
        fields.extend(parse_type_literal_fields(block));
        brace_cursor = close + 1;
    }
    fields
}

fn parse_type_literal_fields(block: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let bytes = block.as_bytes();
    let mut start = 0usize;
    let mut depth_paren = 0u32;
    let mut depth_bracket = 0u32;
    let mut depth_brace = 0u32;
    let mut idx = 0usize;

    while idx < bytes.len() {
        match bytes[idx] {
            b'(' => depth_paren += 1,
            b')' => depth_paren = depth_paren.saturating_sub(1),
            b'[' => depth_bracket += 1,
            b']' => depth_bracket = depth_bracket.saturating_sub(1),
            b'{' => depth_brace += 1,
            b'}' => depth_brace = depth_brace.saturating_sub(1),
            b';' if depth_paren == 0 && depth_bracket == 0 && depth_brace == 0 => {
                if let Some(field) = parse_field_entry(block[start..idx].trim()) {
                    fields.push(field);
                }
                start = idx + 1;
            }
            _ => {}
        }
        idx += 1;
    }

    if let Some(field) = parse_field_entry(block[start..].trim()) {
        fields.push(field);
    }

    fields
}

fn parse_field_entry(field: &str) -> Option<(String, String)> {
    let trimmed = field.trim();
    if trimmed.is_empty() || trimmed.starts_with("/**") {
        return None;
    }

    let separator = find_field_separator(trimmed)?;
    let raw_name = trimmed[..separator].trim();
    let raw_value = trimmed[separator + 1..].trim();
    let raw_name = raw_name.trim_end_matches('?').trim();
    let name = raw_name
        .strip_prefix('"')
        .and_then(|name| name.strip_suffix('"'))
        .unwrap_or(raw_name)
        .trim()
        .to_string();
    if name.is_empty() || raw_value.is_empty() {
        return None;
    }

    Some((name, raw_value.to_string()))
}

fn find_field_separator(field: &str) -> Option<usize> {
    let mut in_string = false;
    for (idx, ch) in field.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            ':' if !in_string => return Some(idx),
            _ => {}
        }
    }
    None
}

fn find_matching_delimiter(text: &str, open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0u32;
    for (idx, ch) in text.char_indices().skip(open_idx) {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

pub(super) fn handler_signature_for_event(
    event_name: &str,
    handler_props: &HashMap<String, String>,
) -> Option<String> {
    let vue_attr = vue_event_attr_label(event_name);
    emit_handler_keys(event_name)
        .into_iter()
        .find_map(|key| handler_props.get(&key).cloned())
        .or_else(|| {
            handler_props
                .iter()
                .find(|(name, _)| {
                    crate::type_provider::merge::jsx_prop_to_vue_attr(name).as_deref()
                        == Some(vue_attr.as_str())
                })
                .map(|(_, signature)| signature.clone())
        })
}

pub(super) fn emit_summary_signature(
    event_name: &str,
    handler_props: &HashMap<String, String>,
) -> String {
    handler_signature_for_event(event_name, handler_props)
        .map(|signature| summarize_event_handler_signature(&signature))
        .unwrap_or_else(|| "()".to_string())
}

pub(super) fn emit_name_for_summary(event_name: &str) -> String {
    vue_event_attr_label(event_name)
        .trim_start_matches('@')
        .to_string()
}

pub(super) fn normalize_event_handler_signature(signature: &str) -> String {
    if let Some(tuple_params) = tuple_payload_params(signature) {
        return format!("({tuple_params}) => void");
    }
    signature.trim().to_string()
}

pub(super) fn summarize_event_handler_signature(signature: &str) -> String {
    if let Some(tuple_params) = tuple_payload_params(signature) {
        return format!("({tuple_params})");
    }
    if let Some(params) = parameter_list(signature) {
        return format!("({params})");
    }
    "()".to_string()
}

fn tuple_payload_params(signature: &str) -> Option<String> {
    let trimmed = signature.trim();
    let start = trimmed.strip_prefix("(...args: [")?;
    let end = start.find("]) =>")?;
    Some(start[..end].trim().to_string())
}

fn parameter_list(signature: &str) -> Option<String> {
    let trimmed = signature.trim();
    let params = trimmed.strip_prefix('(')?;
    let end = params.find(')')?;
    Some(params[..end].trim().to_string())
}

fn emit_handler_keys(event_name: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let canonical = format!("on{}", capitalize_first(event_name));
    keys.push(canonical.clone());

    if !event_name.contains(':') {
        let camel = format!("on{}", capitalize_first(&camelize_event_name(event_name)));
        if camel != canonical {
            keys.push(camel);
        }

        let kebab = format!("on{}", capitalize_first(&hyphenate_event_name(event_name)));
        if kebab != canonical && !keys.iter().any(|key| key == &kebab) {
            keys.push(kebab);
        }
    }

    keys
}
