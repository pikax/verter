//! SFC tokenization, block hashing, and `ParseSnapshot` construction.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use verter_core::compile::parse_sfc;
use verter_core::diagnostics::DiagnosticSeverity;
use verter_core::parser::types::ParsedSfc;
use verter_core::types::NodeProp;

use crate::hash::{hash_16, semantic_hash};
use crate::id::resolve_external;
use crate::types::{
    DescriptorMin, DiagnosticsSnapshot, ExternalBlockKind, ExternalSourceRequest, FileMeta,
    HostDiagnostic, HostSeverity, ParseSnapshot, PreprocessorBlockType, PreprocessorRequest,
    SliceHashes, SrcBlockInfo,
};

/// Zero-copy attribute extraction: returns slices borrowed from `source`.
pub(crate) fn extract_attrs<'a>(props: &[NodeProp], source: &'a str) -> Vec<(&'a str, &'a str)> {
    props
        .iter()
        .map(|p| {
            let name = &source[p.start as usize..p.name_end as usize];
            let value = match (p.value_start, p.value_end) {
                (Some(s), Some(e)) => &source[s as usize..e as usize],
                _ => "",
            };
            (name, value)
        })
        .collect()
}

pub(crate) fn normalize_attr_map(attrs: &[(&str, &str)], include: &[&str]) -> String {
    let mut map = BTreeMap::<&str, &str>::new();
    for &(k, v) in attrs {
        if let Some(&key) = include.iter().find(|&&s| k.eq_ignore_ascii_case(s)) {
            let value = if v.is_empty() { "true" } else { v };
            map.insert(key, value);
        }
    }
    let mut out = String::new();
    for (k, v) in map {
        let _ = writeln!(&mut out, "{}={}", k, v);
    }
    out
}

