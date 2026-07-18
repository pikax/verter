//! Free helpers for SFC script extraction and template-converter input
//! shaping — the session's Vue-bridge extraction module.
//!
//! Owns:
//! - `template_converter_inputs` — projects analysed imports / macros /
//!   bindings into the `(all_imports, unions, props_binding_name)` tuple
//!   consumed by `crate::template_convert::convert_raw_to_analysis`.
//! - `extract_vue_script_content` — public Vue-SFC `<script>` /
//!   `<script setup>` position-preserving source builder used by the
//!   eval-program pipeline. It copies each script block's content to its
//!   RAW SFC byte range and whitespace-blanks every non-script byte, so
//!   every OXC-produced span is SFC-absolute by construction. Carrier
//!   parse spans are used when they agree with the raw scan; otherwise it
//!   falls through to the raw scanner.
//! - The forgiving raw-byte scanner used as a fall-back when the parser
//!   produced lossy spans (`script_content_spans_from_*` and the ASCII
//!   tag/needle helpers).
//! - The `<script setup generic="…">` type-parameter reader
//!   (`sfc_script_setup_type_params`) and the component-meta
//!   `populate_sfc_blocks_sidecar` — Vue-semantic leaves that open the
//!   neutral parse artifact through the blessed `vue_parse()` accessor,
//!   keeping `host_manage/**` free of Vue parse types. Generic parameters
//!   BIND through the prepared-decl bundle's script-setup type bindings
//!   (the dispatch `DeclarationScopePayload` rail), never through an eval
//!   env.

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
    // Carrier-linkage map for template components. A `import type { X }` (or a
    // per-specifier `import { type X }`) is a TYPE-ONLY binding — it has no
    // runtime value, so a tag `<X/>` must NEVER be carrier-linked to it as a
    // value component. Mirror `compile_fact_emission::binding_is_value`
    // (`!import.is_type_only && !binding.is_type_only`) so only runtime value
    // bindings enter the linkage map.
    let mut all_imports: Vec<(String, String)> = imports
        .iter()
        .filter(|imp| !imp.is_type_only)
        .flat_map(|imp| {
            imp.bindings
                .iter()
                .filter(|binding| !binding.is_type_only)
                .map(|binding| (binding.name.clone(), imp.source.clone()))
        })
        .collect();

    // A `const X = defineAsyncComponent(() => import('./X.vue'))` declares a
    // component whose carrier is the dynamically-imported `.vue`. The analyzer
    // captures the static loader specifier on the binding's initializer; surface
    // it in the linkage map so a `<X>` tag links to its `.vue` carrier exactly
    // like a static default import.
    for binding in bindings {
        if let Some(verter_semantic::analysis::BindingInitializer::FunctionCall {
            async_component_source: Some(source),
            ..
        }) = &binding.initializer
        {
            all_imports.push((binding.name.clone(), source.clone()));
        }
    }

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

