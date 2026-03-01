// Shared utilities for code action generation.
//
// Extracted from macro_actions.rs and organize_imports.rs to eliminate duplication.
// These helpers are used by all code action modules: macro_actions, organize_imports,
// component_actions, event_type_hints, etc.

use tower_lsp_server::lsp_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::sfc_scanner::SfcBlock;

/// Find the byte offset to insert a new statement in `<script setup>`.
///
/// Preference order:
/// 1. After the last import statement (past trailing `;`, whitespace, newline)
/// 2. Right after the `<script setup>` tag opening
pub fn find_script_insert_offset(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &SfcBlock,
) -> u32 {
    if let Some(last_import) = analysis.imports.last() {
        let end = last_import.span.end as usize;
        let skip = skip_trailing_whitespace(source.as_bytes(), end);
        return (end + skip) as u32;
    }

    // Fallback: right after the opening <script setup> tag
    setup_block.open_tag_end
}

/// Skip trailing semicolons, whitespace, and a single newline after a byte offset.
///
/// Returns the number of bytes to skip.
pub fn skip_trailing_whitespace(source: &[u8], offset: usize) -> usize {
    let rest = &source[offset..];
    let mut skip = 0;
    // Skip optional semicolon
    if skip < rest.len() && rest[skip] == b';' {
        skip += 1;
    }
    // Skip horizontal whitespace
    while skip < rest.len() && (rest[skip] == b' ' || rest[skip] == b'\t') {
        skip += 1;
    }
    // Skip one newline (\r\n or \n)
    if skip < rest.len() && rest[skip] == b'\r' {
        skip += 1;
    }
    if skip < rest.len() && rest[skip] == b'\n' {
        skip += 1;
    }
    skip
}

/// Whether a TypeScript identifier needs quoting in a type literal.
///
/// Returns `true` for names containing hyphens, spaces, or starting with a digit.
pub fn needs_quoting(name: &str) -> bool {
    name.contains('-')
        || name.contains(' ')
        || name.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Build a `WorkspaceEdit` that inserts text at a position in a document.
pub fn make_insert_edit(uri: &Uri, position: Position, text: String) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: None,
            },
            edits: vec![OneOf::Left(TextEdit {
                range: Range {
                    start: position,
                    end: position,
                },
                new_text: text,
            })],
        }])),
        change_annotations: None,
    }
}

/// Build a `WorkspaceEdit` that replaces text in a range.
pub fn make_replace_edit(uri: &Uri, range: Range, text: String) -> WorkspaceEdit {
    WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: None,
            },
            edits: vec![OneOf::Left(TextEdit {
                range,
                new_text: text,
            })],
        }])),
        change_annotations: None,
    }
}

/// Build a `CodeActionOrCommand` from a `WorkspaceEdit`.
pub fn make_code_action(
    title: String,
    kind: CodeActionKind,
    edit: WorkspaceEdit,
    is_preferred: bool,
    diagnostics: Option<Vec<Diagnostic>>,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title,
        kind: Some(kind),
        diagnostics,
        edit: Some(edit),
        is_preferred: Some(is_preferred),
        ..Default::default()
    })
}

/// Format an action title with singular/plural handling.
///
/// - Single item: `"Add prop 'foo'"`
/// - Multiple items: `"Add 2 props"`
pub fn format_action_title(singular: &str, plural: &str, items: &[&str]) -> String {
    if items.len() == 1 {
        format!("{} '{}'", singular, items[0])
    } else {
        format!(
            "{} {} {}",
            singular.split(' ').next().unwrap_or("Add"),
            items.len(),
            plural.split(' ').skip(1).collect::<Vec<_>>().join(" ")
        )
    }
}

/// Fix placeholder URIs in code actions generated with `SAME_FILE_URI`.
///
/// Replaces all `file:///placeholder` URIs in document edits with the actual URI.
pub fn fix_placeholder_uris(actions: &mut [CodeActionOrCommand], uri: &Uri) {
    for action in actions.iter_mut() {
        if let CodeActionOrCommand::CodeAction(ref mut ca) = action {
            if let Some(ref mut edit) = ca.edit {
                if let Some(DocumentChanges::Edits(ref mut doc_edits)) = edit.document_changes {
                    for doc_edit in doc_edits.iter_mut() {
                        doc_edit.text_document.uri = uri.clone();
                    }
                }
            }
        }
    }
}

/// Build a code action that inserts text at a position, using a placeholder URI.
///
/// The caller must call [`fix_placeholder_uris`] to replace the sentinel URI
/// with the actual document URI before returning to the client.
pub fn make_insert_action(
    title: &str,
    kind: CodeActionKind,
    text: &str,
    position: Position,
) -> CodeActionOrCommand {
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_string(),
        kind: Some(kind),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: SAME_FILE_URI.parse().unwrap(),
                    version: None,
                },
                edits: vec![OneOf::Left(TextEdit {
                    range: Range {
                        start: position,
                        end: position,
                    },
                    new_text: text.to_string(),
                })],
            }])),
            change_annotations: None,
        }),
        is_preferred: Some(false),
        ..Default::default()
    })
}

