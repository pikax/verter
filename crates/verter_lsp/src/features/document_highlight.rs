// Phase 2: Document highlights — highlight all occurrences of a binding in the current file.
// Phase 3: Enhanced with type-aware highlights from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::references::{
    collect_css_ref_spans, find_css_target_in_style_refs, find_css_target_in_template_refs,
};

/// Find all highlights for the symbol at the given position.
///
/// Returns document highlights with read/write distinction:
/// - The declaration site is marked as `Write`
/// - Template binding occurrences from `TemplateAnalysisSnapshot` (precise spans, `Read`)
/// - Script text occurrences (word boundary match, `Read`)
/// - Falls back to text search in template blocks if template analysis is unavailable
pub fn highlights_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Check if this word is a known binding, import, or macro
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| i.bindings.iter().any(|b| b.name == word));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        // Try CSS class/ID highlights
        return css_highlights(offset, source, blocks, analysis, line_index);
    }

    let mut highlights = Vec::new();

    // Add declaration as Write highlight
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span.start > 0 || binding.span.end > 0 {
            if let Some(hl) = span_to_highlight(
                binding.span.start,
                binding.span.end,
                line_index,
                DocumentHighlightKind::WRITE,
            ) {
                highlights.push(hl);
            }
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span.start > 0 || binding.span.end > 0) {
                if let Some(hl) = span_to_highlight(
                    binding.span.start,
                    binding.span.end,
                    line_index,
                    DocumentHighlightKind::WRITE,
                ) {
                    highlights.push(hl);
                }
            }
        }
    }

    // Use template analysis binding occurrences when available (precise spans)
    let has_template_analysis = analysis
        .template
        .as_ref()
        .is_some_and(|t| !t.binding_occurrences.is_empty());

    if has_template_analysis {
        let template = analysis.template.as_ref().unwrap();
        for occ in &template.binding_occurrences {
            if occ.name == word {
                let already_present = highlights.iter().any(|hl| {
                    let hl_start = line_index.position_to_offset(&hl.range.start);
                    hl_start == Some(occ.span.start)
                });
                if already_present {
                    continue;
                }
                if let Some(hl) = span_to_highlight(
                    occ.span.start,
                    occ.span.end,
                    line_index,
                    DocumentHighlightKind::READ,
                ) {
                    highlights.push(hl);
                }
            }
        }
    }

    // Scan script blocks (and template blocks if no template analysis) for Read occurrences
    for block in blocks {
        if has_template_analysis && block.tag_name == "template" {
            continue;
        }

        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;

            let already_present = highlights.iter().any(|hl| {
                let hl_start = line_index.position_to_offset(&hl.range.start);
                hl_start == Some(abs_offset as u32)
            });
            if already_present {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position((abs_offset + word.len()) as u32),
            ) {
                highlights.push(DocumentHighlight {
                    range: Range { start, end },
                    kind: Some(DocumentHighlightKind::READ),
                });
            }
        }
    }

    if highlights.is_empty() {
        None
    } else {
        Some(highlights)
    }
}

/// Find CSS class/ID highlights across template and style blocks.
fn css_highlights(
    offset: usize,
    source: &str,
    blocks: &[SfcBlock],
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<Vec<DocumentHighlight>> {
    // Determine if we're in template or style
    let in_template = blocks.iter().any(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });
    let in_style = blocks.iter().any(|b| {
        b.tag_name == "style" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset < ce as usize
        }
    });

    if !in_template && !in_style {
        return None;
    }

    let target = if in_template {
        analysis
            .template
            .as_ref()
            .and_then(|t| find_css_target_in_template_refs(offset, source, t))
    } else {
        find_css_target_in_style_refs(offset, source, analysis)
    }?;

    let spans = collect_css_ref_spans(&target, source, analysis);
    let highlights: Vec<DocumentHighlight> = spans
        .into_iter()
        .filter_map(|(start, end)| {
            span_to_highlight(start, end, line_index, DocumentHighlightKind::READ)
        })
        .collect();

    if highlights.is_empty() {
        None
    } else {
        Some(highlights)
    }
}

