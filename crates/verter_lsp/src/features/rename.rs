// Phase 2: Rename — rename bindings across script/template blocks in a single file.
// Phase 3: Enhanced with cross-file rename from TypeProvider.

use std::collections::HashMap;

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Sentinel URI used when a rename edit is in the same file.
/// The server replaces this with the actual document URI before returning to the client.
pub const SAME_FILE_URI: &str = "verter-internal:same-file";

/// Check if the symbol at the given position can be renamed.
///
/// Returns a `Range` of the symbol if renaming is allowed, or `None` if not.
pub fn prepare_rename(
    position: &Position,
    source: &str,
    _blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<Range> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Only allow renaming known bindings and non-type imports
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        return None;
    }

    // Return the range of the word at the cursor
    let word_start = find_word_start(source.as_bytes(), offset);
    let word_end = word_start + word.len();

    let start = line_index.offset_to_position(word_start as u32)?;
    let end = line_index.offset_to_position(word_end as u32)?;
    Some(Range { start, end })
}

/// Perform a rename of the symbol at the given position to `new_name`.
///
/// Finds all occurrences in script and template blocks and returns a
/// `WorkspaceEdit` with text edits for each occurrence.
///
/// ## Analysis data needed for cross-file rename:
/// - `AnalyzedImport.resolved_canonical_id` — to rename the export in the source file
/// - `TemplateAnalysisSnapshot.binding_occurrences` — precise template positions
///   Currently uses text search in template; when template analysis is exposed,
///   replace with span-based edits for accuracy.
pub fn rename_at_position(
    position: &Position,
    new_name: &str,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<WorkspaceEdit> {
    let analysis = analysis?;
    let offset = line_index.position_to_offset(position)? as usize;
    let word = word_at_offset(source, offset)?;

    // Verify renaming is allowed
    let is_binding = analysis.bindings.iter().any(|b| b.name == word);
    let is_import = analysis
        .imports
        .iter()
        .any(|i| !i.is_type_only && i.bindings.iter().any(|b| b.name == word && !b.is_type_only));
    let is_macro = analysis
        .macros
        .iter()
        .any(|m| m.binding_name.as_ref().is_some_and(|n| n == &word));

    if !is_binding && !is_import && !is_macro {
        return None;
    }

    let mut edits: Vec<TextEdit> = Vec::new();

    // Collect declaration spans
    if let Some(binding) = analysis.bindings.iter().find(|b| b.name == word) {
        if binding.span_start > 0 || binding.span_end > 0 {
            if let Some(edit) =
                span_to_edit(binding.span_start, binding.span_end, new_name, line_index)
            {
                edits.push(edit);
            }
        }
    }
    for import in &analysis.imports {
        for binding in &import.bindings {
            if binding.name == word && (binding.span_start > 0 || binding.span_end > 0) {
                if let Some(edit) =
                    span_to_edit(binding.span_start, binding.span_end, new_name, line_index)
                {
                    edits.push(edit);
                }
            }
        }
    }

    // Scan all blocks for text occurrences
    for block in blocks {
        let (content_start, content_end) = block.content_range();
        let content = &source[content_start as usize..content_end as usize];

        for occ_offset in find_all_word_occurrences(content, &word) {
            let abs_offset = content_start as usize + occ_offset;
            let abs_end = abs_offset + word.len();

            // Skip if this overlaps a declaration span edit
            let already_covered = edits.iter().any(|e| {
                let e_start = line_index.position_to_offset(&e.range.start);
                e_start == Some(abs_offset as u32)
            });
            if already_covered {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(abs_offset as u32),
                line_index.offset_to_position(abs_end as u32),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: new_name.to_string(),
                });
            }
        }
    }

    if edits.is_empty() {
        return None;
    }

    let uri: Uri = SAME_FILE_URI.parse().unwrap();
    #[allow(clippy::mutable_key_type)] // Uri has interior mutability but we only insert once
    let mut changes = HashMap::new();
    changes.insert(uri, edits);

    Some(WorkspaceEdit {
        changes: Some(changes),
        ..Default::default()
    })
}

fn span_to_edit(
    span_start: u32,
    span_end: u32,
    new_name: &str,
    line_index: &LineIndex,
) -> Option<TextEdit> {
    let start = line_index.offset_to_position(span_start)?;
    let end = line_index.offset_to_position(span_end)?;
    Some(TextEdit {
        range: Range { start, end },
        new_text: new_name.to_string(),
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

fn find_word_start(bytes: &[u8], offset: usize) -> usize {
    let mut start = offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
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
    fn test_rename_binding_across_blocks() {
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

        let edit = rename_at_position(
            &position,
            "counter",
            source,
            &blocks,
            Some(&analysis),
            &line_index,
        );
        assert!(edit.is_some());

        let edit = edit.unwrap();
        let changes = edit.changes.unwrap();
        let uri: Uri = SAME_FILE_URI.parse().unwrap();
        let edits = changes.get(&uri).unwrap();

        // Declaration + template usage = at least 2 edits
        assert!(edits.len() >= 2, "expected >=2 edits, got {}", edits.len());
        assert!(edits.iter().all(|e| e.new_text == "counter"));
    }

    #[test]
    fn test_prepare_rename_returns_range() {
        let source = "<script setup>\nconst count = ref(0)\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let count_offset = source.find("count").unwrap() as u32;

        let analysis = make_analysis(
            vec![AnalyzedBinding {
                name: "count".to_string(),
                kind: AnalyzedBindingKind::Const,
                is_reactive: true,
                reactivity_kind: ReactivityKind::Ref,
                type_annotation: None,
                initializer: None,
                span_start: count_offset,
                span_end: count_offset + 5,
            }],
            vec![],
        );

        let position = line_index.offset_to_position(count_offset).unwrap();

        let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(range.is_some());
        let range = range.unwrap();
        assert_eq!(range.start, position);
    }

    #[test]
    fn test_cannot_rename_unknown_word() {
        let source = "<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(vec![], vec![]);

        let offset = source.find("const").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(range.is_none());
    }

    #[test]
    fn test_cannot_rename_type_only_import() {
        let source = "<script setup>\nimport type { Props } from './types'\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);

        let analysis = make_analysis(
            vec![],
            vec![AnalyzedImport {
                source: "./types".to_string(),
                is_type_only: true,
                bindings: vec![AnalyzedImportBinding {
                    name: "Props".to_string(),
                    is_type_only: true,
                    vue_api: None,
                    span_start: 0,
                    span_end: 0,
                }],
                span_start: 0,
                span_end: 0,
                resolved_canonical_id: None,
            }],
        );

        let offset = source.find("Props").unwrap();
        let position = line_index.offset_to_position(offset as u32).unwrap();

        let range = prepare_rename(&position, source, &blocks, Some(&analysis), &line_index);
        assert!(range.is_none());
    }
}
