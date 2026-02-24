//! SFC tokenization, block hashing, and `ParseSnapshot` construction.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use oxc_allocator::Allocator;
use oxc_span::SourceType;

use verter_core::diagnostics::{DiagnosticSeverity, SyntaxPluginContext, SyntaxPluginOptions};
use verter_core::parser::Syntax;
use verter_core::tokenizer::byte::tokenize;
use verter_core::types::NodeProp;

use crate::hash::{hash_16, semantic_hash};
use crate::id::resolve_external;
use crate::types::{
    DescriptorMin, DiagnosticsSnapshot, ExternalBlockKind, ExternalSourceRequest, FileMeta,
    HostDiagnostic, HostSeverity, ParseSnapshot, SliceHashes, SrcBlockInfo,
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
) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());

    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };

    let mut syntax = Syntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));

    let mut script_hashes = Vec::new();
    let mut script_attrs_fp = Vec::new();
    let mut script_count = 0;
    let mut has_script = false;
    let mut script_lang: Option<String> = None;
    let mut src_blocks = Vec::new();
    let mut external_requests = Vec::new();

    for (idx, script) in [syntax.script(), syntax.script_setup()]
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
        script_attrs_fp.push(normalize_attr_map(&attrs, &["setup", "lang", "src"]));
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

    if let Some(ast) = syntax.template_ast() {
        template_count = 1;
        has_template = true;
        if let Some(content) = ast.root.content.as_ref() {
            template_hash = Some(hash_16(
                &source.as_bytes()[content.start as usize..content.end as usize],
            ));
        } else {
            template_hash = Some(hash_16(&[]));
        }

        let attrs = extract_attrs(&ast.root.attributes, source);
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

    for (idx, style) in syntax.style_nodes().iter().enumerate() {
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
    }

    let mut custom_hashes = Vec::new();
    let mut custom_attrs_fp = Vec::new();
    let mut custom_types = Vec::new();
    let mut custom_langs = Vec::new();

    for (idx, custom) in syntax.unknown_nodes().iter().enumerate() {
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
        vapor: syntax.is_vapor(),
    };

    let slices = SliceHashes {
        script: script_hash,
        template: template_hash,
        styles: style_hashes,
        custom: custom_hashes,
    };

    let semantic_hash = semantic_hash(&slices, &descriptor);

    let raw_diags = syntax.take_diagnostics();
    // Build UTF-16 resolver lazily — only when diagnostics have spans
    let resolver = if raw_diags.iter().any(|d| d.span.is_some()) {
        Some(verter_core::cursor::position::PositionResolver::new(source))
    } else {
        None
    };
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
                span_start: d.span.map(|s| {
                    resolver
                        .as_ref()
                        .map(|r| r.offset_to_line_col(s.start as usize).2 as u32)
                        .unwrap_or(s.start)
                }),
                span_end: d.span.map(|s| {
                    resolver
                        .as_ref()
                        .map(|r| r.offset_to_line_col(s.end as usize).2 as u32)
                        .unwrap_or(s.end)
                }),
            })
            .collect(),
    );

    // Build style analyses for each style block (when style analysis flags are set)
    let style_analyses: Vec<verter_analysis::StyleBlockAnalysis> =
        if analysis_scope.needs_style_analysis() {
            syntax
                .style_nodes()
                .iter()
                .map(|style| build_single_style_analysis(style, source))
                .collect()
        } else {
            Vec::new()
        };

    // Build script analysis from script block contents (when script analysis flags are set)
    let (script_analysis, script_panic_diag) = if analysis_scope.needs_script_analysis() {
        build_script_analysis_from_syntax(&syntax, source)
    } else {
        (verter_analysis::ScriptAnalysisSnapshot::default(), None)
    };

    // Merge any panic diagnostic into parse diagnostics
    let parse_diagnostics = if let Some(diag) = script_panic_diag {
        parse_diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![diag]))
    } else {
        parse_diagnostics
    };

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
    }
}

/// Build a single style analysis from a parsed style node and the SFC source.
/// Shared by `parse_vue_snapshot()` (eager) and `build_style_analyses_from_source()` (on-demand).
fn build_single_style_analysis(
    style: &verter_core::parser::types::RootNodeStyle,
    source: &str,
) -> verter_analysis::StyleBlockAnalysis {
    let module_name =
        find_attr(&extract_attrs(&style.attributes, source), "module").filter(|v| v != "true");
    let vue_input = verter_analysis::VueStyleInput::default();
    let analysis_lang = match style.lang {
        Some(verter_core::parser::types::StyleLang::Css) | None => {
            let css_content = style
                .content
                .map(|span| &source[span.start as usize..span.end as usize])
                .unwrap_or("");
            return verter_analysis::build_css_style_analysis(
                css_content,
                vue_input,
                style.scoped,
                style.module,
                module_name.as_deref(),
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
                span_start: None,
                span_end: None,
            };
            (T::default(), Some(diagnostic))
        }
    }
}