fn span_to_highlight(
    span_start: u32,
    span_end: u32,
    line_index: &LineIndex,
    kind: DocumentHighlightKind,
) -> Option<DocumentHighlight> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(DocumentHighlight {
        range: Range { start, end },
        kind: Some(kind),
    })
}

use crate::utils::{find_all_word_occurrences, word_at_offset};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::*;

    fn make_analysis(
        bindings: Vec<AnalyzedBinding>,
        imports: Vec<AnalyzedImport>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            bindings,
            imports,
            ..Default::default()
        }
    }

    #[test]
    fn test_highlight_binding_declaration_and_usages() {
        let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let count_decl = source.rfind("count").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span: verter_span::Span::new(count_decl, count_decl + 5),
            }],
            vec![],
        );

        let template_count = source.find("count").unwrap();
        let position = line_index
            .offset_to_position(template_count as u32)
            .unwrap();

        let highlights =
            highlights_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_some());

        let highlights = highlights.unwrap();
        // At least 2: declaration (Write) + template usage (Read)
        assert!(highlights.len() >= 2);

        // Check that we have both Write and Read kinds
        assert!(highlights
            .iter()
            .any(|h| h.kind == Some(DocumentHighlightKind::WRITE)));
        assert!(highlights
            .iter()
            .any(|h| h.kind == Some(DocumentHighlightKind::READ)));
    }

    #[test]
    fn test_no_highlight_for_unknown_word() {
        let source = "<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_analysis(vec![], vec![]);

        let offset = source.find("const").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let highlights =
            highlights_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_none());
    }

    // =========================================================================
    // CSS Class/ID Highlight Tests (A4)
    // =========================================================================

    /// @ai-generated - Clicking class in template highlights in template + style
    #[test]
    fn test_highlight_css_class_from_template() {
        let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &["btn"], None);
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(verter_analysis::TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let btn_offset = source.find("btn\"").unwrap();
        let pos = line_index.offset_to_position(btn_offset as u32).unwrap();
        let highlights =
            highlights_at_position(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_some());
        let highlights = highlights.unwrap();
        assert!(
            highlights.len() >= 2,
            "should highlight in template and style"
        );
    }

    /// @ai-generated - Clicking class in style highlights in style + template
    #[test]
    fn test_highlight_css_class_from_style() {
        let source = "<template><div class=\"btn\"></div></template>\n<style scoped>\n.btn { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &["btn"], None);
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(verter_analysis::TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let btn_style_offset = source.rfind(".btn").unwrap() + 1; // skip the '.'
        let pos = line_index
            .offset_to_position(btn_style_offset as u32)
            .unwrap();
        let highlights =
            highlights_at_position(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_some());
        let highlights = highlights.unwrap();
        assert!(
            highlights.len() >= 2,
            "should highlight in style and template"
        );
    }

    fn make_element_with_attrs(
        source: &str,
        tag: &str,
        classes: &[&str],
        _id: Option<&str>,
    ) -> verter_analysis::TemplateElement {
        let mut attrs = Vec::new();
        if !classes.is_empty() {
            let class_val = classes.join(" ");
            let pattern = format!("class=\"{}\"", class_val);
            let start = source.find(&pattern).unwrap_or(0) as u32;
            let end = start + pattern.len() as u32;
            attrs.push(verter_analysis::TemplateAttribute {
                name: "class".into(),
                value: Some(class_val),
                is_dynamic: false,
                span: verter_span::Span::new(start, end),
            });
        }
        verter_analysis::TemplateElement {
            tag: tag.into(),
            is_component: false,
            is_self_closing: false,
            namespace: verter_analysis::ElementNamespace::Html,
            attributes: attrs,
            directives: vec![],
            v_for: None,
            v_model: None,
            has_v_if: false,
            has_v_else: false,
            has_v_else_if: false,
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_element_children: false,
            nesting_depth: 0,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: verter_span::Span::new(0, 0),
            tag_span_end: 0,
        }
    }

    fn build_style(source: &str, blocks: &[SfcBlock]) -> verter_analysis::StyleBlockAnalysis {
        let style_block = blocks.iter().find(|b| b.tag_name == "style").unwrap();
        let (content_start, content_end) = style_block.content_range();
        let css_content = &source[content_start as usize..content_end as usize];
        let scoped = style_block.attrs_raw.contains("scoped");
        verter_analysis::style::build_css_style_analysis(
            css_content,
            verter_analysis::style::VueStyleInput {
                v_binds: vec![],
                special_pseudos: vec![],
            },
            scoped,
            false,
            None,
            content_start,
        )
    }

    #[test]
    fn test_highlight_import_binding() {
        let source = "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let ref_import_offset = source.find("ref").unwrap() as u32;

        let analysis = make_analysis(
            vec![],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: Some(VueApiClassification::Ref),
                    span: verter_span::Span::new(ref_import_offset, ref_import_offset + 3),
                }],
                span: verter_span::Span::new(0, 0),
                resolved_canonical_id: None,
            }],
        );

        let position = line_index.offset_to_position(ref_import_offset).unwrap();

        let highlights =
            highlights_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_some());
        let highlights = highlights.unwrap();
        // Import declaration (Write) + usage in ref(0) (Read)
        assert!(highlights.len() >= 2);
    }

    /// @ai-generated - CSS ID highlights across template and style
    #[test]
    fn test_highlight_css_id_from_template() {
        let source = "<template><div id=\"app\"></div></template>\n<style scoped>\n#app { margin: 0; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let mut el = make_element_with_attrs(source, "div", &[], None);
        let id_pattern = "id=\"app\"";
        let id_start = source.find(id_pattern).unwrap_or(0) as u32;
        let id_end = id_start + id_pattern.len() as u32;
        el.attributes.push(verter_analysis::TemplateAttribute {
            name: "id".into(),
            value: Some("app".into()),
            is_dynamic: false,
            span: verter_span::Span::new(id_start, id_end),
        });

        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(verter_analysis::TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let id_offset = source.find("app\"").unwrap();
        let pos = line_index.offset_to_position(id_offset as u32).unwrap();
        let highlights =
            highlights_at_position(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_some(), "should highlight CSS ID");
        let highlights = highlights.unwrap();
        assert!(
            highlights.len() >= 2,
            "should highlight in template and style, got {}",
            highlights.len()
        );
    }

    /// @ai-generated - No CSS highlights for class not in any style
    #[test]
    fn test_no_css_highlight_without_style_match() {
        let source = "<template><div class=\"missing\"></div></template>\n<style scoped>\n.other { color: red; }\n</style>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let css = build_style(source, &blocks);

        let el = make_element_with_attrs(source, "div", &["missing"], None);
        let analysis = FileAnalysisSnapshot {
            styles: vec![css],
            template: Some(verter_analysis::TemplateAnalysisSnapshot {
                elements: vec![el],
                ..Default::default()
            }),
            ..Default::default()
        };

        let class_offset = source.find("missing\"").unwrap();
        let pos = line_index.offset_to_position(class_offset as u32).unwrap();
        let highlights =
            highlights_at_position(&pos, source, &blocks, Some(&analysis), &line_index);
        // May find template-only highlights, but should not have style highlights
        if let Some(highlights) = highlights {
            // At most 1 (the template occurrence itself), not 2+
            // The key negative: should not crash or find spurious matches
            assert!(
                highlights.len() <= 1,
                "should not find style highlights for missing class, got {}",
                highlights.len()
            );
        }
    }
}
