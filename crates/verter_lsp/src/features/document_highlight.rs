// Phase 2: Document highlights — highlight all occurrences of a binding in the current file.
// Phase 3: Enhanced with type-aware highlights from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

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
        return None;
    }

    let mut highlights = Vec::new();

    // Add declaration as Write highlight
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span_start > 0 || binding.span_end > 0 {
            if let Some(hl) = span_to_highlight(
                binding.span_start,
                binding.span_end,
                line_index,
                DocumentHighlightKind::WRITE,
            ) {
                highlights.push(hl);
            }
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span_start > 0 || binding.span_end > 0) {
                if let Some(hl) = span_to_highlight(
                    binding.span_start,
                    binding.span_end,
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
                    hl_start == Some(occ.span_start)
                });
                if already_present {
                    continue;
                }
                if let Some(hl) = span_to_highlight(
                    occ.span_start,
                    occ.span_end,
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

fn find_all_word_occurrences(content: &str, word: &str) -> Vec<usize> {
    let mut results = Vec::new();
    let bytes = content.as_bytes();
    let word_len = word.len();

    let mut start = 0;
    while let Some(offset) = content[start..].find(word) {
        let abs = start + offset;
        let after = abs + word_len;

        let before_ok = abs == 0 || !is_ident_byte(bytes[abs - 1]);
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);

        if before_ok && after_ok {
            results.push(abs);
        }

        start = abs + 1;
    }

    results
}

fn word_at_offset(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset >= bytes.len() || !is_ident_byte(bytes[offset]) {
        return None;
    }

    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    let mut end = offset;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    Some(source[start..end].to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

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
            macros: vec![],
            macro_type_deps: vec![],
            script_flags: 0,
            styles: vec![],
            template: None,
        }
    }

    #[test]
    fn test_highlight_binding_declaration_and_usages() {
        let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let count_decl = source.rfind("count").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span_start: count_decl,
                span_end: count_decl + 5,
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
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(vec![], vec![]);

        let offset = source.find("const").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let highlights =
            highlights_at_position(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(highlights.is_none());
    }

    #[test]
    fn test_highlight_import_binding() {
        let source = "<script setup>\nimport { ref } from 'vue'\nconst x = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

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
                    span_start: ref_import_offset,
                    span_end: ref_import_offset + 3,
                }],
                span_start: 0,
                span_end: 0,
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
}
