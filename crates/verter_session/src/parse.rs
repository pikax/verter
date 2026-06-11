//! SFC tokenization, block hashing, and `ParseSnapshot` construction.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::Program;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

use verter_compiler::compile::parse_sfc;
use verter_compiler::diagnostics::DiagnosticSeverity;
use verter_compiler::parser::types::ParsedSfc;
use verter_compiler::types::NodeProp;

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
    analysis_scope: verter_semantic::analysis::AnalysisScope,
) -> (ParseSnapshot, Arc<verter_language::FrameworkParseArtifact>) {
    let parsed = Arc::new(parse_sfc(source, None, None));
    let snapshot = build_vue_snapshot_from_parsed(canonical_id, source, analysis_scope, &parsed);
    let artifact = verter_compiler::framework_common::vue_bridge::build_vue_parse_artifact(
        source,
        parsed,
        crate::file_artifact_store::LEGACY_PARSER_VERSION,
    );

    (snapshot, artifact)
}

/// Parse Vue SFC source straight into the framework-neutral parse
/// artifact (the route-owned cold-parse producer's entry — no
/// `ParseSnapshot` is needed there).
pub(crate) fn build_vue_parse_artifact_from_source(
    source: &str,
) -> Arc<verter_language::FrameworkParseArtifact> {
    let parsed = Arc::new(parse_sfc(source, None, None));
    verter_compiler::framework_common::vue_bridge::build_vue_parse_artifact(
        source,
        parsed,
        crate::file_artifact_store::LEGACY_PARSER_VERSION,
    )
}

pub(crate) fn non_sfc_source_type(canonical_id: &str) -> SourceType {
    if canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
    {
        SourceType::d_ts()
    } else {
        SourceType::ts()
    }
}

/// Pure source-type computation for an imported eval target.
///
/// Single source of truth; the scheduler caches its result on
/// [`crate::host_executor::HostSourceData::source_type`] so cache-key callers
/// can read the authoritative value via
/// [`crate::VerterHost::authoritative_source_type_for`] instead of recomputing
/// from `(canonical_id, raw_source, framework_parse)` — a pair that is
/// unstable when `framework_parse` is dropped mid-resolution.
///
/// Dispatches on the file's resolved [`FileLanguage`](verter_language::FileLanguage)
/// row: framework carriers read the neutral
/// `FrameworkParseCommon.script_regions[].source_type` their producer
/// populated at parse time (UNIFORMLY — no per-carrier downcast); plain
/// scripts derive from the canonical path.
pub(crate) fn imported_eval_source_type(
    file_language: &verter_language::FileLanguage,
    canonical_id: &str,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
) -> SourceType {
    if file_language.is_framework_carrier() {
        framework_parse
            .and_then(|artifact| artifact.common.script_regions.first())
            .map(|region| oxc_source_type_from_neutral(region.source_type))
            .unwrap_or_else(SourceType::ts)
    } else {
        non_sfc_source_type(canonical_id)
    }
}

/// Map the neutral [`verter_language::ScriptSourceType`] dialect onto
/// the OXC [`SourceType`] the parser pipeline consumes.
pub(crate) fn oxc_source_type_from_neutral(
    source_type: verter_language::ScriptSourceType,
) -> SourceType {
    match source_type {
        verter_language::ScriptSourceType::Ts => SourceType::ts(),
        verter_language::ScriptSourceType::Tsx => SourceType::tsx(),
        verter_language::ScriptSourceType::Js => SourceType::script(),
        verter_language::ScriptSourceType::Jsx => SourceType::jsx(),
        verter_language::ScriptSourceType::Dts => SourceType::d_ts(),
    }
}