/// Build script analysis from an already-parsed Syntax. Concatenates script block
/// contents and runs OXC analysis with catch_unwind for panic safety.
/// Shared by `parse_vue_snapshot()` (eager) and `build_script_analysis_from_source()` (on-demand).
fn build_script_analysis_from_syntax(
    syntax: &Syntax,
    source: &str,
) -> (
    verter_analysis::ScriptAnalysisSnapshot,
    Option<HostDiagnostic>,
) {
    let mut combined_content = String::new();
    for script in [syntax.script(), syntax.script_setup()]
        .into_iter()
        .flatten()
    {
        if let Some(span) = script.content {
            let content = &source[span.start as usize..span.end as usize];
            if !combined_content.is_empty() {
                combined_content.push('\n');
            }
            combined_content.push_str(content);
        }
    }
    if combined_content.is_empty() {
        return (verter_analysis::ScriptAnalysisSnapshot::default(), None);
    }
    let alloc = Allocator::new();
    catch_analysis_panic(
        "script analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_analysis::build_script_analysis(&combined_content, SourceType::ts(), &alloc)
        }),
    )
}

/// Compute script analysis on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_script_analysis_from_source(
    source: &str,
) -> verter_analysis::ScriptAnalysisSnapshot {
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));
    let (analysis, _diag) = build_script_analysis_from_syntax(&syntax, source);
    analysis
}

/// Compute style analyses on demand from SFC source. Used by get_analysis()
/// when eager_analysis was false during upsert().
pub(crate) fn build_style_analyses_from_source(
    source: &str,
) -> Vec<verter_analysis::StyleBlockAnalysis> {
    let opts = SyntaxPluginOptions::default();
    let ctx = SyntaxPluginContext {
        input: source,
        bytes: source.as_bytes(),
        options: &opts,
        diagnostics: Vec::new(),
    };
    let mut syntax = Syntax::new(false);
    tokenize(source.as_bytes(), |e| syntax.handle(&e, &ctx));
    syntax
        .style_nodes()
        .iter()
        .map(|style| build_single_style_analysis(style, source))
        .collect()
}

pub(crate) fn parse_non_sfc_snapshot(_canonical_id: &str, source: &str) -> ParseSnapshot {
    let whole_hash = hash_16(source.as_bytes());
    let slices = SliceHashes::default();
    let descriptor = DescriptorMin::default();
    // Use whole_hash as semantic_hash so content changes in non-SFC files
    // (e.g. .ts files imported for type resolution) are properly detected
    // and trigger dependent SFC recompilation.
    let semantic_hash = whole_hash;

    let alloc = Allocator::new();
    let (export_signatures, panic_diag) = catch_analysis_panic(
        "export signature analysis",
        std::panic::AssertUnwindSafe(|| {
            verter_analysis::build_export_signatures(source, SourceType::ts(), &alloc)
        }),
    );

    let parse_diagnostics = match panic_diag {
        Some(diag) => DiagnosticsSnapshot::from_vec(vec![diag]),
        None => DiagnosticsSnapshot::default(),
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
        script_analysis: verter_analysis::ScriptAnalysisSnapshot::default(),
        export_signatures,
        style_analyses: Vec::new(),
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot("Comp.vue", "", AnalysisScope::LSP);
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
            "Comp.vue",
            "<template><div/></template><style lang=\"scss\">.a{}</style>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.style_langs[0], Some("scss".to_string()));
    }

    /// @ai-generated - script_lang is extracted from <script lang="ts">
    #[test]
    fn parse_vue_snapshot_script_lang_ts() {
        let snap = parse_vue_snapshot(
            "Comp.vue",
            "<script setup lang=\"ts\">const n = 1</script><template><div/></template>",
            AnalysisScope::LSP,
        );
        assert_eq!(snap.meta.script_lang, Some("ts".to_string()));
    }

    /// @ai-generated - script_lang extracted from multiline SFC with export type
    #[test]
    fn parse_vue_snapshot_script_lang_ts_multiline() {
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap1 = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
        let snap2 = parse_vue_snapshot("Comp.vue", src, AnalysisScope::LSP);
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
        let snap = parse_vue_snapshot(
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
        let snap = parse_vue_snapshot(
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
        let snap_normal = parse_vue_snapshot(
            "Comp.vue",
            "<template><div>hello</div></template>",
            AnalysisScope::LSP,
        );
        assert!(
            !snap_normal.descriptor.vapor,
            "normal template should not be vapor"
        );

        let snap_vapor = parse_vue_snapshot(
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
}
