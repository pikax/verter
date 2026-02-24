// Phase 2: References — find all occurrences of a binding across script/template blocks.
// Phase 3: Enhanced with cross-file references from TypeProvider.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Sentinel URI used when a reference is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Find all references to the symbol at the given position.
///
/// Strategy:
/// 1. Find the word at the cursor position
/// 2. Collect all occurrences:
///    - The binding declaration span (if include_declaration)
///    - Template binding occurrences from `TemplateAnalysisSnapshot` (precise spans)
///    - Text occurrences in script blocks (word boundary match)
///    - Falls back to text search in template blocks if template analysis is unavailable
pub fn references_at_position(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    include_declaration: bool,
) -> Option<Vec<Location>> {
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

    let mut locations = Vec::new();

    // Add the declaration span if requested
    if include_declaration {
        if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
            if binding.span_start > 0 || binding.span_end > 0 {
                if let Some(loc) =
                    span_to_location(binding.span_start, binding.span_end, line_index)
                {
                    locations.push(loc);
                }
            }
        }
        for import in &analysis.imports {
            for binding in &import.bindings {
                if binding.name == word && (binding.span_start > 0 || binding.span_end > 0) {
                    if let Some(loc) =
                        span_to_location(binding.span_start, binding.span_end, line_index)
                    {
                        locations.push(loc);
                    }
                }
            }
        }
        for mac in &analysis.macros {
            if mac.binding_name.as_ref().is_some_and(|n| n == &word)
                && (mac.span_start > 0 || mac.span_end > 0)
            {
                if let Some(loc) = span_to_location(mac.span_start, mac.span_end, line_index) {
                    locations.push(loc);
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
                // Skip if this overlaps a declaration we already added
                let already_present = locations.iter().any(|loc| {
                    let loc_start = line_index.position_to_offset(&loc.range.start);
                    loc_start == Some(occ.span_start)
                });
                if already_present {
                    continue;
                }
                if let Some(loc) = span_to_location(occ.span_start, occ.span_end, line_index) {
                    locations.push(loc);
                }
            }
        }
    }

    // Scan script blocks for text occurrences (template is covered by analysis above)
    for block in blocks {
        // Skip template blocks if we have template analysis
        if has_template_analysis && block.tag_name == "template" {
            continue;
        }

        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;

            // Skip if this overlaps a declaration we already added
            let already_present = locations.iter().any(|loc| {
                let loc_start = line_index.position_to_offset(&loc.range.start);
                loc_start == Some(abs_offset as u32)
            });
            if already_present {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position((abs_offset + word.len()) as u32),
            ) {
                locations.push(Location {
                    uri: SAME_FILE_URI.parse().unwrap(),
                    range: Range { start, end },
                });
            }
        }
    }

    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

fn span_to_location(span_start: u32, span_end: u32, line_index: &LineIndex) -> Option<Location> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(Location {
        uri: SAME_FILE_URI.parse().unwrap(),
        range: Range { start, end },
    })
}

/// Find all byte offsets where `word` appears at a word boundary in `content`.
fn find_all_word_occurrences(content: &str, word: &str) -> Vec<usize> {
    let mut results = Vec::new();
    let bytes = content.as_bytes();
    let word_bytes = word.as_bytes();
    let word_len = word_bytes.len();

    let mut start = 0;
    while let Some(offset) = content[start..].find(word) {
        let abs = start + offset;
        let after = abs + word_len;

        // Check word boundaries
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
        macros: Vec<AnalyzedMacro>,
    ) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            bindings,
            imports,
            macros,
            macro_type_deps: vec![],
            script_flags: 0,
            styles: vec![],
            template: None,
        }
    }

    #[test]
    fn test_references_for_binding_across_blocks() {
        let source = "<template>\n  {{ count }}\n</template>\n\n<script setup>\nconst count = ref(0)\nconsole.log(count)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let count_decl = source.rfind("count = ref").unwrap() as u32;

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
            vec![],
        );

        // Click on "count" in template
        let template_count = source.find("count").unwrap();
        let position = line_index
            .offset_to_position(template_count as u32)
            .unwrap();

        let refs = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        assert!(refs.is_some());
        let refs = refs.unwrap();
        // Declaration + template occurrence + two script occurrences ("count = ref" and "log(count)")
        assert!(refs.len() >= 3, "expected >=3 refs, got {}", refs.len());
    }

    #[test]
    fn test_references_exclude_declaration() {
        let source =
            "<template>\n  {{ x }}\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let x_offset = source.rfind("x = 1").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "x".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: false,
                reactivity_kind: ReactivityKind::None,
                type_annotation: None,
                initializer: None,
                span_start: x_offset,
                span_end: x_offset + 1,
            }],
            vec![],
            vec![],
        );

        let template_x = source.find(" x ").unwrap() + 1;
        let position = line_index.offset_to_position(template_x as u32).unwrap();

        let refs_with_decl = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        let refs_without_decl = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            false,
        );

        assert!(refs_with_decl.is_some());
        assert!(refs_without_decl.is_some());
        // With declaration should have more entries
        assert!(refs_with_decl.unwrap().len() >= refs_without_decl.unwrap().len());
    }

    #[test]
    fn test_no_references_for_unknown_word() {
        let source = "<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(vec![], vec![], vec![]);

        // Click on "const" — not a binding
        let offset = source.find("const").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let refs = references_at_position(
            &position,
            source,
            &blocks,
            Some(&analysis),
            &line_index,
            true,
        );
        assert!(refs.is_none());
    }

    #[test]
    fn test_find_all_word_occurrences() {
        let content = "count = count + counter";
        let results = find_all_word_occurrences(content, "count");
        assert_eq!(results, vec![0, 8]); // "count" but not "counter"
    }
}