pub(crate) fn build_vue_snapshot_from_parsed(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_semantic::analysis::AnalysisScope,
    parsed: &ParsedSfc,
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());

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
    let style_analyses: Vec<verter_semantic::analysis::StyleBlockAnalysis> =
        if analysis_scope.needs_style_analysis() {
            build_style_analyses_from_parsed(parsed, source, canonical_id)
        } else {
            Vec::new()
        };

    // Vue SFCs are still modules: we need named export signatures from the
    // script content even when full script analysis is disabled so barrel
    // re-export resolution can find `export type Foo = ...` in `.vue` files.
    let (export_signatures, export_panic_diag) =
        build_export_signatures_from_parsed_with_diagnostic(parsed, source);

    // Build script analysis from script block contents (when script analysis flags are set)
    let (mut script_analysis, script_panic_diag) = if analysis_scope.needs_script_analysis() {
        build_script_analysis_from_parsed_with_diagnostic(parsed, source)
    } else {
        (
            verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
            None,
        )
    };

    // Cross-reference: mark script bindings that are referenced by CSS v-bind() in style blocks
    if !style_analyses.is_empty() && !script_analysis.bindings.is_empty() {
        script_analysis.mark_bindings_used_in_style(&style_analyses);
    }

    // Merge any panic diagnostic into parse diagnostics
    let mut extra_diags = Vec::new();
    if let Some(diag) = export_panic_diag {
        extra_diags.push(diag);
    }
    if let Some(diag) = script_panic_diag {
        extra_diags.push(diag);
    }
    let parse_diagnostics = if extra_diags.is_empty() {
        parse_diagnostics
    } else {
        parse_diagnostics.merge(DiagnosticsSnapshot::from_vec(extra_diags))
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
        script_analysis: Arc::new(script_analysis),
        export_signatures,
        style_analyses,
        preprocessor_requests,
    }
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
    style: &verter_compiler::parser::types::RootNodeStyle,
    source: &str,
    canonical_id: &str,
) -> verter_semantic::analysis::StyleBlockAnalysis {
    let module_name =
        find_attr(&extract_attrs(&style.attributes, source), "module").filter(|v| v != "true");
    let content_offset = style.content.map(|span| span.start).unwrap_or(0);

    // Extract CSS content from the SFC source
    let css_content = style
        .content
        .map(|span| &source[span.start as usize..span.end as usize])
        .unwrap_or("");

    // Run CSS prepass to extract v-bind() expressions and their generated variable names
    let component_name = verter_compiler::compile::extract_component_name(canonical_id);
    let scope_id = verter_compiler::compile::get_hash(&component_name);
    let prepass_result = verter_compiler::css::prepass::prepass(css_content, &scope_id);

    // Build VueStyleInput from prepass results
    let vue_input = verter_semantic::analysis::VueStyleInput {
        v_binds: prepass_result
            .v_bind_vars
            .iter()
            .map(|vb| verter_semantic::analysis::VBindInput {
                expression: vb.expression.clone(),
                quoted: false,
                start: content_offset,
                end: content_offset,
                generated_var_name: Some(vb.var_name.clone()),
            })
            .collect(),
        special_pseudos: vec![],
    };

    let sfc_source_len = source.len() as u32;

    let analysis_lang = match style.lang {
        Some(verter_compiler::parser::types::StyleLang::Css) | None => {
            let analysis = verter_semantic::analysis::build_css_style_analysis(
                css_content,
                vue_input,
                style.scoped,
                style.module,
                module_name.as_deref(),
                content_offset,
            );
            if let Some(css) = &analysis.css {
                css.debug_assert_valid_spans(sfc_source_len);
            }
            return analysis;
        }
        Some(verter_compiler::parser::types::StyleLang::Scss) => {
            verter_semantic::analysis::StyleAnalysisLang::Scss
        }
        Some(verter_compiler::parser::types::StyleLang::Sass) => {
            verter_semantic::analysis::StyleAnalysisLang::Sass
        }
        Some(verter_compiler::parser::types::StyleLang::Less) => {
            verter_semantic::analysis::StyleAnalysisLang::Less
        }
        Some(verter_compiler::parser::types::StyleLang::Stylus) => {
            verter_semantic::analysis::StyleAnalysisLang::Stylus
        }
        Some(verter_compiler::parser::types::StyleLang::Unknown) => {
            verter_semantic::analysis::StyleAnalysisLang::Unknown
        }
    };
    verter_semantic::analysis::build_preprocessor_style_analysis(
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

/// The SFC's OXC source type, resolved from `<script lang>` through the
/// Vue carrier producer's resolver (the one `<script lang>` authority —
/// the same data the producer stamps onto `ScriptRegion.source_type`).
fn vue_oxc_source_type(parsed: &ParsedSfc, source: &str) -> SourceType {
    oxc_source_type_from_neutral(
        verter_compiler::framework_common::vue_bridge::vue_script_source_type(parsed, source),
    )
}

/// Build script analysis from an already-parsed SFC.
///
/// Runs OXC analysis over the **position-preserving** script source
/// ([`crate::host_resolve::extract_vue_script_content`]) — script content at
/// its raw SFC byte offsets, non-script bytes whitespace-blanked — so every
/// span the analyzer produces (including each `AnalyzedMacro.parsed_type_argument`
/// internal `TypeExpr` span) is SFC-absolute by construction. No post-analysis
/// offset translation is required; downstream consumers use the spans directly
/// with `LineIndex::offset_to_position()`.
///
/// Shared by `parse_vue_snapshot()` (eager) and `build_script_analysis_from_parsed()`.
fn build_script_analysis_from_parsed_with_diagnostic(
    parsed: &ParsedSfc,
    source: &str,
) -> (
    verter_semantic::analysis::ScriptAnalysisSnapshot,
    Option<HostDiagnostic>,
) {
    let Some(script_source) = crate::host_resolve::extract_vue_script_content(source, Some(parsed))
    else {
        return (
            verter_semantic::analysis::ScriptAnalysisSnapshot::default(),
            None,
        );
    };
    let source_type = vue_oxc_source_type(parsed, source);
    let alloc = Allocator::new();
    catch_analysis_panic(
        "script analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_semantic::analysis::build_script_analysis(&script_source, source_type, &alloc)
        }),
    )
}

