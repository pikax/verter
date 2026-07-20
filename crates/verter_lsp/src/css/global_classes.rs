//! Workspace-wide navigation for GLOBAL CSS classes.
//!
//! A class is GLOBAL when a style declaration for it lives in a non-scoped,
//! non-module Vue `<style>` block, or inside a `:global(...)` pseudo of a
//! scoped/module/Svelte block. Global classes get find-all-references across
//! every host-known file: all style declarations of the class in global
//! contexts plus all template/markup usages. Scoped classes NEVER cross the
//! file boundary.
//!
//! `<style module>` is FAIL-CLOSED throughout the css-native class engine:
//! module classes compile to hashed local names, so they are neither global
//! nor addressable by plain `class="foo"` tokens — the only escape is
//! `:global(...)`, which opts a declaration out of module-local hashing.
//! In-file `$style.foo` navigation is OUT of scope here by design: `$style`
//! member access is an expression on the generated TS surface, owned by the
//! typed `$style` TSX injection and the TypeProvider — the css-native engine
//! serves nothing for module classes rather than something mis-mapped.

use tower_lsp_server::ls_types::*;
use verter_session::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::references::{
    collect_template_css_ref_spans, find_css_target_in_style_refs,
    find_css_target_in_template_refs, markup_class_token_at, CssRefTarget,
};

/// The CSS class name at `offset` (template attribute entry, markup class
/// token, or style class occurrence), regardless of scope.
pub(crate) fn css_class_name_at(
    offset: usize,
    source: &str,
    blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
) -> Option<String> {
    let in_style = blocks.iter().any(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });
    if in_style {
        return match find_css_target_in_style_refs(offset, source, analysis) {
            Some(CssRefTarget::Class(name)) => Some(name),
            _ => None,
        };
    }
    if let Some(template) = analysis.template.as_deref() {
        if let Some(CssRefTarget::Class(name)) =
            find_css_target_in_template_refs(offset, source, template)
        {
            return Some(name);
        }
    }
    markup_class_token_at(offset, analysis).map(|t| t.name.clone())
}

/// Whether `class_span` sits inside a `:global(...)` pseudo of `style`.
fn class_in_global_pseudo(
    style: &verter_semantic::analysis::StyleBlockAnalysis,
    class_span: verter_span::Span,
) -> bool {
    style.special_pseudos.iter().any(|p| {
        p.kind == verter_semantic::analysis::SpecialPseudoKind::Global
            && class_span.start >= p.start
            && class_span.end <= p.end
    })
}

/// Whether a class declaration at `class_span` in `style` is a GLOBAL
/// declaration: a non-scoped, non-module block, or a `:global(...)` escape.
fn class_decl_is_global(
    style: &verter_semantic::analysis::StyleBlockAnalysis,
    class_span: verter_span::Span,
) -> bool {
    (!style.scoped && !style.is_module) || class_in_global_pseudo(style, class_span)
}

/// Whether a class declaration at `class_span` in `style` is addressable by
/// PLAIN markup class tokens (`class="foo"`). `<style module>` classes are
/// hashed-local — addressable only through the TS-owned `$style.*` surface —
/// so the css-native class legs fail closed on them, unless the declaration
/// sits inside `:global(...)`.
pub(crate) fn class_plain_addressable(
    style: &verter_semantic::analysis::StyleBlockAnalysis,
    class_span: verter_span::Span,
) -> bool {
    !style.is_module || class_in_global_pseudo(style, class_span)
}

/// Whether `analysis` declares `name` in a GLOBAL context: a non-scoped,
/// non-module style block, or inside `:global(...)` of a scoped/module/
/// Svelte block.
pub(crate) fn class_declared_global(name: &str, analysis: &FileAnalysisSnapshot) -> bool {
    for style in analysis.styles.iter() {
        let Some(css) = style.css.as_ref() else {
            continue;
        };
        for cls in &css.classes {
            if cls.name != name || cls.span.start == 0 {
                continue;
            }
            if class_decl_is_global(style, cls.span) {
                return true;
            }
        }
    }
    false
}