pub(crate) fn find_attr(attrs: &[(&str, &str)], name: &str) -> Option<String> {
    attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| {
            if v.is_empty() {
                "true".to_string()
            } else {
                v.to_string()
            }
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn try_resolve_src_block(
    canonical_id: &str,
    attrs: &[(&str, &str)],
    tag_name: &str,
    block_kind: ExternalBlockKind,
    index: usize,
    tag_open_start: u32,
    tag_open_end: u32,
    tag_close: Option<u32>,
    external_requests: &mut Vec<ExternalSourceRequest>,
    src_blocks: &mut Vec<SrcBlockInfo>,
) {
    if let Some(src) = find_attr(attrs, "src") {
        let resolved = resolve_external(canonical_id, &src);
        external_requests.push(ExternalSourceRequest {
            owner_canonical_id: canonical_id.to_string(),
            block_kind,
            index,
            specifier: src,
            resolved_canonical_id: resolved.clone(),
        });
        src_blocks.push(SrcBlockInfo {
            tag_name: tag_name.to_string(),
            resolved_canonical_id: resolved,
            tag_open_start,
            tag_open_end,
            tag_close_start: tag_close,
        });
    }
}

pub(crate) fn parse_vue_snapshot(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_analysis::AnalysisScope,
) -> (ParseSnapshot, ParsedSfc) {
    let whole_hash = hash_16(source.as_bytes());

    // Single parse — cached ParsedSfc is returned alongside the snapshot.
    let parsed = parse_sfc(source, None, None);

    let mut script_hashes = Vec::new();
    let mut script_attrs_fp = Vec::new();
    let mut script_count = 0;
    let mut has_script = false;
    let mut script_lang: Option<String> = None;
    // Content span for script block (used for preprocessor request content extraction)
    let mut script_content_span: Option<(u32, u32)> = None;
    let mut src_blocks = Vec::new();
    let mut external_requests = Vec::new();

    for (idx, script) in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
        .enumerate()
    {
        script_count += 1;
        has_script = true;
        let content = if let Some(span) = script.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        script_hashes.push(hash_16(content));

        let mut attrs = extract_attrs(&script.attributes, source);
        // Capture script lang from the first script block that has one
        if script_lang.is_none() {
            if let Some(lang) = find_attr(&attrs, "lang") {
                if lang != "true" {
                    script_lang = Some(lang);
                    // Capture content span for the script with non-native lang
                    if let Some(span) = script.content {
                        script_content_span = Some((span.start, span.end));
                    }
                }
            }
        }
        if script.is_setup {
            attrs.push(("setup", "true"));
        }
        if let Some(src_span) = script.src {
            let specifier = source[src_span.start as usize..src_span.end as usize].to_string();
            let resolved = resolve_external(canonical_id, &specifier);
            src_blocks.push(SrcBlockInfo {
                tag_name: "script".to_string(),
                resolved_canonical_id: resolved.clone(),
                tag_open_start: script.tag_open.start,
                tag_open_end: script.tag_open.end,
                tag_close_start: script.tag_close.as_ref().map(|c| c.start),
            });
            external_requests.push(ExternalSourceRequest {
                owner_canonical_id: canonical_id.to_string(),
                block_kind: ExternalBlockKind::Script,
                index: idx,
                specifier,
                resolved_canonical_id: resolved,
            });
        }
        script_attrs_fp.push(normalize_attr_map(
            &attrs,
            &["setup", "lang", "src", "generic", "attrs"],
        ));
    }

    let script_hash = if script_hashes.is_empty() {
        None
    } else {
        let mut buf = Vec::with_capacity(script_hashes.len() * 16);
        for h in &script_hashes {
            buf.extend_from_slice(h);
        }
        Some(hash_16(&buf))
    };

    let mut template_count = 0;
    let mut has_template = false;
    let mut template_hash = None;
    let mut template_attrs_fp = Vec::new();
    let mut template_lang: Option<String> = None;
    // Content span for template block (used for preprocessor request content extraction)
    let mut template_content_span: Option<(u32, u32)> = None;

    if let Some(ast) = parsed.template_ast() {
        template_count = 1;
        has_template = true;
        if let Some(content) = ast.root.content.as_ref() {
            template_hash = Some(hash_16(
                &source.as_bytes()[content.start as usize..content.end as usize],
            ));
            template_content_span = Some((content.start, content.end));
        } else {
            template_hash = Some(hash_16(&[]));
        }

        let attrs = extract_attrs(&ast.root.attributes, source);
        // Capture template lang from lang attribute
        if let Some(lang) = find_attr(&attrs, "lang") {
            if lang != "true" && !lang.eq_ignore_ascii_case("html") {
                template_lang = Some(lang);
            }
        }
        try_resolve_src_block(
            canonical_id,
            &attrs,
            "template",
            ExternalBlockKind::Template,
            0,
            ast.root.tag_open.start,
            ast.root.tag_open.end,
            ast.root.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );
        template_attrs_fp.push(normalize_attr_map(&attrs, &["lang", "src"]));
    }

    let mut style_hashes = Vec::new();
    let mut style_attrs_fp = Vec::new();
    let mut style_langs = Vec::new();
    let mut has_scoped_style = false;
    // Content spans for style blocks (used for preprocessor request content extraction)
    let mut style_content_spans: Vec<Option<(u32, u32)>> = Vec::new();

    for (idx, style) in parsed.style_nodes().iter().enumerate() {
        let content = if let Some(span) = style.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        style_hashes.push(hash_16(content));

        let mut attrs = extract_attrs(&style.attributes, source);
        if style.scoped {
            has_scoped_style = true;
            attrs.push(("scoped", "true"));
        }
        if style.module {
            attrs.push(("module", "true"));
        }

        try_resolve_src_block(
            canonical_id,
            &attrs,
            "style",
            ExternalBlockKind::Style,
            idx,
            style.tag_open.start,
            style.tag_open.end,
            style.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );

        style_attrs_fp.push(normalize_attr_map(
            &attrs,
            &["scoped", "module", "lang", "src"],
        ));

        style_langs.push(find_attr(&attrs, "lang"));
        style_content_spans.push(style.content.map(|span| (span.start, span.end)));
    }

    let mut custom_hashes = Vec::new();
    let mut custom_attrs_fp = Vec::new();
    let mut custom_types = Vec::new();
    let mut custom_langs = Vec::new();
    // Content spans for custom blocks (used for preprocessor request content extraction)
    let mut custom_content_spans: Vec<Option<(u32, u32)>> = Vec::new();

    for (idx, custom) in parsed.unknown_nodes().iter().enumerate() {
        let content = if let Some(span) = custom.content {
            &source.as_bytes()[span.start as usize..span.end as usize]
        } else {
            b""
        };
        custom_hashes.push(hash_16(content));

        let block_type =
            &source[custom.tag_open.start as usize + 1..custom.tag_open.name_end as usize];
        custom_types.push(block_type.to_string());

        let mut attrs = extract_attrs(&custom.attributes, source);
        attrs.push(("type", block_type));

        try_resolve_src_block(
            canonical_id,
            &attrs,
            block_type,
            ExternalBlockKind::Custom,
            idx,
            custom.tag_open.start,
            custom.tag_open.end,
            custom.tag_close.as_ref().map(|c| c.start),
            &mut external_requests,
            &mut src_blocks,
        );

        custom_langs.push(find_attr(&attrs, "lang"));
        custom_content_spans.push(custom.content.map(|span| (span.start, span.end)));

        custom_attrs_fp.push(normalize_attr_map(&attrs, &["type", "lang", "src"]));
    }

    let descriptor = DescriptorMin {
        script_count,
        template_count,
        style_count: style_hashes.len(),
        custom_count: custom_hashes.len(),
        script_attr_fingerprints: script_attrs_fp,
        template_attr_fingerprints: template_attrs_fp,
        style_attr_fingerprints: style_attrs_fp,
        custom_attr_fingerprints: custom_attrs_fp,
        vapor: parsed.is_vapor(),
    };

    let slices = SliceHashes {
        script: script_hash,
        template: template_hash,
        styles: style_hashes,
        custom: custom_hashes,
    };

    let semantic_hash = semantic_hash(&slices, &descriptor);

    let raw_diags = parsed.clone_diagnostics();
    let parse_diagnostics = DiagnosticsSnapshot::from_vec(
        raw_diags
            .into_iter()
            .map(|d| HostDiagnostic {
                severity: match d.severity {
                    DiagnosticSeverity::Error => HostSeverity::Error,
                    DiagnosticSeverity::Warning => HostSeverity::Warning,
                    DiagnosticSeverity::Info => HostSeverity::Info,
                },
                code: format!("{:?}", d.code),
                message: d.message,
                span: d.span,
            })
            .collect(),
    );

    // Build style analyses for each style block (when style analysis flags are set)
    let style_analyses: Vec<verter_analysis::StyleBlockAnalysis> =
        if analysis_scope.needs_style_analysis() {
            build_style_analyses_from_parsed(&parsed, source, canonical_id)
        } else {
            Vec::new()
        };

    // Build script analysis from script block contents (when script analysis flags are set)
    let (script_analysis, script_panic_diag) = if analysis_scope.needs_script_analysis() {
        build_script_analysis_from_parsed_with_diagnostic(&parsed, source)
    } else {
        (verter_analysis::ScriptAnalysisSnapshot::default(), None)
    };

    // Merge any panic diagnostic into parse diagnostics
    let parse_diagnostics = if let Some(diag) = script_panic_diag {
        parse_diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![diag]))
    } else {
        parse_diagnostics
    };

    // Build preprocessor requests for non-native languages
    let preprocessor_requests = build_preprocessor_requests(
        &template_lang,
        template_content_span,
        &script_lang,
        script_content_span,
        &style_langs,
        &style_content_spans,
        &custom_types,
        &custom_langs,
        &custom_content_spans,
        source,
    );

    (
        ParseSnapshot {
            whole_hash,
            semantic_hash,
            slices,
            descriptor,
            meta: FileMeta {
                has_script,
                has_template,
                has_scoped_style,
                script_lang,
                template_lang,
                style_langs,
                custom_types,
                custom_langs,
            },
            external_requests,
            src_blocks,
            parse_diagnostics,
            script_analysis,
            export_signatures: Vec::new(),
            style_analyses,
            preprocessor_requests,
        },
        parsed,
    )
}