/// Placeholder URI used by code actions that edit the same file.
/// Must be replaced with the actual document URI via [`fix_placeholder_uris`].
pub const SAME_FILE_URI: &str = "file:///placeholder";

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_script_insert_offset ───────────────────────────────────────

    #[test]
    fn insert_offset_after_last_import() {
        let source = "<script setup>\nimport { ref } from 'vue'\nimport { computed } from 'vue'\nconst x = 1\n</script>";
        let analysis = FileAnalysisSnapshot {
            imports: vec![
                verter_analysis::AnalyzedImport {
                    source: "vue".into(),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(15, 40),
                    resolved_canonical_id: None,
                },
                verter_analysis::AnalyzedImport {
                    source: "vue".into(),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(41, 71),
                    resolved_canonical_id: None,
                },
            ],
            ..Default::default()
        };
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);
        let setup_block = blocks.iter().find(|b| b.is_setup()).unwrap();

        let offset = find_script_insert_offset(source, &analysis, setup_block) as usize;

        // Positive: offset is past both imports
        assert!(
            offset > 71,
            "offset ({offset}) should be past second import (71)"
        );
        // Negative: offset is NOT inside any import
        assert!(
            offset >= 72,
            "offset should be past the newline after the import"
        );
    }

    #[test]
    fn insert_offset_falls_back_to_script_tag() {
        let source = "<script setup>\nconst x = 1\n</script>";
        let analysis = FileAnalysisSnapshot::default();
        let blocks = crate::documents::sfc_scanner::scan_sfc_blocks(source);
        let setup_block = blocks.iter().find(|b| b.is_setup()).unwrap();

        let offset = find_script_insert_offset(source, &analysis, setup_block);

        // Should be right after <script setup> tag
        assert_eq!(offset, setup_block.open_tag_end);
        // Negative: offset is NOT 0
        assert!(offset > 0, "offset should not be 0");
    }

    // ── skip_trailing_whitespace ────────────────────────────────────────

    #[test]
    fn skip_whitespace_skips_semicolon_and_newline() {
        //                                       ^offset=17 (the ')
        let source = b"import x from 'y';\r\nconst a = 1";
        // After the closing quote, the ';' is at 17, so pass 17 to skip from there.
        // But the function is designed to be called with the span_end of the import,
        // which is *after* the quote. The ';' at index 17 follows the quote at 16.
        // 'i' 'm' 'p' 'o' 'r' 't' ' ' 'x' ' ' 'f' 'r' 'o' 'm' ' ' '\'' 'y' '\'' ';' '\r' '\n'
        //  0   1   2   3   4   5   6   7   8   9  10  11  12  13   14  15   16   17   18   19
        let offset = 17; // at the ';'
        let skip = skip_trailing_whitespace(source, offset);
        // Should skip ';', '\r', '\n' = 3 bytes
        assert_eq!(skip, 3);
        // Negative: does NOT skip past 'const'
        assert_eq!(source[offset + skip], b'c');
    }

    #[test]
    fn skip_whitespace_handles_just_newline() {
        let source = b"import x from 'y'\nconst a = 1";
        let offset = 17;
        let skip = skip_trailing_whitespace(source, offset);
        assert_eq!(skip, 1); // just \n
    }

    #[test]
    fn skip_whitespace_no_newline() {
        let source = b"import x from 'y'  end";
        let offset = 17;
        let skip = skip_trailing_whitespace(source, offset);
        // Skips the two spaces but not past non-whitespace
        assert_eq!(skip, 2);
    }

    // ── needs_quoting ───────────────────────────────────────────────────

    #[test]
    fn plain_identifier_no_quoting() {
        assert!(!needs_quoting("foo"));
        assert!(!needs_quoting("myProp"));
        assert!(!needs_quoting("_private"));
    }

    #[test]
    fn hyphenated_name_needs_quoting() {
        assert!(needs_quoting("nav-bar"));
        assert!(needs_quoting("my-component"));
    }

    #[test]
    fn digit_start_needs_quoting() {
        assert!(needs_quoting("0abc"));
        assert!(needs_quoting("123"));
    }

    #[test]
    fn space_in_name_needs_quoting() {
        assert!(needs_quoting("my prop"));
    }

    // ── make_insert_edit ────────────────────────────────────────────────

    #[test]
    fn insert_edit_has_correct_range_and_text() {
        let uri: Uri = "file:///test.vue".parse().unwrap();
        let pos = Position {
            line: 3,
            character: 0,
        };
        let edit = make_insert_edit(&uri, pos, "new text".into());

        if let Some(DocumentChanges::Edits(doc_edits)) = &edit.document_changes {
            assert_eq!(doc_edits.len(), 1);
            assert_eq!(doc_edits[0].text_document.uri, uri);
            if let OneOf::Left(te) = &doc_edits[0].edits[0] {
                // Positive: range start == end (insertion)
                assert_eq!(
                    te.range.start, te.range.end,
                    "insertion should have start == end"
                );
                assert_eq!(te.new_text, "new text");
                // Negative: range is NOT a replacement
                assert_eq!(te.range.start.line, 3);
            } else {
                panic!("expected TextEdit");
            }
        } else {
            panic!("expected DocumentChanges::Edits");
        }
    }

    // ── make_replace_edit ───────────────────────────────────────────────

    #[test]
    fn replace_edit_has_correct_range_and_text() {
        let uri: Uri = "file:///test.vue".parse().unwrap();
        let range = Range {
            start: Position {
                line: 1,
                character: 0,
            },
            end: Position {
                line: 1,
                character: 5,
            },
        };
        let edit = make_replace_edit(&uri, range, "replaced".into());

        if let Some(DocumentChanges::Edits(doc_edits)) = &edit.document_changes {
            if let OneOf::Left(te) = &doc_edits[0].edits[0] {
                assert_ne!(
                    te.range.start, te.range.end,
                    "replacement should have start != end"
                );
                assert_eq!(te.new_text, "replaced");
            } else {
                panic!("expected TextEdit");
            }
        } else {
            panic!("expected DocumentChanges::Edits");
        }
    }

    // ── make_code_action ────────────────────────────────────────────────

    #[test]
    fn code_action_has_correct_kind_and_title() {
        let uri: Uri = "file:///test.vue".parse().unwrap();
        let edit = make_insert_edit(&uri, Position::default(), "text".into());
        let action = make_code_action(
            "Test Action".into(),
            CodeActionKind::QUICKFIX,
            edit,
            true,
            None,
        );

        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert_eq!(ca.title, "Test Action");
            assert_eq!(ca.kind, Some(CodeActionKind::QUICKFIX));
            assert_eq!(ca.is_preferred, Some(true));
            // Negative: no diagnostics when None passed
            assert!(ca.diagnostics.is_none());
        } else {
            panic!("expected CodeAction");
        }
    }

    #[test]
    fn code_action_with_diagnostics() {
        let uri: Uri = "file:///test.vue".parse().unwrap();
        let edit = make_insert_edit(&uri, Position::default(), "text".into());
        let diag = Diagnostic {
            range: Range::default(),
            message: "test diagnostic".into(),
            ..Default::default()
        };
        let action = make_code_action(
            "Fix".into(),
            CodeActionKind::QUICKFIX,
            edit,
            false,
            Some(vec![diag]),
        );

        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert!(ca.diagnostics.is_some());
            assert_eq!(ca.diagnostics.as_ref().unwrap().len(), 1);
            assert_eq!(ca.is_preferred, Some(false));
        } else {
            panic!("expected CodeAction");
        }
    }

    // ── format_action_title ─────────────────────────────────────────────

    #[test]
    fn single_item_uses_singular_with_name() {
        let title = format_action_title("Add prop", "Add props", &["foo"]);
        assert_eq!(title, "Add prop 'foo'");
    }

    #[test]
    fn multiple_items_uses_plural_with_count() {
        let title = format_action_title("Add prop", "Add props", &["foo", "bar"]);
        assert!(title.contains("2"), "should contain count");
        assert!(title.contains("props"), "should contain plural form");
        // Negative: should NOT list individual names
        assert!(!title.contains("foo"), "should not list individual names");
    }

    // ── fix_placeholder_uris ────────────────────────────────────────────

    #[test]
    fn fix_placeholder_uris_replaces_sentinel() {
        let real_uri: Uri = "file:///project/src/App.vue".parse().unwrap();
        let mut actions = vec![make_insert_action(
            "Test",
            CodeActionKind::QUICKFIX,
            "text",
            Position::default(),
        )];

        // Before fix: has placeholder
        if let CodeActionOrCommand::CodeAction(ref ca) = actions[0] {
            if let Some(DocumentChanges::Edits(ref edits)) =
                ca.edit.as_ref().unwrap().document_changes
            {
                assert_eq!(edits[0].text_document.uri.as_str(), SAME_FILE_URI);
            }
        }

        fix_placeholder_uris(&mut actions, &real_uri);

        // After fix: has real URI
        if let CodeActionOrCommand::CodeAction(ref ca) = actions[0] {
            if let Some(DocumentChanges::Edits(ref edits)) =
                ca.edit.as_ref().unwrap().document_changes
            {
                assert_eq!(edits[0].text_document.uri, real_uri);
                // Negative: no longer has placeholder
                assert_ne!(edits[0].text_document.uri.as_str(), SAME_FILE_URI);
            }
        }
    }
}