fn build_export_signatures_from_parsed_with_diagnostic(
    parsed: &ParsedSfc,
    source: &str,
) -> (
    Vec<verter_semantic::analysis::ExportSignature>,
    Option<HostDiagnostic>,
) {
    let Some(script_source) = crate::host_resolve::extract_vue_script_content(source, Some(parsed))
    else {
        return (Vec::new(), None);
    };

    let source_type = vue_oxc_source_type(parsed, source);
    let alloc = Allocator::new();
    catch_analysis_panic(
        "export signature analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_semantic::analysis::build_export_signatures(&script_source, source_type, &alloc)
        }),
    )
}

/// Compute script analysis on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_script_analysis_from_source(
    source: &str,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    // On-demand Vue re-parse routes through the Vue carrier producer so
    // the artifact stays the one post-parse representation.
    let artifact = build_vue_parse_artifact_from_source(source);
    build_script_analysis_for_artifact(Some(&artifact), source)
}

pub(crate) fn build_script_analysis_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    let (analysis, _diag) = build_script_analysis_from_parsed_with_diagnostic(parsed, source);
    analysis
}

/// Compute style analyses on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_style_analyses_from_source(
    source: &str,
    canonical_id: &str,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    // On-demand Vue re-parse routes through the Vue carrier producer so
    // the artifact stays the one post-parse representation.
    let artifact = build_vue_parse_artifact_from_source(source);
    build_style_analyses_for_artifact(Some(&artifact), source, canonical_id)
}

pub(crate) fn build_style_analyses_from_parsed(
    parsed: &ParsedSfc,
    source: &str,
    canonical_id: &str,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    parsed
        .style_nodes()
        .iter()
        .map(|style| build_single_style_analysis(style, source, canonical_id))
        .collect()
}

/// Artifact-facing script-analysis builder: reuse the carrier parse
/// when the neutral artifact opens through the blessed Vue accessor,
/// else re-parse from source.
pub(crate) fn build_script_analysis_for_artifact(
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    source: &str,
) -> verter_semantic::analysis::ScriptAnalysisSnapshot {
    match framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse) {
        Some(parsed) => build_script_analysis_from_parsed(parsed, source),
        None => build_script_analysis_from_source(source),
    }
}

