//! Free helpers for SFC script extraction and template-converter input
//! shaping.
//!
//! Owns:
//! - `template_converter_inputs` — projects analysed imports / macros /
//!   bindings into the `(all_imports, unions, props_binding_name)` tuple
//!   consumed by `crate::template_convert::convert_raw_to_analysis`.
//! - `extract_vue_script_content` — public Vue-SFC `<script>` /
//!   `<script setup>` position-preserving source builder used by the
//!   eval-program pipeline. It copies each script block's content to its
//!   RAW SFC byte range and whitespace-blanks every non-script byte, so
//!   every OXC-produced span is SFC-absolute by construction. Cached-parse
//!   spans are used when they agree with the raw scan; otherwise it falls
//!   through to the raw scanner.
//! - The forgiving raw-byte scanner used as a fall-back when the parser
//!   produced lossy spans (`script_content_spans_from_*` and the ASCII
//!   tag/needle helpers).

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

/// Extract a **position-preserving** script-only source from a Vue SFC string.
///
/// The result is byte-for-byte the SAME LENGTH as `source`: every `<script>` /
/// `<script setup>` block's content is copied to its RAW SFC byte range, and
/// every non-script byte is whitespace-blanked (original `\r`/`\n` preserved so
/// line/column geometry is unchanged; all other bytes replaced with a single
/// space). Because the script text sits at its raw offsets, every span the OXC
/// parser produces from this source is SFC-ABSOLUTE by construction — there is
/// no compact-concatenation coordinate system to translate back from.
///
/// Cached parse spans are used when they agree with a raw-source scan. If the
/// parser produced lossy spans for forgiving SFC input, fall back to the raw
/// scan so type resolution still sees the original script text.
pub(crate) fn extract_vue_script_content(
    source: &str,
    cached_parse: Option<&verter_compiler::parser::types::ParsedSfc>,
) -> Option<String> {
    let scanned = script_content_spans_from_source(source);
    let parsed = cached_parse.and_then(script_content_spans_from_parsed);

    // Prefer the parser's spans when they AGREE with the raw scan (same
    // semantics as the previous compact extractor, which compared the two
    // concatenated strings). On disagreement / parser miss, the raw scan is
    // authoritative so forgiving SFC input still yields script text.
    let spans = match (parsed, scanned) {
        (Some(parsed), Some(scanned)) if parsed == scanned => parsed,
        (_, Some(scanned)) => scanned,
        (Some(parsed), None) => parsed,
        (None, None) => return None,
    };
    if spans.is_empty() {
        return None;
    }

    Some(build_position_preserving_script_source(source, &spans))
}

/// Collect the sorted, de-duplicated `<script>` content byte ranges from a
/// parsed SFC. Ranges are RAW SFC byte offsets `[start, end)`.
fn script_content_spans_from_parsed(
    parsed: &verter_compiler::parser::types::ParsedSfc,
) -> Option<Vec<(u32, u32)>> {
    let mut spans: Vec<(u32, u32)> = [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .filter_map(|script| script.content.map(|span| (span.start, span.end)))
        .filter(|(start, end)| end > start)
        .collect();
    spans.sort_by_key(|(start, _)| *start);
    (!spans.is_empty()).then_some(spans)
}

/// Forgiving raw-byte scanner: collect the sorted `<script>` content byte
/// ranges from SFC source when the parser produced lossy spans. Ranges are RAW
/// SFC byte offsets `[start, end)`.
fn script_content_spans_from_source(source: &str) -> Option<Vec<(u32, u32)>> {
    const SCRIPT_OPEN: &[u8] = b"<script";
    const SCRIPT_CLOSE: &[u8] = b"</script>";

    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut spans: Vec<(u32, u32)> = Vec::new();

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

        if close_start > content_start && source.is_char_boundary(content_start) {
            spans.push((content_start as u32, close_start as u32));
        }
        cursor = close_start.saturating_add(SCRIPT_CLOSE.len());
    }

    spans.sort_by_key(|(start, _)| *start);
    (!spans.is_empty()).then_some(spans)
}

/// Build a same-length, position-preserving script-only source.
///
/// `spans` are RAW SFC content byte ranges `[start, end)`, pre-sorted and
/// non-overlapping. Every byte of the output is one of:
/// - a script-content byte copied verbatim from `source` at its raw offset;
/// - an original line terminator (`\r` / `\n`) preserved so line geometry is
///   identical to the SFC;
/// - a single ASCII space for every other (markup) byte.
///
/// In addition, every inter-script gap that contains NO line terminator gets a
/// single injected `\n` at its first byte, matching the `"\n"` separator the
/// previous compact extractor inserted between adjacent blocks (so two blocks
/// whose last/first statements lack a trailing `;` or end in a `//` line
/// comment still parse as two statements, never one fused line).
///
/// UTF-8 safety: every replaced byte is a single-byte ASCII value and every
/// preserved range is copied wholesale from valid UTF-8, so the result is valid
/// UTF-8 and exactly `source.len()` bytes long.
fn build_position_preserving_script_source(source: &str, spans: &[(u32, u32)]) -> String {
    let src = source.as_bytes();
    // Blank every byte first (line terminators preserved), then stamp script
    // content over its raw range.
    let mut out: Vec<u8> = src
        .iter()
        .map(|&b| if b == b'\n' || b == b'\r' { b } else { b' ' })
        .collect();

    for &(start, end) in spans {
        let (start, end) = (start as usize, end as usize);
        if end <= src.len() && start <= end {
            out[start..end].copy_from_slice(&src[start..end]);
        }
    }

    // Inject a newline into any inter-script gap that lacks a terminator so
    // adjacent blocks stay on separate logical lines for the TS parser.
    for window in spans.windows(2) {
        let prev_end = window[0].1 as usize;
        let next_start = window[1].0 as usize;
        if prev_end >= next_start || next_start > out.len() {
            continue;
        }
        let gap = &out[prev_end..next_start];
        if !gap.iter().any(|&b| b == b'\n' || b == b'\r') {
            // Gap is all spaces (already blanked); the first gap byte is safe
            // to overwrite with a newline without disturbing any script byte.
            out[prev_end] = b'\n';
        }
    }

    debug_assert_eq!(
        out.len(),
        src.len(),
        "position-preserving script source must equal the SFC source length"
    );
    // SAFETY-equivalent: constructed entirely from ASCII replacements + verbatim
    // copies of valid-UTF-8 script ranges, so `out` is valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|err| {
        // Defensive: a malformed span boundary could (in principle) split a
        // multi-byte sequence. Fall back to a lossy decode rather than panic;
        // this preserves length only when the input was ASCII, but a torn
        // boundary is already a corrupt-span bug surfaced elsewhere.
        String::from_utf8_lossy(err.as_bytes()).into_owned()
    })
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
