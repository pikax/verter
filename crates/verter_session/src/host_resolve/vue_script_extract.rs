//! Free helpers for SFC script extraction and template-converter input
//! shaping.
//!
//! Owns:
//! - `template_converter_inputs` — projects analysed imports / macros /
//!   bindings into the `(all_imports, unions, props_binding_name)` tuple
//!   consumed by `crate::template_convert::convert_raw_to_analysis`.
//! - `extract_vue_script_content` — public Vue-SFC `<script>` /
//!   `<script setup>` text extractor used by the eval-program pipeline,
//!   with `cached_parse` agreement / fall-through onto the raw scanner.
//! - The forgiving raw-byte scanner used as a fall-back when the parser
//!   produced lossy spans (`extract_vue_script_content_from_*` and the
//!   ASCII tag/needle helpers).

#[allow(clippy::type_complexity)]
pub(crate) fn template_converter_inputs(
    imports: &[verter_semantic::analysis::AnalyzedImport],
    macros: &[verter_semantic::analysis::AnalyzedMacro],
    bindings: &[verter_semantic::analysis::AnalyzedBinding],
) -> (
    Vec<(String, String)>,
    Vec<(String, Vec<String>)>,
    Option<String>,
) {
    let all_imports: Vec<(String, String)> = imports
        .iter()
        .flat_map(|imp| {
            imp.bindings
                .iter()
                .map(|binding| (binding.name.clone(), imp.source.clone()))
        })
        .collect();

    let mut unions = Vec::new();
    let define_props = macros
        .iter()
        .find(|mac| mac.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineProps);
    if let Some(dp) = define_props {
        for field in &dp.prop_fields {
            if let Some(type_ann) = &field.type_annotation {
                let classes = verter_semantic::analysis::parse_string_literal_union(type_ann);
                if !classes.is_empty() {
                    unions.push((field.name.clone(), classes));
                }
            }
        }
    }

    for binding in bindings {
        if let Some(type_ann) = &binding.type_annotation {
            let effective_type =
                verter_semantic::analysis::unwrap_reactive_type(type_ann).unwrap_or(type_ann);
            let classes = verter_semantic::analysis::parse_string_literal_union(effective_type);
            if !classes.is_empty() {
                unions.push((binding.name.clone(), classes));
            }
        }
    }

    let props_binding_name = define_props.and_then(|dp| dp.binding_name.clone());

    (all_imports, unions, props_binding_name)
}

/// Extract concatenated script content from a Vue SFC source string.
///
/// Cached parse spans are used when they agree with a raw-source scan. If the
/// parser produced lossy spans for forgiving SFC input, fall back to the raw
/// scan so type resolution still sees the original script text.
pub(crate) fn extract_vue_script_content(
    source: &str,
    cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
) -> Option<String> {
    let scanned = extract_vue_script_content_from_source(source);
    let parsed =
        cached_parse.and_then(|parsed| extract_vue_script_content_from_parsed(source, parsed));

    match (parsed, scanned) {
        (Some(parsed), Some(scanned)) if parsed == scanned => Some(parsed),
        (_, Some(scanned)) => Some(scanned),
        (Some(parsed), None) => Some(parsed),
        (None, None) => None,
    }
}

fn extract_vue_script_content_from_parsed(
    source: &str,
    parsed: &verter_compiler::parser::types::ParsedSfc,
) -> Option<String> {
    let mut script_blocks: Vec<(u32, u32)> = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .filter_map(|script| script.content.map(|span| (span.start, span.end)))
        .collect();
    script_blocks.sort_by_key(|(start, _)| *start);

    let mut combined = String::new();
    for (start, end) in script_blocks {
        let Some(content) = source.get(start as usize..end as usize) else {
            continue;
        };
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(content);
    }

    (!combined.is_empty()).then_some(combined)
}

fn extract_vue_script_content_from_source(source: &str) -> Option<String> {
    const SCRIPT_OPEN: &[u8] = b"<script";
    const SCRIPT_CLOSE: &[u8] = b"</script>";

    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut combined = String::new();

    while let Some(open_start) = find_ascii_tag(bytes, SCRIPT_OPEN, cursor) {
        let Some(tag_end) = find_tag_end(bytes, open_start) else {
            break;
        };
        if is_self_closing_tag(bytes, tag_end) {
            cursor = tag_end.saturating_add(1);
            continue;
        }

        let content_start = tag_end.saturating_add(1);
        let boundary = find_next_known_root_block(bytes, content_start).unwrap_or(bytes.len());
        let Some(close_start) = find_last_ascii_tag(bytes, SCRIPT_CLOSE, content_start, boundary)
        else {
            cursor = content_start;
            continue;
        };

        let Some(content) = source.get(content_start..close_start) else {
            cursor = close_start.saturating_add(SCRIPT_CLOSE.len());
            continue;
        };
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(content);
        cursor = close_start.saturating_add(SCRIPT_CLOSE.len());
    }

    (!combined.is_empty()).then_some(combined)
}

fn find_ascii_tag(bytes: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || bytes.len() < needle.len() || from >= bytes.len() {
        return None;
    }

    let last_start = bytes.len() - needle.len();
    let mut idx = from;
    while idx <= last_start {
        if bytes[idx..idx + needle.len()].eq_ignore_ascii_case(needle)
            && matches!(
                bytes.get(idx + needle.len()),
                None | Some(b'>')
                    | Some(b'/')
                    | Some(b' ')
                    | Some(b'\t')
                    | Some(b'\n')
                    | Some(b'\r')
            )
        {
            return Some(idx);
        }
        idx += 1;
    }
    None
}

fn find_last_ascii_tag(bytes: &[u8], needle: &[u8], from: usize, to: usize) -> Option<usize> {
    if needle.is_empty() || from >= to || bytes.len() < needle.len() {
        return None;
    }

    let search_end = to.min(bytes.len());
    let mut last = None;
    let mut cursor = from;
    while let Some(idx) = find_ascii_tag(bytes, needle, cursor) {
        if idx >= search_end {
            break;
        }
        last = Some(idx);
        cursor = idx.saturating_add(needle.len());
    }
    last
}

fn find_tag_end(bytes: &[u8], open_start: usize) -> Option<usize> {
    let mut idx = open_start.saturating_add(1);
    let mut quote = None;

    while idx < bytes.len() {
        let ch = bytes[idx];
        match quote {
            Some(active) if ch == active => quote = None,
            Some(_) => {}
            None if ch == b'\'' || ch == b'"' => quote = Some(ch),
            None if ch == b'>' => return Some(idx),
            None => {}
        }
        idx += 1;
    }

    None
}

fn is_self_closing_tag(bytes: &[u8], tag_end: usize) -> bool {
    if tag_end == 0 {
        return false;
    }

    let mut idx = tag_end;
    while idx > 0 {
        idx -= 1;
        match bytes[idx] {
            b' ' | b'\t' | b'\n' | b'\r' => continue,
            b'/' => return true,
            _ => return false,
        }
    }

    false
}

fn find_next_known_root_block(bytes: &[u8], from: usize) -> Option<usize> {
    [
        b"<script".as_slice(),
        b"<template".as_slice(),
        b"<style".as_slice(),
    ]
    .into_iter()
    .filter_map(|needle| find_ascii_tag(bytes, needle, from))
    .min()
}