/// Build preprocessor requests for blocks that use non-native languages.
///
/// A non-native language is any `lang` that the Rust compiler cannot handle natively:
/// - Template: anything other than HTML (or no `lang`)
/// - Script: anything not in `[ts, tsx, js, jsx]`
/// - Style: anything other than CSS (or no `lang`)
/// - Custom: any custom block with a `lang` attribute
#[allow(clippy::too_many_arguments)]
fn build_preprocessor_requests(
    template_lang: &Option<String>,
    template_content_span: Option<(u32, u32)>,
    script_lang: &Option<String>,
    script_content_span: Option<(u32, u32)>,
    style_langs: &[Option<String>],
    style_content_spans: &[Option<(u32, u32)>],
    custom_types: &[String],
    custom_langs: &[Option<String>],
    custom_content_spans: &[Option<(u32, u32)>],
    source: &str,
) -> Vec<PreprocessorRequest> {
    let mut requests = Vec::new();

    // Template: non-native if template_lang is Some (already filtered for "html")
    if let Some(lang) = template_lang {
        let content = template_content_span
            .map(|(s, e)| &source[s as usize..e as usize])
            .unwrap_or("");
        requests.push(PreprocessorRequest {
            block_type: PreprocessorBlockType::Template,
            index: 0,
            lang: lang.clone(),
            content: content.to_string(),
        });
    }

    // Script: non-native if not in [ts, tsx, js, jsx]
    if let Some(lang) = script_lang {
        let is_native = matches!(
            lang.as_str(),
            "ts" | "tsx" | "js" | "jsx" | "TS" | "TSX" | "JS" | "JSX"
        );
        if !is_native {
            let content = script_content_span
                .map(|(s, e)| &source[s as usize..e as usize])
                .unwrap_or("");
            requests.push(PreprocessorRequest {
                block_type: PreprocessorBlockType::Script,
                index: 0,
                lang: lang.clone(),
                content: content.to_string(),
            });
        }
    }

    // Style: non-native if lang is Some and not "css"
    for (idx, lang_opt) in style_langs.iter().enumerate() {
        if let Some(lang) = lang_opt {
            if !lang.eq_ignore_ascii_case("css") {
                let content = style_content_spans
                    .get(idx)
                    .and_then(|s| *s)
                    .map(|(s, e)| &source[s as usize..e as usize])
                    .unwrap_or("");
                requests.push(PreprocessorRequest {
                    block_type: PreprocessorBlockType::Style,
                    index: idx,
                    lang: lang.clone(),
                    content: content.to_string(),
                });
            }
        }
    }

    // Custom: any custom block with a lang attribute
    for (idx, lang_opt) in custom_langs.iter().enumerate() {
        if let Some(lang) = lang_opt {
            let content = custom_content_spans
                .get(idx)
                .and_then(|s| *s)
                .map(|(s, e)| &source[s as usize..e as usize])
                .unwrap_or("");
            requests.push(PreprocessorRequest {
                block_type: PreprocessorBlockType::Custom,
                index: idx,
                lang: lang.clone(),
                content: content.to_string(),
            });
            // Also store custom block type name in context for the caller
            let _ = custom_types.get(idx); // suppress unused warning
        }
    }

    requests
}

/// Build a single style analysis from a parsed style node and the SFC source.
/// Shared by `parse_vue_snapshot()` (eager) and `build_style_analyses_from_source()` (on-demand).
fn build_single_style_analysis(
    style: &verter_core::parser::types::RootNodeStyle,
    source: &str,
    canonical_id: &str,
) -> verter_analysis::StyleBlockAnalysis {
    let module_name =
        find_attr(&extract_attrs(&style.attributes, source), "module").filter(|v| v != "true");
    let content_offset = style.content.map(|span| span.start).unwrap_or(0);

    // Extract CSS content from the SFC source
    let css_content = style
        .content
        .map(|span| &source[span.start as usize..span.end as usize])
        .unwrap_or("");

    // Run CSS prepass to extract v-bind() expressions and their generated variable names
    let component_name = verter_core::compile::extract_component_name(canonical_id);
    let scope_id = verter_core::compile::get_hash(&component_name);
    let prepass_result = verter_core::css::prepass::prepass(css_content, &scope_id);

    // Build VueStyleInput from prepass results
    let vue_input = verter_analysis::VueStyleInput {
        v_binds: prepass_result
            .v_bind_vars
            .iter()
            .map(|vb| verter_analysis::VBindInput {
                expression: vb.expression.clone(),
                quoted: false,
                start: content_offset,
                end: content_offset,
                generated_var_name: Some(vb.var_name.clone()),
            })
            .collect(),
        special_pseudos: vec![],
    };

    let analysis_lang = match style.lang {
        Some(verter_core::parser::types::StyleLang::Css) | None => {
            return verter_analysis::build_css_style_analysis(
                css_content,
                vue_input,
                style.scoped,
                style.module,
                module_name.as_deref(),
                content_offset,
            );
        }
        Some(verter_core::parser::types::StyleLang::Scss) => {
            verter_analysis::StyleAnalysisLang::Scss
        }
        Some(verter_core::parser::types::StyleLang::Sass) => {
            verter_analysis::StyleAnalysisLang::Sass
        }
        Some(verter_core::parser::types::StyleLang::Less) => {
            verter_analysis::StyleAnalysisLang::Less
        }
        Some(verter_core::parser::types::StyleLang::Stylus) => {
            verter_analysis::StyleAnalysisLang::Stylus
        }
        Some(verter_core::parser::types::StyleLang::Unknown) => {
            verter_analysis::StyleAnalysisLang::Unknown
        }
    };
    verter_analysis::build_preprocessor_style_analysis(
        analysis_lang,
        vue_input,
        style.scoped,
        style.module,
        module_name.as_deref(),
        content_offset,
    )
}

/// Run a closure with panic safety, returning a warning diagnostic if it panics.
fn catch_analysis_panic<T: Default>(
    label: &str,
    f: impl FnOnce() -> T + std::panic::UnwindSafe,
) -> (T, Option<HostDiagnostic>) {
    match std::panic::catch_unwind(f) {
        Ok(value) => (value, None),
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            let diagnostic = HostDiagnostic {
                severity: HostSeverity::Warning,
                code: "HOST_ANALYSIS_PANIC".to_string(),
                message: format!("{label}: {msg}"),
                span: None,
            };
            (T::default(), Some(diagnostic))
        }
    }
}

/// Build script analysis from an already-parsed SFC. Concatenates script block
/// contents and runs OXC analysis with catch_unwind for panic safety.
/// Shared by `parse_vue_snapshot()` (eager) and `build_script_analysis_from_parsed()`.
///
/// After OXC analysis, adjusts all span fields from script-content-relative offsets
/// to SFC-absolute byte offsets so downstream consumers (LSP features) can use them
/// directly with `LineIndex::offset_to_position()`.
fn build_script_analysis_from_parsed_with_diagnostic(
    parsed: &ParsedSfc,
    source: &str,
) -> (
    verter_analysis::ScriptAnalysisSnapshot,
    Option<HostDiagnostic>,
) {
    let mut combined_content = String::new();
    // Track (sfc_content_start, content_length) for each block in the concatenation
    let mut block_ranges: Vec<(u32, u32)> = Vec::new();
    for script in [parsed.script(), parsed.script_setup()]
        .into_iter()
        .flatten()
    {
        if let Some(span) = script.content {
            let content = &source[span.start as usize..span.end as usize];
            if !combined_content.is_empty() {
                combined_content.push('\n');
            }
            block_ranges.push((span.start, (span.end - span.start)));
            combined_content.push_str(content);
        }
    }
    if combined_content.is_empty() {
        return (verter_analysis::ScriptAnalysisSnapshot::default(), None);
    }
    let alloc = Allocator::new();
    let (mut analysis, diag) = catch_analysis_panic(
        "script analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_analysis::build_script_analysis(&combined_content, SourceType::ts(), &alloc)
        }),
    );
    adjust_analysis_spans(&mut analysis, &block_ranges);
    (analysis, diag)
}