/// Artifact-facing style-analysis builder (see
/// [`build_script_analysis_for_artifact`]).
pub(crate) fn build_style_analyses_for_artifact(
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    source: &str,
    canonical_id: &str,
) -> Vec<verter_semantic::analysis::StyleBlockAnalysis> {
    match framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse) {
        Some(parsed) => build_style_analyses_from_parsed(parsed, source, canonical_id),
        None => build_style_analyses_from_source(source, canonical_id),
    }
}

/// Artifact-facing Vue snapshot builder: `Some(snapshot)` when the
/// neutral artifact carries a Vue parse (opened through the blessed
/// accessor), `None` otherwise.
pub(crate) fn build_vue_snapshot_from_artifact(
    canonical_id: &str,
    source: &str,
    analysis_scope: verter_semantic::analysis::AnalysisScope,
    framework_parse: &verter_language::FrameworkParseArtifact,
) -> Option<ParseSnapshot> {
    let parsed = crate::typeinfo::adapters::vue::vue_parse(framework_parse)?;
    Some(build_vue_snapshot_from_parsed(
        canonical_id,
        source,
        analysis_scope,
        parsed,
    ))
}

/// The Vue template-data compile half shared by
/// `build_template_analysis` / `compute_template_analysis_if_missing`
/// (relocated behind the Vue bridge so `host_manage/**` stays free of
/// `ParsedSfc` / `parse_sfc`).
///
/// Parses `compile_source` (re-using the artifact's carrier parse when
/// `reuse_carrier_parse` is set and the artifact opens as Vue), runs
/// the META-target compile, and returns the extracted raw template
/// data. `None` on structural compile errors (type-resolution errors —
/// `XInvalidMacroType` / `XMissingMacroType` — do not block template
/// extraction) or when no template data was produced.
pub(crate) fn compile_vue_template_data(
    canonical_id: &str,
    compile_source: &str,
    framework_parse: Option<&verter_language::FrameworkParseArtifact>,
    reuse_carrier_parse: bool,
) -> Option<verter_compiler::compile::RawTemplateData> {
    let cached = if reuse_carrier_parse {
        framework_parse.and_then(crate::typeinfo::adapters::vue::vue_parse)
    } else {
        None
    };
    let parsed = match cached {
        Some(parsed) => std::borrow::Cow::Borrowed(parsed.as_ref()),
        None => std::borrow::Cow::Owned(parse_sfc(compile_source, None, None)),
    };

    let alloc = Allocator::new();
    let options = verter_compiler::compile::CodegenOptions {
        target: verter_compiler::compile::CompileTarget::META,
        filename: Some(canonical_id.to_string()),
        ..verter_compiler::compile::CodegenOptions::default()
    };
    let verter_opts = verter_compiler::compile::VerterCompileOptions {
        extract_template_data: true,
        ..verter_compiler::compile::VerterCompileOptions::default()
    };
    let compiled = verter_compiler::compile::compile_from_parsed(
        compile_source,
        &parsed,
        &options,
        &verter_opts,
        &alloc,
    );

    // Bail on structural compile errors that would invalidate template
    // data; skip type-resolution errors since template slot extraction
    // does not depend on type resolution.
    let has_structural_errors = compiled.errors.iter().any(|d| {
        matches!(
            d.severity,
            verter_compiler::compile::CompileDiagnosticSeverity::Error,
        ) && !d.code.starts_with("XInvalidMacroType")
            && !d.code.starts_with("XMissingMacroType")
    });
    if has_structural_errors {
        return None;
    }

    compiled.template_data
}