/// Read the `<script setup generic="…">` type-parameter clause from the
/// neutral parse artifact (opened through the blessed `vue_parse()`
/// accessor). Empty for non-Vue artifacts, plain scripts, and Vue files
/// without `<script setup>` generics.
pub(crate) fn sfc_script_setup_type_params(
    source: &str,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> Vec<verter_type_expr::TypeParam> {
    let parsed = framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse);
    let Some(setup) = parsed.and_then(|parsed| parsed.script_setup()) else {
        return Vec::new();
    };
    let Some(generic_span) = setup.generic else {
        return Vec::new();
    };
    let clause = source[generic_span.start as usize..generic_span.end as usize].trim();
    if clause.is_empty() {
        return Vec::new();
    }
    verter_semantic::analysis::type_eval_build::parse_type_parameter_clause(clause)
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
    parsed_sfc: Option<&verter_compiler::parser::types::ParsedSfc>,
) -> Option<String> {
    let scanned = script_content_spans_from_source(source);
    let parsed = parsed_sfc.and_then(script_content_spans_from_parsed);

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
        // Close tag must be found with JS-aware scanning: a comment or string
        // containing `` `<style scoped>` `` / `"</script>"` must NOT truncate
        // the script block (reka-ui RadioGroupItem, and the existing
        // `</script>`-in-string case).
        let Some(close_start) = find_script_close_outside_js_context(bytes, content_start) else {
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

/// Find the first `</script>` after `from` that is not inside a JS string,
/// template literal, or line/block comment.
///
/// A naive scan that stops at the next SFC root tag (`<style` / `<template`)
/// false-positives on comments like `` // `<style scoped>` keeps working ``,
/// truncating the setup block and dropping every `defineProps` macro.
fn find_script_close_outside_js_context(bytes: &[u8], from: usize) -> Option<usize> {
    const CLOSE: &[u8] = b"</script>";
    if from >= bytes.len() {
        return None;
    }

    let mut i = from;
    // 0 = code, 1 = line comment, 2 = block comment, 3 = ', 4 = ", 5 = `
    let mut state: u8 = 0;

    while i < bytes.len() {
        let b = bytes[i];
        match state {
            0 => {
                // Line comment
                if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = 1;
                    i += 2;
                    continue;
                }
                // Block comment
                if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = 2;
                    i += 2;
                    continue;
                }
                // Quotes
                if b == b'\'' {
                    state = 3;
                    i += 1;
                    continue;
                }
                if b == b'"' {
                    state = 4;
                    i += 1;
                    continue;
                }
                if b == b'`' {
                    state = 5;
                    i += 1;
                    continue;
                }
                // Potential </script> (case-insensitive). The needle already
                // ENDS in `>` — the match is a complete close tag, so no
                // after-byte boundary check applies (an after-byte rule
                // belongs to a `<script`-PREFIX needle; requiring one here
                // silently dropped spans for the adjacent
                // `</script><template>` spelling).
                if b == b'<'
                    && i + CLOSE.len() <= bytes.len()
                    && bytes[i..i + CLOSE.len()].eq_ignore_ascii_case(CLOSE)
                {
                    return Some(i);
                }
                i += 1;
            }
            1 => {
                // Line comment ends at newline
                if b == b'\n' || b == b'\r' {
                    state = 0;
                }
                i += 1;
            }
            2 => {
                // Block comment ends at */
                if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    state = 0;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            3 => {
                // Single-quoted string (no multi-line escape handling beyond \')
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'\'' {
                    state = 0;
                }
                i += 1;
            }
            4 => {
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'"' {
                    state = 0;
                }
                i += 1;
            }
            5 => {
                // Template literal: ignore ${} nesting for close-tag search;
                // only unescaped ` ends the template.
                if b == b'\\' {
                    i = (i + 2).min(bytes.len());
                    continue;
                }
                if b == b'`' {
                    state = 0;
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
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
pub(crate) fn build_position_preserving_script_source(
    source: &str,
    spans: &[(u32, u32)],
) -> String {
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

fn string_from_span(source: &str, span: Option<verter_compiler::common::Span>) -> Option<String> {
    span.map(|span| source[span.start as usize..span.end as usize].to_string())
}

fn sfc_attributes_from_props(
    props: &[verter_compiler::types::NodeProp],
    source: &str,
) -> Vec<verter_semantic::analysis::component_meta::SfcAttributeAnalysis> {
    crate::parse::extract_attrs(props, source)
        .into_iter()
        .map(
            |(name, value)| verter_semantic::analysis::component_meta::SfcAttributeAnalysis {
                name: name.to_string(),
                value: if value.is_empty() {
                    None
                } else {
                    Some(value.to_string())
                },
            },
        )
        .collect()
}

fn sfc_custom_block_type(source: &str, tag_open: &verter_compiler::types::NodeTag) -> String {
    source[tag_open.start as usize + 1..tag_open.name_end as usize].to_string()
}

/// Populate the component-meta SFC-blocks sidecar from the carrier
/// parse (template/script/style/custom block attrs). Vue-semantic leaf:
/// opens the neutral artifact through the blessed `vue_parse()`
/// accessor. No-op for non-Vue canonicals and artifact-less state.
pub(crate) fn populate_sfc_blocks_sidecar(
    host: &crate::VerterHost,
    canonical_id: &str,
    meta: &mut verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
) {
    let Some((source, framework_parse, _)) = host.current_eval_state(canonical_id) else {
        return;
    };
    let Some(parsed) = framework_parse
        .as_deref()
        .and_then(crate::typeinfo::adapters::vue::vue_parse)
    else {
        return;
    };
    let source = source.as_ref();

    let template = parsed.template_ast().map(|template| {
        let attrs = crate::parse::extract_attrs(&template.root.attributes, source);
        verter_semantic::analysis::component_meta::TemplateBlockAnalysis {
            lang: string_from_span(source, template.root.lang),
            src: crate::parse::find_attr(&attrs, "src"),
            attributes: sfc_attributes_from_props(&template.root.attributes, source),
        }
    });

    let script = parsed.script().map(|script| {
        let attrs = crate::parse::extract_attrs(&script.attributes, source);
        verter_semantic::analysis::component_meta::ScriptBlockAnalysis {
            lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
            src: crate::parse::find_attr(&attrs, "src"),
            generic: string_from_span(source, script.generic),
            attrs_type: string_from_span(source, script.attrs),
            attributes: sfc_attributes_from_props(&script.attributes, source),
        }
    });

    let script_setup = parsed.script_setup().map(|script| {
        let attrs = crate::parse::extract_attrs(&script.attributes, source);
        verter_semantic::analysis::component_meta::ScriptBlockAnalysis {
            lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
            src: crate::parse::find_attr(&attrs, "src"),
            generic: string_from_span(source, script.generic),
            attrs_type: string_from_span(source, script.attrs),
            attributes: sfc_attributes_from_props(&script.attributes, source),
        }
    });

    let styles = parsed
        .style_nodes()
        .iter()
        .enumerate()
        .map(|(index, style)| {
            let attrs = crate::parse::extract_attrs(&style.attributes, source);
            verter_semantic::analysis::component_meta::StyleBlockInfoAnalysis {
                index,
                lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
                src: crate::parse::find_attr(&attrs, "src"),
                scoped: style.scoped,
                is_module: style.module,
                module_name: crate::parse::find_attr(&attrs, "module")
                    .filter(|value| value != "true"),
                attributes: sfc_attributes_from_props(&style.attributes, source),
            }
        })
        .collect();

    let custom = parsed
        .unknown_nodes()
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let attrs = crate::parse::extract_attrs(&block.attributes, source);
            verter_semantic::analysis::component_meta::CustomBlockAnalysis {
                index,
                block_type: sfc_custom_block_type(source, &block.tag_open),
                lang: crate::parse::find_attr(&attrs, "lang").filter(|lang| lang != "true"),
                src: crate::parse::find_attr(&attrs, "src"),
                attributes: sfc_attributes_from_props(&block.attributes, source),
            }
        })
        .collect();

    meta.sfc_blocks = Some(
        verter_semantic::analysis::component_meta::SfcBlocksAnalysis {
            template,
            script,
            script_setup,
            styles,
            custom,
        },
    );
}