/// Map a byte offset in the concatenated script content to the SFC-absolute offset.
///
/// The concatenated content is built as: `block0_content + "\n" + block1_content + ...`
/// Each block's `(sfc_start, length)` is tracked in `block_ranges`.
fn combined_offset_to_sfc(offset: u32, block_ranges: &[(u32, u32)]) -> u32 {
    let mut cursor = 0u32;
    for &(sfc_start, len) in block_ranges {
        if offset < cursor + len {
            return sfc_start + (offset - cursor);
        }
        cursor += len + 1; // +1 for the '\n' separator between blocks
    }
    // Fallback: offset past all blocks (shouldn't happen with valid spans)
    offset
}

/// Adjust all span fields in a `ScriptAnalysisSnapshot` from script-content-relative
/// to SFC-absolute byte offsets.
fn adjust_analysis_spans(
    analysis: &mut verter_analysis::ScriptAnalysisSnapshot,
    block_ranges: &[(u32, u32)],
) {
    if block_ranges.is_empty() {
        return;
    }
    // Single block with start=0 means no adjustment needed (non-SFC .ts file)
    if block_ranges.len() == 1 && block_ranges[0].0 == 0 {
        return;
    }

    let map = |offset: u32| combined_offset_to_sfc(offset, block_ranges);

    for import in &mut analysis.imports {
        import.span.start = map(import.span.start);
        import.span.end = map(import.span.end);
        for binding in &mut import.bindings {
            binding.span.start = map(binding.span.start);
            binding.span.end = map(binding.span.end);
        }
    }
    for reference in &mut analysis.module_references {
        reference.span.start = map(reference.span.start);
        reference.span.end = map(reference.span.end);
        reference.expr_span.start = map(reference.expr_span.start);
        reference.expr_span.end = map(reference.expr_span.end);
    }
    for binding in &mut analysis.bindings {
        binding.span.start = map(binding.span.start);
        binding.span.end = map(binding.span.end);
    }
    for mac in &mut analysis.macros {
        mac.span.start = map(mac.span.start);
        mac.span.end = map(mac.span.end);
        for pf in &mut mac.prop_fields {
            pf.span.start = map(pf.span.start);
            pf.span.end = map(pf.span.end);
        }
    }
    for call in &mut analysis.vue_api_calls {
        call.span.start = map(call.span.start);
        call.span.end = map(call.span.end);
        for param in &mut call.callback_params {
            param.span.start = map(param.span.start);
            param.span.end = map(param.span.end);
        }
    }
    for call in &mut analysis.dom_query_calls {
        call.span.start = map(call.span.start);
        call.span.end = map(call.span.end);
        call.arg_span.start = map(call.arg_span.start);
        call.arg_span.end = map(call.arg_span.end);
    }
    for manip in &mut analysis.css_var_manipulations {
        manip.span.start = map(manip.span.start);
        manip.span.end = map(manip.span.end);
    }
    if let Some(ref mut offset) = analysis.first_await_offset {
        *offset = map(*offset);
    }
}

/// Compute script analysis on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_script_analysis_from_source(
    source: &str,
) -> verter_analysis::ScriptAnalysisSnapshot {
    let parsed = parse_sfc(source, None, None);
    build_script_analysis_from_parsed(&parsed, source)
}

pub(crate) fn build_script_analysis_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
) -> verter_analysis::ScriptAnalysisSnapshot {
    let (analysis, _diag) = build_script_analysis_from_parsed_with_diagnostic(parsed, source);
    analysis
}

/// Compute style analyses on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_style_analyses_from_source(
    source: &str,
    canonical_id: &str,
) -> Vec<verter_analysis::StyleBlockAnalysis> {
    let parsed = parse_sfc(source, None, None);
    build_style_analyses_from_parsed(&parsed, source, canonical_id)
}

pub(crate) fn build_style_analyses_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
    canonical_id: &str,
) -> Vec<verter_analysis::StyleBlockAnalysis> {
    parsed
        .style_nodes()
        .iter()
        .map(|style| build_single_style_analysis(style, source, canonical_id))
        .collect()
}