/// The class token at `offset` IF this file declares it globally — the
/// trigger for workspace-wide navigation. Scoped classes return `None`
/// (fail closed: never cross-file).
pub(crate) fn global_class_target_at(
    offset: usize,
    source: &str,
    blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
) -> Option<String> {
    let name = css_class_name_at(offset, source, blocks, analysis)?;
    class_declared_global(&name, analysis).then_some(name)
}

/// Per-file spans for a global class: style declarations in GLOBAL contexts
/// only, plus (unless `declarations_only`) every template/markup usage.
pub(crate) fn global_class_spans_in_file(
    name: &str,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    declarations_only: bool,
) -> Vec<(u32, u32)> {
    let mut spans: Vec<(u32, u32)> = Vec::new();

    for style in analysis.styles.iter() {
        let Some(css) = style.css.as_ref() else {
            continue;
        };
        for cls in &css.classes {
            if cls.name != name || cls.span.start == 0 {
                continue;
            }
            if class_decl_is_global(style, cls.span) {
                spans.push((cls.span.start, cls.span.end));
            }
        }
    }

    if !declarations_only {
        if let Some(template) = analysis.template.as_deref() {
            spans.extend(collect_template_css_ref_spans(
                &CssRefTarget::Class(name.to_string()),
                source,
                template,
            ));
        }
        for token in analysis.markup_class_tokens.iter() {
            if token.name == name {
                spans.push((token.span.start, token.span.end));
            }
        }
    }

    spans
}