pub(crate) fn build_non_sfc_snapshot_from_program(
    _canonical_id: &str,
    source: &str,
    source_type: SourceType,
    program: &Program<'_>,
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());
    let slices = SliceHashes::default();
    let descriptor = DescriptorMin::default();
    let semantic_hash = whole_hash;

    let export_signatures =
        verter_semantic::analysis::build_export_signatures_from_program(source, program);
    let script_analysis = verter_semantic::analysis::build_script_analysis_with_scope_from_program(
        source,
        source_type,
        program,
        verter_semantic::analysis::AnalysisScope::IMPORTS
            | verter_semantic::analysis::AnalysisScope::BINDINGS
            | verter_semantic::analysis::AnalysisScope::FUNC_RETURNS
            | verter_semantic::analysis::AnalysisScope::REACTIVITY
            | verter_semantic::analysis::AnalysisScope::MACROS
            | verter_semantic::analysis::AnalysisScope::MACRO_TYPE_DEPS
            | verter_semantic::analysis::AnalysisScope::VUE_API_USAGE
            | verter_semantic::analysis::AnalysisScope::EXPORT_SIGNATURES
            | verter_semantic::analysis::AnalysisScope::SCRIPT_USAGES,
    );

    ParseSnapshot {
        whole_hash,
        semantic_hash,
        slices,
        descriptor,
        meta: FileMeta::default(),
        external_requests: Vec::new(),
        src_blocks: Vec::new(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        script_analysis: Arc::new(script_analysis),
        export_signatures,
        style_analyses: Vec::new(),
        preprocessor_requests: Vec::new(),
    }
}

pub(crate) fn parse_non_sfc_snapshot(canonical_id: &str, source: &str) -> ParseSnapshot {
    let source_type = non_sfc_source_type(canonical_id);
    let alloc = Allocator::new();
    let parser = Parser::new(&alloc, source, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return ParseSnapshot {
            whole_hash: hash_16(source.as_bytes()),
            semantic_hash: hash_16(source.as_bytes()),
            slices: SliceHashes::default(),
            descriptor: DescriptorMin::default(),
            meta: FileMeta::default(),
            external_requests: Vec::new(),
            src_blocks: Vec::new(),
            parse_diagnostics: DiagnosticsSnapshot::default(),
            script_analysis: Arc::new(verter_semantic::analysis::ScriptAnalysisSnapshot::default()),
            export_signatures: Vec::new(),
            style_analyses: Vec::new(),
            preprocessor_requests: Vec::new(),
        };
    }

    build_non_sfc_snapshot_from_program(canonical_id, source, source_type, &result.program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use smallvec::SmallVec;
    use verter_compiler::types::NodeProp;
    use verter_semantic::analysis::AnalysisScope;

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

    #[test]
    fn parse_vue_snapshot_collects_named_export_signatures_from_script() {
        let source = r#"<script lang="ts">
export interface Props {
  label: string
}

export type Keys = 'label'
</script>
<template><div /></template>"#;

        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", source, AnalysisScope::NONE);
        let names: Vec<&str> = snap
            .export_signatures
            .iter()
            .map(|sig| sig.name.as_str())
            .collect();

        assert!(
            names.contains(&"Props"),
            "Vue SFC export signatures should include named type exports, got: {names:?}"
        );
        assert!(
            names.contains(&"Keys"),
            "Vue SFC export signatures should include named aliases, got: {names:?}"
        );

        let props_sig = snap
            .export_signatures
            .iter()
            .find(|sig| sig.name == "Props")
            .expect("Props export signature should exist");
        let expected_start = source
            .find("Props")
            .expect("Props identifier should exist in source") as u32;
        assert_eq!(
            props_sig.span.start, expected_start,
            "export signature span should be remapped to SFC-absolute offsets"
        );
    }

    #[test]
    fn parse_vue_snapshot_uses_script_lang_for_script_analysis() {
        let source = r#"<script setup lang="tsx">
const view = <div className="card">hello</div>
</script>
<template><div /></template>"#;

        let (snap, _parsed) = parse_vue_snapshot("Comp.vue", source, AnalysisScope::LSP);
        assert!(
            snap.script_analysis
                .bindings
                .iter()
                .any(|binding| binding.name == "view"),
            "TSX script analysis should respect the SFC script lang and retain bindings"
        );
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
            .find(|call| call.api == verter_semantic::analysis::VueApiClassification::Watch)
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
                .any(|c| c.api == verter_semantic::analysis::VueApiClassification::Provide),
            "should detect provide() call"
        );
        assert!(
            analysis
                .vue_api_calls
                .iter()
                .any(|c| c.api == verter_semantic::analysis::VueApiClassification::OnMounted),
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
                .all(|c| c.api != verter_semantic::analysis::VueApiClassification::OnUnmounted),
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