pub(crate) fn parse_non_sfc_snapshot(canonical_id: &str, source: &str) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());
    let slices = SliceHashes::default();
    let descriptor = DescriptorMin::default();
    // Use whole_hash as semantic_hash so content changes in non-SFC files
    // (e.g. .ts files imported for type resolution) are properly detected
    // and trigger dependent SFC recompilation.
    let semantic_hash = whole_hash;

    // Use SourceType::d_ts() for declaration files — OXC parses them differently
    // and panics on certain constructs (e.g. tuple types) with the wrong mode.
    let source_type = if canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
    {
        SourceType::d_ts()
    } else {
        SourceType::ts()
    };

    let alloc = Allocator::new();
    let (export_signatures, panic_diag) = catch_analysis_panic(
        "export signature analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_analysis::build_export_signatures(source, source_type, &alloc)
        }),
    );

    // Run script analysis for non-SFC files to populate exported_functions
    // (composable return shape data used by cross-file binding enrichment).
    let alloc2 = Allocator::new();
    let (script_analysis, script_panic_diag) = catch_analysis_panic(
        "script analysis (non-SFC)",
        std::panic::AssertUnwindSafe(|| {
            // All script-applicable flags (skip template/style flags since they're SFC-specific).
            // TODO: may reduce to a lighter scope if analysis of non-SFC files becomes a performance bottleneck
            verter_analysis::build_script_analysis_with_scope(
                source,
                source_type,
                &alloc2,
                verter_analysis::AnalysisScope::IMPORTS
                    | verter_analysis::AnalysisScope::BINDINGS
                    | verter_analysis::AnalysisScope::FUNC_RETURNS
                    | verter_analysis::AnalysisScope::REACTIVITY
                    | verter_analysis::AnalysisScope::MACROS
                    | verter_analysis::AnalysisScope::MACRO_TYPE_DEPS
                    | verter_analysis::AnalysisScope::VUE_API_USAGE
                    | verter_analysis::AnalysisScope::EXPORT_SIGNATURES
                    | verter_analysis::AnalysisScope::SCRIPT_USAGES,
            )
        }),
    );

    let mut diags = Vec::new();
    if let Some(d) = panic_diag {
        diags.push(d);
    }
    if let Some(d) = script_panic_diag {
        diags.push(d);
    }
    let parse_diagnostics = if diags.is_empty() {
        DiagnosticsSnapshot::default()
    } else {
        DiagnosticsSnapshot::from_vec(diags)
    };

    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics,
        script_analysis,
        export_signatures,
        style_analyses: Vec::new(),
        preprocessor_requests: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;
    use verter_analysis::AnalysisScope;
    use verter_core::types::NodeProp;

    // ── Helper: build a NodeProp pointing into a source string ──

    fn make_prop(
        start: u32,
        name_end: u32,
        value_start: Option<u32>,
        value_end: Option<u32>,
    ) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            value_start,
            value_end,
            modifiers: SmallVec::new(),
            is_dynamic: None,
        }
    }

    // ═══════════════════════════════════════════════════════════
    // extract_attrs
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests extract_attrs preserves original case (zero-copy)
    #[test]
    fn extract_attrs_preserves_case() {
        //              0123456789
        // Lang="ts"
        // 0=L 1=a 2=n 3=g 4== 5=" 6=t 7=s 8="
        let source = "Lang=\"ts\"";
        let props = vec![make_prop(0, 4, Some(6), Some(8))];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs, vec![("Lang", "ts")]);
    }

    /// @ai-generated - Tests extract_attrs extracts attribute value correctly
    #[test]
    fn extract_attrs_extracts_value() {
        // src="./foo.html"
        // 0=s 1=r 2=c 3== 4=" 5=. 6=/ ... 14=l 15="
        let source = "src=\"./foo.html\"";
        let props = vec![make_prop(0, 3, Some(5), Some(15))];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs[0].1, "./foo.html");
    }

    /// @ai-generated - Tests extract_attrs with no value (boolean attribute)
    #[test]
    fn extract_attrs_no_value_is_empty_string() {
        let source = "scoped";
        let props = vec![make_prop(0, 6, None, None)];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs, vec![("scoped", "")]);
    }

    /// @ai-generated - Tests extract_attrs with multiple attributes
    #[test]
    fn extract_attrs_multiple_props() {
        // lang="ts" setup src
        // 0=l 1=a 2=n 3=g 4== 5=" 6=t 7=s 8=" 9=  10=s 11=e 12=t 13=u 14=p 15=  16=s 17=r 18=c
        let source = "lang=\"ts\" setup src";
        let props = vec![
            make_prop(0, 4, Some(6), Some(8)), // lang="ts"
            make_prop(10, 15, None, None),     // setup
            make_prop(16, 19, None, None),     // src
        ];
        let attrs = extract_attrs(&props, source);
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[0], ("lang", "ts"));
        assert_eq!(attrs[1], ("setup", ""));
        assert_eq!(attrs[2], ("src", ""));
    }

    // ═══════════════════════════════════════════════════════════
    // normalize_attr_map
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests normalize_attr_map filters to included keys only
    #[test]
    fn normalize_attr_map_include_filter() {
        let attrs = vec![("lang", "ts"), ("setup", ""), ("id", "foo")];
        let result = normalize_attr_map(&attrs, &["lang", "setup"]);
        assert!(result.contains("lang=ts"));
        assert!(result.contains("setup=true"));
        assert!(!result.contains("id"));
    }

    /// @ai-generated - Tests normalize_attr_map treats empty value as "true"
    #[test]
    fn normalize_attr_map_empty_value_becomes_true() {
        let attrs = vec![("scoped", "")];
        let result = normalize_attr_map(&attrs, &["scoped"]);
        assert!(result.contains("scoped=true"));
    }

    /// @ai-generated - Tests normalize_attr_map uses BTreeMap sort order
    #[test]
    fn normalize_attr_map_sorted_by_key() {
        let attrs = vec![("src", "x.ts"), ("lang", "ts")];
        let result = normalize_attr_map(&attrs, &["src", "lang"]);
        let lang_pos = result.find("lang").unwrap();
        let src_pos = result.find("src").unwrap();
        assert!(lang_pos < src_pos, "keys should be sorted alphabetically");
    }

    /// @ai-generated - Tests normalize_attr_map with no matching keys
    #[test]
    fn normalize_attr_map_no_matches_empty_string() {
        let attrs = vec![("id", "foo")];
        let result = normalize_attr_map(&attrs, &["lang", "setup"]);
        assert!(result.is_empty());
    }

    /// @ai-generated - normalize_attr_map uses newline separators, not literal \n
    #[test]
    fn normalize_attr_map_uses_newline_separator() {
        let attrs = vec![("lang", "ts"), ("scoped", "")];
        let result = normalize_attr_map(&attrs, &["lang", "scoped"]);
        // Each entry should be separated by a real newline character
        assert!(
            result.contains('\n'),
            "fingerprint should contain newline chars, got: {:?}",
            result
        );
        assert_eq!(result, "lang=ts\nscoped=true\n");
    }

    // ═══════════════════════════════════════════════════════════
    // find_attr
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Tests find_attr is case-insensitive
    #[test]
    fn find_attr_case_insensitive() {
        let attrs = vec![("Lang", "ts")];
        assert_eq!(find_attr(&attrs, "LANG"), Some("ts".to_string()));
        assert_eq!(find_attr(&attrs, "lang"), Some("ts".to_string()));
    }

    /// @ai-generated - Tests find_attr returns None for missing attribute
    #[test]
    fn find_attr_missing_returns_none() {
        let attrs = vec![("lang", "ts")];
        assert_eq!(find_attr(&attrs, "src"), None);
    }

    /// @ai-generated - Tests find_attr empty value returns "true"
    #[test]
    fn find_attr_empty_value_returns_true() {
        let attrs = vec![("scoped", "")];
        assert_eq!(find_attr(&attrs, "scoped"), Some("true".to_string()));
    }

    /// @ai-generated - Tests find_attr returns first match
    #[test]
    fn find_attr_returns_first_match() {
        let attrs = vec![("lang", "ts"), ("lang", "jsx")];
        assert_eq!(find_attr(&attrs, "lang"), Some("ts".to_string()));
    }

    // ═══════════════════════════════════════════════════════════
    // parse_vue_snapshot
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Script setup only: has_script=true, has_template=false
    #[test]
    fn parse_vue_snapshot_script_setup_only() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert!(snap.slices.script.is_some());
        assert!(snap.slices.template.is_none());
        assert_eq!(snap.descriptor.script_count, 1);
        assert_eq!(snap.descriptor.template_count, 0);
    }

    /// @ai-generated - Template only: has_template=true, has_script=false
    #[test]
    fn parse_vue_snapshot_template_only() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_template);
        assert!(!snap.meta.has_script);
        assert!(snap.slices.template.is_some());
        assert!(snap.slices.script.is_none());
        assert_eq!(snap.descriptor.template_count, 1);
        assert_eq!(snap.descriptor.script_count, 0);
    }

    /// @ai-generated - Full SFC: all blocks present
    #[test]
    fn parse_vue_snapshot_full_sfc() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script>\n<template><div>{{n}}</div></template>\n<style>.a{color:red}</style>",
            AnalysisScope::LSP,
        );
        assert!(snap.meta.has_script);
        assert!(snap.meta.has_template);
        assert!(snap.slices.script.is_some());
        assert!(snap.slices.template.is_some());
        assert_eq!(snap.slices.styles.len(), 1);
        assert_eq!(snap.descriptor.script_count, 1);
        assert_eq!(snap.descriptor.template_count, 1);
        assert_eq!(snap.descriptor.style_count, 1);
    }

    /// @ai-generated - Multiple styles: correct count and langs
    #[test]
    fn parse_vue_snapshot_multiple_styles() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style>.a{}</style><style lang=\"scss\">.b{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.slices.styles.len(), 2);
        assert_eq!(snap.descriptor.style_count, 2);
        assert_eq!(snap.meta.style_langs.len(), 2);
    }

    /// @ai-generated - Custom block detection
    #[test]
    fn parse_vue_snapshot_custom_block() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><i18n>{\"en\":{\"hi\":\"hello\"}}</i18n>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.descriptor.custom_count, 1);
        assert_eq!(snap.meta.custom_types, vec!["i18n"]);
        assert_eq!(snap.slices.custom.len(), 1);
    }

    /// @ai-generated - Empty string doesn't panic, all counts zero
    #[test]
    fn parse_vue_snapshot_empty_sfc() {
        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", "", AnalysisScope::LSP);
        assert!(!snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert_eq!(snap.descriptor.script_count, 0);
        assert_eq!(snap.descriptor.template_count, 0);
        assert_eq!(snap.descriptor.style_count, 0);
        assert_eq!(snap.descriptor.custom_count, 0);
    }

    /// @ai-generated - Script with src produces external_requests
    #[test]
    fn parse_vue_snapshot_script_with_src() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<script setup src=\"./script.ts\"></script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert!(!snap.external_requests.is_empty());
        assert!(!snap.src_blocks.is_empty());
        assert_eq!(snap.src_blocks[0].tag_name, "script");
    }

    /// @ai-generated - Scoped style fingerprint contains scoped info
    #[test]
    fn parse_vue_snapshot_scoped_style_fingerprint() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style scoped>.a{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.descriptor.style_count, 1);
        let fp = &snap.descriptor.style_attr_fingerprints[0];
        assert!(fp.contains("scoped=true"), "fingerprint: {}", fp);
    }

    /// @ai-generated - Style lang is detected
    #[test]
    fn parse_vue_snapshot_style_lang() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style lang=\"scss\">.a{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.style_langs[0], Some("scss".to_string()));
    }

    /// @ai-generated - script_lang is extracted from <script lang="ts">
    #[test]
    fn parse_vue_snapshot_script_lang_ts() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup lang=\"ts\">const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.script_lang, Some("ts".to_string()));
    }

    /// @ai-generated - script_lang extracted from multiline SFC with export type
    #[test]
    fn parse_vue_snapshot_script_lang_ts_multiline() {
        let (snap, _parsed) = parse_vue_snapshot(
            "SideMenu.vue",
            r#"<script setup lang="ts">
import type { MenuItems } from './types.ts'
import { computed } from 'vue'

export type NavigatePayload =
  | { type: 'notification'; to: string }
  | { type: 'menu-item'; to: string }

interface SideMenuProps {
  visible?: boolean
  menuItems?: MenuItems[]
}

const props = defineProps<SideMenuProps>()
const isOpen = computed(() => props.visible)
</script>

<template><div>{{ isOpen }}</div></template>

<style lang="scss" scoped>
.menu { color: red; }
</style>"#,
            AnalysisScope::LSP,
        );
        assert_eq!(
            snap.meta.script_lang,
            Some("ts".to_string()),
            "script_lang should be 'ts' for multiline SFC with lang=\"ts\""
        );
    }

    /// @ai-generated - script_lang is None when no lang attribute
    #[test]
    fn parse_vue_snapshot_script_lang_none() {
        let (snap, _parsed) = parse_vue_snapshot(
            "Comp.vue",
            "<script setup>const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.script_lang, None);
    }

    /// @ai-generated - Deterministic: same source → identical hashes
    #[test]
    fn parse_vue_snapshot_deterministic_hashes() {
        let src = "<script setup>const n = 1</script><template><div>{{n}}</div></template>";
        let (snap1, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let (snap2, _) = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        assert_eq!(snap1.whole_hash, snap2.whole_hash);
        assert_eq!(snap1.semantic_hash, snap2.semantic_hash);
        assert_eq!(snap1.slices.script, snap2.slices.script);
        assert_eq!(snap1.slices.template, snap2.slices.template);
    }

    // ═══════════════════════════════════════════════════════════
    // parse_non_sfc_snapshot
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - Non-SFC whole_hash differs per content
    #[test]
    fn parse_non_sfc_whole_hash_differs() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1");
        let b = parse_non_sfc_snapshot("b.ts", "export const y = 2");
        assert_ne!(a.whole_hash, b.whole_hash);
    }

    /// @ai-generated - Non-SFC semantic_hash is content-dependent so callers
    /// can detect when an imported .ts file changes.
    #[test]
    fn parse_non_sfc_semantic_hash_content_dependent() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1");
        let b = parse_non_sfc_snapshot("b.ts", "export const y = 2");
        assert_ne!(
            a.semantic_hash, b.semantic_hash,
            "different non-SFC content must produce different semantic hashes"
        );
    }

    /// @ai-generated - Non-SFC semantic_hash is deterministic
    #[test]
    fn parse_non_sfc_semantic_hash_deterministic() {
        let a = parse_non_sfc_snapshot("a.ts", "export const x = 1");
        let b = parse_non_sfc_snapshot("a.ts", "export const x = 1");
        assert_eq!(a.semantic_hash, b.semantic_hash);
    }

    /// @ai-generated - <template src="..."> produces ExternalSourceRequest
    /// with ExternalBlockKind::Template
    #[test]
    fn parse_vue_snapshot_template_src_external_request() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<template src=\"./t.html\"></template><script setup>const n=1</script>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.external_requests.len(), 1);
        assert_eq!(
            snap.external_requests[0].block_kind,
            ExternalBlockKind::Template
        );
        assert_eq!(snap.external_requests[0].specifier, "./t.html");
        assert_eq!(
            snap.external_requests[0].resolved_canonical_id,
            "/src/t.html"
        );
        assert_eq!(snap.src_blocks[0].tag_name, "template");
    }

    /// @ai-generated - <style src="..."> produces ExternalSourceRequest
    /// with ExternalBlockKind::Style
    #[test]
    fn parse_vue_snapshot_style_src_external_request() {
        let (snap, _parsed) = parse_vue_snapshot(
            "/src/Comp.vue",
            "<template><div/></template><style src=\"./s.css\"></style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.external_requests.len(), 1);
        assert_eq!(
            snap.external_requests[0].block_kind,
            ExternalBlockKind::Style
        );
        assert_eq!(snap.external_requests[0].specifier, "./s.css");
        assert_eq!(
            snap.external_requests[0].resolved_canonical_id,
            "/src/s.css"
        );
        assert_eq!(snap.src_blocks[0].tag_name, "style");
    }

    /// @ai-generated - Vapor flag detection on <template vapor>
    #[test]
    fn parse_vue_snapshot_vapor_detection() {
        let (snap_normal, _) = parse_vue_snapshot(
            "Comp.vue",
            "<template><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(
            !snap_normal.descriptor.vapor,
            "normal template should not be vapor"
        );

        let (snap_vapor, _) = parse_vue_snapshot(
            "Comp.vue",
            "<template vapor><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(
            snap_vapor.descriptor.vapor,
            "template with vapor attribute should be detected"
        );
    }

    /// @ai-generated - Non-SFC has no blocks
    #[test]
    fn parse_non_sfc_no_blocks() {
        let snap = parse_non_sfc_snapshot("helper.ts", "const x = 1");
        assert!(!snap.meta.has_script);
        assert!(!snap.meta.has_template);
        assert_eq!(snap.descriptor.script_count, 0);
        assert_eq!(snap.descriptor.template_count, 0);
        assert!(snap.external_requests.is_empty());
        assert!(snap.src_blocks.is_empty());
    }

    // ═══════════════════════════════════════════════════════════
    // Script span SFC-absolute adjustment
    // ═══════════════════════════════════════════════════════════

    /// @ai-generated - combined_offset_to_sfc: single block maps offset correctly
    #[test]
    fn combined_offset_to_sfc_single_block() {
        // Script content starts at SFC offset 45, length 30
        let blocks = [(45u32, 30u32)];
        assert_eq!(combined_offset_to_sfc(0, &blocks), 45);
        assert_eq!(combined_offset_to_sfc(7, &blocks), 52);
        assert_eq!(combined_offset_to_sfc(29, &blocks), 74);
    }

    /// @ai-generated - combined_offset_to_sfc: dual blocks map offsets correctly
    #[test]
    fn combined_offset_to_sfc_dual_blocks() {
        // <script> content at SFC offset 10, length 20
        // <script setup> content at SFC offset 60, length 30
        // combined_content = source[10..30] + "\n" + source[60..90]
        let blocks = [(10u32, 20u32), (60u32, 30u32)];
        // Offset in first block
        assert_eq!(combined_offset_to_sfc(0, &blocks), 10);
        assert_eq!(combined_offset_to_sfc(19, &blocks), 29);
        // Offset in second block (after the \n separator at position 20)
        assert_eq!(combined_offset_to_sfc(21, &blocks), 60);
        assert_eq!(combined_offset_to_sfc(30, &blocks), 69);
        assert_eq!(combined_offset_to_sfc(50, &blocks), 89);
    }

    /// @ai-generated - Script binding spans become SFC-absolute after parsing
    #[test]
    fn script_analysis_spans_are_sfc_absolute() {
        // Template block takes bytes 0..48, script starts after
        let source = r#"<template><div>{{ msg }}</div></template>
<script setup>
const msg = 'hello'
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        // Find the "msg" binding
        let binding = snap
            .script_analysis
            .bindings
            .iter()
            .find(|b| b.name == "msg")
            .expect("should find 'msg' binding");

        // The span should point to "msg" in "const msg = 'hello'" within the SFC source
        let script_line = "const msg = 'hello'";
        let msg_in_script = source.find(script_line).unwrap() + "const ".len();
        assert_eq!(
            binding.span.start as usize, msg_in_script,
            "span.start should be SFC-absolute offset of 'msg' in script, got {} expected {}",
            binding.span.start, msg_in_script
        );
        assert_eq!(
            binding.span.end as usize,
            msg_in_script + "msg".len(),
            "span.end should be SFC-absolute end of 'msg' in script"
        );
    }

    /// @ai-generated - Import spans become SFC-absolute after parsing
    #[test]
    fn import_analysis_spans_are_sfc_absolute() {
        let source = r#"<template><div/></template>
<script setup>
import { ref } from 'vue'
const x = ref(0)
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        let import = snap
            .script_analysis
            .imports
            .iter()
            .find(|i| i.source == "vue")
            .expect("should find vue import");

        // import statement should have SFC-absolute span
        let import_line = "import { ref } from 'vue'";
        let import_offset = source.find(import_line).unwrap();
        assert_eq!(
            import.span.start as usize, import_offset,
            "import span.start should be SFC-absolute"
        );

        // "ref" binding inside the import should also be SFC-absolute
        let ref_binding = import
            .bindings
            .iter()
            .find(|b| b.name == "ref")
            .expect("should find 'ref' binding");
        // "ref" appears after "import { "
        let ref_in_import = source.find("{ ref }").unwrap() + 2; // past "{ "
        assert_eq!(
            ref_binding.span.start as usize, ref_in_import,
            "import binding span.start should be SFC-absolute"
        );
    }

    // ═══════════════════════════════════════════════════════════
    // Preprocessor request tests
    // ═══════════════════════════════════════════════════════════

    #[test]
    fn vue_api_callback_param_spans_are_sfc_absolute() {
        let source = r#"<template><div/></template>
<script setup>
import { watch, ref } from 'vue'
const count = ref(0)
watch(count, (value, oldValue) => {
  console.log(value, oldValue)
})
</script>"#;
        let (snap, _parsed) = parse_vue_snapshot("App.vue", source, AnalysisScope::LSP);

        let watch_call = snap
            .script_analysis
            .vue_api_calls
            .iter()
            .find(|call| call.api == verter_analysis::VueApiClassification::Watch)
            .expect("should find watch() call");

        assert_eq!(watch_call.callback_params.len(), 2);

        let value_start = source.find("(value, oldValue)").unwrap() + 1;
        let old_value_start = source.find("oldValue").unwrap();
        assert_eq!(
            watch_call.callback_params[0].span.start as usize,
            value_start
        );
        assert_eq!(
            watch_call.callback_params[1].span.start as usize,
            old_value_start
        );
    }

    /// @ai-generated - template_lang captured for pug
    #[test]
    fn parse_captures_template_lang_pug() {
        let source =
            "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(
            snap.meta.template_lang,
            Some("pug".to_string()),
            "template_lang should be 'pug'"
        );
    }

    /// @ai-generated - no template_lang for plain HTML template
    #[test]
    fn no_template_lang_for_html() {
        let source =
            "<template><div>hello</div></template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.meta.template_lang.is_none(),
            "template_lang should be None for native HTML"
        );
    }

    /// @ai-generated - explicit lang="html" is treated as native (no preprocessor request)
    #[test]
    fn no_template_lang_for_explicit_html() {
        let source = "<template lang=\"html\"><div>hello</div></template>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.meta.template_lang.is_none(),
            "template_lang should be None for explicit lang='html'"
        );
        assert!(
            snap.preprocessor_requests.is_empty(),
            "no preprocessor requests for native HTML"
        );
    }

    /// @ai-generated - preprocessor request for pug template
    #[test]
    fn preprocessor_request_for_pug_template() {
        let source = "<template lang=\"pug\">\ndiv hello\n</template>\n<script setup>\nconst x = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Template);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "pug");
        assert!(
            req.content.contains("div hello"),
            "content should contain 'div hello', got: {}",
            req.content
        );
    }

    /// @ai-generated - preprocessor request for coffee script
    #[test]
    fn preprocessor_request_for_coffee_script() {
        let source =
            "<template><div>hello</div></template>\n<script lang=\"coffee\">\nx = 1\n</script>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Script);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "coffee");
        assert!(
            req.content.contains("x = 1"),
            "content should contain 'x = 1', got: {}",
            req.content
        );
    }

    /// @ai-generated - no preprocessor requests for native langs
    #[test]
    fn no_preprocessor_requests_for_native_langs() {
        let source =
            "<template><div>hello</div></template>\n<script lang=\"ts\" setup>\nconst x = 1\n</script>\n<style>.a { color: red }</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert!(
            snap.preprocessor_requests.is_empty(),
            "no preprocessor requests for html + ts + css"
        );
    }

    /// @ai-generated - preprocessor request for scss style
    #[test]
    fn preprocessor_request_for_scss_style() {
        let source = "<template><div>hello</div></template>\n<style lang=\"scss\">\n.a { .b { color: red } }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Style);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "scss");
        assert!(
            req.content.contains(".a { .b"),
            "content should contain '.a {{ .b', got: {}",
            req.content
        );
    }

    /// @ai-generated - preprocessor request for custom block with lang
    #[test]
    fn preprocessor_request_for_custom_block_with_lang() {
        let source = "<template><div>hello</div></template>\n<i18n lang=\"yaml\">\nen:\n  hello: world\n</i18n>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(snap.preprocessor_requests.len(), 1);
        let req = &snap.preprocessor_requests[0];
        assert_eq!(req.block_type, PreprocessorBlockType::Custom);
        assert_eq!(req.index, 0);
        assert_eq!(req.lang, "yaml");
        assert!(
            req.content.contains("hello: world"),
            "content should contain 'hello: world', got: {}",
            req.content
        );
    }

    /// @ai-generated - multiple preprocessor requests for mixed non-native langs
    #[test]
    fn multiple_preprocessor_requests_for_mixed_langs() {
        let source = "<template lang=\"pug\">\ndiv hello\n</template>\n<script lang=\"coffee\">\nx = 1\n</script>\n<style lang=\"scss\">\n.a { .b { color: red } }\n</style>";
        let (snap, _parsed) = parse_vue_snapshot("test.vue", source, AnalysisScope::NONE);
        assert_eq!(
            snap.preprocessor_requests.len(),
            3,
            "should have 3 preprocessor requests: template, script, style"
        );

        // Verify each type is present
        let types: Vec<_> = snap
            .preprocessor_requests
            .iter()
            .map(|r| r.block_type)
            .collect();
        assert!(types.contains(&PreprocessorBlockType::Template));
        assert!(types.contains(&PreprocessorBlockType::Script));
        assert!(types.contains(&PreprocessorBlockType::Style));
    }

    #[test]
    fn parse_non_sfc_dts_does_not_panic() {
        // This .d.ts content with tuple types triggers an OXC panic when parsed
        // with SourceType::ts() instead of SourceType::d_ts().
        let dts_content = r#"
export type Slot<T extends any = any> = (...args: [T] | (T extends undefined ? [] : never)) => VNode[];
type InternalSlots = { [name: string]: Slot | undefined; };
export declare function defineComponent<T>(options: T): T;
export type VNodeRef = string | Ref | ((ref: Element | null, refs: Record<string, any>) => void);
"#;
        // Should not panic — previously crashed with unwrap() on None in oxc_ast::ts.rs
        let snapshot = parse_non_sfc_snapshot(
            "node_modules/@vue/runtime-core/dist/runtime-core.d.ts",
            dts_content,
        );
        // Verify no panic diagnostics were emitted
        assert!(
            snapshot.parse_diagnostics.diagnostics.is_empty(),
            "should not have parse diagnostics for valid .d.ts content"
        );
    }

    /// Non-SFC analysis scope includes VUE_API_USAGE, MACROS, MACRO_TYPE_DEPS,
    /// EXPORT_SIGNATURES, and SCRIPT_USAGES (all script-applicable flags).
    #[test]
    fn parse_non_sfc_expanded_analysis_scope() {
        // A .ts file that uses Vue APIs at top level — provide() call detected
        let source = r#"import { provide, ref, onMounted } from 'vue'
const count = ref(0)
provide('counter', count)
onMounted(() => { console.log('mounted') })
"#;
        let snap = parse_non_sfc_snapshot("composable.ts", source);
        let analysis = &snap.script_analysis;
        // Positive: VUE_API_USAGE should detect provide() and onMounted() calls
        assert!(
            !analysis.vue_api_calls.is_empty(),
            "should detect vue API calls (provide, onMounted) in non-SFC .ts file"
        );
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .any(|c| c.api == verter_analysis::VueApiClassification::Provide),
            "should detect provide() call"
        );
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .any(|c| c.api == verter_analysis::VueApiClassification::OnMounted),
            "should detect onMounted() call"
        );
        // Positive: IMPORTS should be populated
        assert!(!analysis.imports.is_empty(), "should have imports from vue");
        // Positive: BINDINGS should be populated
        assert!(
            !analysis.bindings.is_empty(),
            "should have bindings (count)"
        );
        // Negative: should not have lifecycle hooks that aren't in the source
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .all(|c| c.api != verter_analysis::VueApiClassification::OnUnmounted),
            "should not detect onUnmounted which isn't in the source"
        );
    }

    #[test]
    fn parse_non_sfc_dts_variants() {
        // All .d.ts extension variants should use the correct SourceType
        let content = "export declare const foo: string;";
        for id in &["types.d.ts", "index.d.mts", "utils.d.cts"] {
            let snapshot = parse_non_sfc_snapshot(id, content);
            assert!(
                snapshot.parse_diagnostics.diagnostics.is_empty(),
                "{id} should parse without diagnostics"
            );
        }
    }
}