/// Collect cross-file locations for a global class across every host-known
/// file (excluding `origin_canonical` — the origin file's spans come from the
/// native same-file path). Candidate files are prefiltered by a cheap source
/// containment check; the actual spans come from each file's typed analysis.
pub(crate) fn collect_cross_file_global_class_locations(
    host: &verter_session::VerterHost,
    origin_canonical: Option<&str>,
    name: &str,
    encoding: PositionEncodingKind,
    declarations_only: bool,
) -> Vec<Location> {
    let mut out: Vec<Location> = Vec::new();
    for (canonical, _language) in host.list_files() {
        if Some(canonical.as_str()) == origin_canonical {
            continue;
        }
        let Some(source) = host.get_source(&canonical) else {
            continue;
        };
        // Cheap candidate prefilter only — the typed analysis below is the
        // sole authority for actual spans.
        if !source.contains(name) {
            continue;
        }
        let Some(analysis) = host.get_analysis(&canonical) else {
            continue;
        };
        let spans = global_class_spans_in_file(name, &source, &analysis, declarations_only);
        if spans.is_empty() {
            continue;
        }
        let Some(uri) = crate::type_provider::merge::file_path_to_uri(&canonical) else {
            continue;
        };
        let line_index = LineIndex::new(&source, encoding.clone());
        for (start, end) in spans {
            if let (Some(s), Some(e)) = (
                line_index.offset_to_position(start),
                line_index.offset_to_position(end),
            ) {
                out.push(Location {
                    uri: uri.clone(),
                    range: Range { start: s, end: e },
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::{build_css_style_analysis, StyleAnalysisLang, VueStyleInput};

    fn style(source: &str, scoped: bool) -> verter_semantic::analysis::StyleBlockAnalysis {
        build_css_style_analysis(source, VueStyleInput::default(), scoped, false, None, 0)
    }

    fn module_style(source: &str) -> verter_semantic::analysis::StyleBlockAnalysis {
        build_css_style_analysis(source, VueStyleInput::default(), false, true, None, 0)
    }

    #[test]
    fn unscoped_declaration_is_global() {
        let analysis = FileAnalysisSnapshot {
            styles: (vec![style(".btn { color: red; }", false)]).into(),
            ..Default::default()
        };
        assert!(class_declared_global("btn", &analysis));
    }

    #[test]
    fn scoped_declaration_is_not_global() {
        let analysis = FileAnalysisSnapshot {
            styles: (vec![style(".btn { color: red; }", true)]).into(),
            ..Default::default()
        };
        assert!(
            !class_declared_global("btn", &analysis),
            "a scoped class must NEVER be treated as global"
        );
    }

    #[test]
    fn module_declaration_is_not_global() {
        // `<style module>` scans with scoped=false, is_module=true. Module
        // classes compile to hashed local names — maximally local, never a
        // workspace-wide target.
        let analysis = FileAnalysisSnapshot {
            styles: (vec![module_style(".btn { color: red; }")]).into(),
            ..Default::default()
        };
        assert!(
            !class_declared_global("btn", &analysis),
            "a `<style module>` class must NEVER be treated as global"
        );
    }

    #[test]
    fn module_declarations_never_enter_global_spans() {
        let analysis = FileAnalysisSnapshot {
            styles: (vec![module_style(".btn { color: red; }")]).into(),
            ..Default::default()
        };
        let decls = global_class_spans_in_file("btn", ".btn { color: red; }", &analysis, true);
        assert!(
            decls.is_empty(),
            "a module declaration is never a cross-file declaration target: {decls:?}"
        );
    }

    #[test]
    fn module_global_pseudo_inner_class_is_global() {
        // `:global(...)` opts a class out of module-local hashing — the one
        // module escape that IS a real global class.
        let analysis = FileAnalysisSnapshot {
            styles: (vec![module_style(":global(.reset) { margin: 0; }")]).into(),
            ..Default::default()
        };
        assert!(class_declared_global("reset", &analysis));
    }

    #[test]
    fn global_pseudo_inner_class_is_global_even_in_scoped_block() {
        let analysis = FileAnalysisSnapshot {
            styles: (vec![style(":global(.reset) { margin: 0; }", true)]).into(),
            ..Default::default()
        };
        assert!(class_declared_global("reset", &analysis));
    }

    #[test]
    fn scoped_svelte_style_stays_local_but_global_pseudo_opts_out() {
        // Svelte-shaped: scoped by default with a :global escape.
        let source = ".local { color: red; }\n:global(.shared) { color: blue; }";
        let analysis = FileAnalysisSnapshot {
            styles: (vec![verter_semantic::analysis::build_scanned_style_analysis(
                StyleAnalysisLang::Css,
                source,
                VueStyleInput::default(),
                true,
                false,
                None,
                0,
            )])
            .into(),
            ..Default::default()
        };
        assert!(!class_declared_global("local", &analysis));
        assert!(class_declared_global("shared", &analysis));
    }

    #[test]
    fn spans_in_file_gate_declarations_by_scope_but_keep_usages() {
        let src = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
        // Scoped declaration + a template usage: declarations-only yields
        // NOTHING (scoped decl excluded); usage collection still finds the
        // template token.
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(src);
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (scs, sce) = style_block.content_range();
        let analysis = FileAnalysisSnapshot {
            markup_class_tokens: std::sync::Arc::new(vec![
                verter_semantic::analysis::MarkupClassToken {
                    name: "btn".to_string(),
                    span: verter_span::Span::new(23, 26),
                    from_directive: false,
                },
            ]),
            styles: (vec![build_css_style_analysis(
                &src[scs as usize..sce as usize],
                VueStyleInput::default(),
                true,
                false,
                None,
                scs,
            )])
            .into(),
            ..Default::default()
        };
        let decls = global_class_spans_in_file("btn", src, &analysis, true);
        assert!(decls.is_empty(), "scoped declarations never cross files");
        let all = global_class_spans_in_file("btn", src, &analysis, false);
        assert_eq!(all.len(), 1, "the markup usage is still a reference");
    }
}
