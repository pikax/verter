// Shared utilities for code action generation.
//
// Extracted from macro_actions.rs and organize_imports.rs to eliminate duplication.
// These helpers are used by all code action modules: macro_actions, organize_imports,
// component_actions, event_type_hints, etc.

use tower_lsp_server::ls_types::*;
use verter_session_query::analysis::file_analysis::AnalysisSourceRevision;
use verter_session_query::analysis::file_analysis::FileAnalysisSnapshot;
use verter_session_query::analysis::types::MemberListAnchor;

use crate::documents::carrier_structure::CarrierBlockView;
use crate::documents::line_index::LineIndex;

/// The live document buffer a macro edit will be applied to.
///
/// Its ONLY capability is turning an analyzer-minted [`MemberListAnchor`] into a
/// position in these exact bytes. It hands out no `&str`, so a function that
/// holds one cannot read macro source text: re-deriving membership or placement
/// by scanning braces is not merely forbidden, it is unexpressible.
pub struct LiveEditTarget<'a> {
    source: &'a str,
    line_index: &'a LineIndex,
}

impl<'a> LiveEditTarget<'a> {
    /// Bind the live buffer and the line index built from those same bytes.
    pub fn new(source: &'a str, line_index: &'a LineIndex) -> Self {
        Self { source, line_index }
    }

    /// Content identity of these exact bytes.
    pub fn revision(&self) -> AnalysisSourceRevision {
        AnalysisSourceRevision::of_source(self.source)
    }

    /// Convert an anchor into an LSP position in the live buffer, or `None`
    /// when the anchor cannot be proven to address it.
    ///
    /// Fail-closed on BOTH failure modes: an offset past the live source's end,
    /// and an offset that is not on a UTF-8 character boundary. Either means the
    /// anchor was minted against different bytes, and an edit there would
    /// corrupt the document — so there is no fallback offset, only `None`.
    ///
    /// One predicate covers both: `str::is_char_boundary` is `false` for any
    /// index greater than the string's length, so a separate bounds comparison
    /// would be redundant rather than additional.
    pub fn anchor_position(&self, anchor: &MemberListAnchor) -> Option<Position> {
        let offset = anchor.insert_offset();
        if !self.source.is_char_boundary(offset as usize) {
            return None;
        }
        self.line_index.offset_to_position(offset)
    }
}

/// Find the byte offset to insert a new statement in `<script setup>`.
///
/// Preference order:
/// 1. After the last import statement (past trailing `;`, whitespace, newline)
/// 2. Right after the `<script setup>` tag opening
pub fn find_script_insert_offset(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &CarrierBlockView,
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
                    uri: PLACEHOLDER_URI.clone(),
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

/// How the client applies a `WorkspaceEdit`, negotiated once at `initialize`
/// from `workspace.workspaceEdit.documentChanges`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceEditSupport {
    /// The client accepts `documentChanges`: every text edit is delivered as a
    /// `TextDocumentEdit` naming the version it was computed against, so the
    /// client rejects an edit to a document that has since moved.
    VersionedDocumentChanges,
    /// The client did not advertise `documentChanges`, so the only shape it can
    /// apply is `WorkspaceEdit.changes`. Every target is validated before
    /// delivery, text edits are downgraded to `changes`, and an edit that
    /// needs a resource operation (file creation) is withheld whole.
    #[default]
    Unversioned,
}

impl WorkspaceEditSupport {
    /// Read the client's advertised `workspace.workspaceEdit.documentChanges`.
    pub fn negotiate(capabilities: &ClientCapabilities) -> Self {
        let document_changes = capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_edit.as_ref())
            .and_then(|edit| edit.document_changes)
            .unwrap_or(false);
        if document_changes {
            Self::VersionedDocumentChanges
        } else {
            Self::Unversioned
        }
    }
}

/// What a request's snapshot knows about one document an edit targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTargetRevision {
    /// Open in the client; the edit was computed against this captured
    /// version.
    Open(i32),
    /// Not open in the client: the edit applies to the file on disk.
    Closed,
    /// Open in the client, but the request captured no revision of it, so
    /// nothing proves which bytes the edit addresses.
    Uncaptured,
}

/// Why an edit cannot be delivered to the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditRefusal {
    /// The request snapshot cannot bind this target to a revision.
    UnboundTarget(Uri),
    /// The edit carries a resource operation the client cannot apply.
    ResourceOperation,
}

/// Bind every document `edit` touches to the revision the request computed it
/// against — the one place an LSP edit's delivery shape is decided.
///
/// Every target is resolved through `revision_of` first; an
/// [`EditTargetRevision::Uncaptured`] target refuses the whole edit, so a
/// partial cross-file edit is never delivered. Then, for a
/// [`WorkspaceEditSupport::VersionedDocumentChanges`] client, `changes` entries
/// become `TextDocumentEdit`s (ordered by URI) and every `TextDocumentEdit`
/// carries its target's captured version, `null` only for a closed target. An
/// [`WorkspaceEditSupport::Unversioned`] client receives only `changes`: every
/// `TextDocumentEdit` is downgraded to it, and an edit carrying a resource
/// operation is refused whole with [`EditRefusal::ResourceOperation`] rather
/// than delivered without its creation step.
pub fn bind_workspace_edit(
    edit: &mut WorkspaceEdit,
    support: WorkspaceEditSupport,
    revision_of: &mut dyn FnMut(&Uri) -> EditTargetRevision,
) -> Result<(), EditRefusal> {
    let mut revisions: Vec<(Uri, EditTargetRevision)> = Vec::new();
    let mut resolve = |uri: &Uri| -> Result<EditTargetRevision, EditRefusal> {
        if let Some((_, revision)) = revisions.iter().find(|(known, _)| known == uri) {
            return Ok(*revision);
        }
        let revision = revision_of(uri);
        if revision == EditTargetRevision::Uncaptured {
            return Err(EditRefusal::UnboundTarget(uri.clone()));
        }
        revisions.push((uri.clone(), revision));
        Ok(revision)
    };

    if let Some(changes) = &edit.changes {
        for uri in changes.keys() {
            resolve(uri)?;
        }
    }
    let mut document_edits: Vec<&mut TextDocumentEdit> = match edit.document_changes.as_mut() {
        Some(DocumentChanges::Edits(edits)) => edits.iter_mut().collect(),
        Some(DocumentChanges::Operations(operations)) => operations
            .iter_mut()
            .filter_map(|operation| match operation {
                DocumentChangeOperation::Edit(edit) => Some(edit),
                DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
        None => Vec::new(),
    };
    let mut bound = Vec::with_capacity(document_edits.len());
    for document_edit in &mut document_edits {
        bound.push(resolve(&document_edit.text_document.uri)?);
    }
    if support == WorkspaceEditSupport::Unversioned {
        drop(document_edits);
        return downgrade_to_changes(edit);
    }

    for (document_edit, revision) in document_edits.into_iter().zip(bound) {
        document_edit.text_document.version = captured_version(revision);
    }
    let Some(changes) = edit.changes.take() else {
        return Ok(());
    };
    let mut changes: Vec<(Uri, Vec<TextEdit>)> = changes.into_iter().collect();
    changes.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    let converted = changes
        .into_iter()
        .map(|(uri, edits)| {
            let version = captured_version(resolve(&uri)?);
            Ok(TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier { uri, version },
                edits: edits.into_iter().map(OneOf::Left).collect(),
            })
        })
        .collect::<Result<Vec<_>, EditRefusal>>()?;
    match edit.document_changes.as_mut() {
        None => edit.document_changes = Some(DocumentChanges::Edits(converted)),
        Some(DocumentChanges::Edits(edits)) => edits.extend(converted),
        Some(DocumentChanges::Operations(operations)) => {
            operations.extend(converted.into_iter().map(DocumentChangeOperation::Edit))
        }
    }
    Ok(())
}

/// Rewrite `edit`'s `documentChanges` into `changes` for a client that cannot
/// apply `documentChanges`. Every route builds all of an edit's text against
/// the one captured revision of each target, so the edits of a target merge in
/// order. Change annotations need `documentChanges` and are dropped with it.
fn downgrade_to_changes(edit: &mut WorkspaceEdit) -> Result<(), EditRefusal> {
    if let Some(DocumentChanges::Operations(operations)) = &edit.document_changes {
        if operations
            .iter()
            .any(|operation| matches!(operation, DocumentChangeOperation::Op(_)))
        {
            return Err(EditRefusal::ResourceOperation);
        }
    }
    let document_edits: Vec<TextDocumentEdit> = match edit.document_changes.take() {
        None => return Ok(()),
        Some(DocumentChanges::Edits(edits)) => edits,
        Some(DocumentChanges::Operations(operations)) => operations
            .into_iter()
            .filter_map(|operation| match operation {
                DocumentChangeOperation::Edit(edit) => Some(edit),
                DocumentChangeOperation::Op(_) => None,
            })
            .collect(),
    };
    let changes = edit.changes.get_or_insert_with(Default::default);
    for document_edit in document_edits {
        changes
            .entry(document_edit.text_document.uri)
            .or_default()
            .extend(
                document_edit
                    .edits
                    .into_iter()
                    .map(|text_edit| match text_edit {
                        OneOf::Left(text_edit) => text_edit,
                        OneOf::Right(annotated) => annotated.text_edit,
                    }),
            );
    }
    edit.change_annotations = None;
    Ok(())
}

fn captured_version(revision: EditTargetRevision) -> Option<i32> {
    match revision {
        EditTargetRevision::Open(version) => Some(version),
        EditTargetRevision::Closed | EditTargetRevision::Uncaptured => None,
    }
}

/// Re-export for backwards compatibility.
pub use super::sentinel_uris::PLACEHOLDER_URI_STR as SAME_FILE_URI;
pub use super::sentinel_uris::{PLACEHOLDER_URI, PLACEHOLDER_URI_STR};

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_script_insert_offset ───────────────────────────────────────

    #[test]
    fn insert_offset_after_last_import() {
        let source = "<script setup>\nimport { ref } from 'vue'\nimport { computed } from 'vue'\nconst x = 1\n</script>";
        let analysis = FileAnalysisSnapshot {
            imports: vec![
                verter_session_query::analysis::types::AnalyzedImport {
                    source: "vue".into(),
                    owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(15, 40),
                    resolved_canonical_id: None,
                },
                verter_session_query::analysis::types::AnalyzedImport {
                    source: "vue".into(),
                    owner: verter_type_expr::TopLevelOwnerId::instance(0),
                    is_type_only: false,
                    bindings: vec![],
                    span: verter_span::Span::new(41, 71),
                    resolved_canonical_id: None,
                },
            ],
            ..Default::default()
        };
        let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
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
        let blocks = crate::documents::carrier_structure::test_carrier_blocks(source);
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

    // ── bind_workspace_edit ─────────────────────────────────────────────

    fn insert(line: u32, text: &str) -> TextEdit {
        TextEdit {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: 0 },
            },
            new_text: text.to_string(),
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn changes_edit(targets: &[(&Uri, TextEdit)]) -> WorkspaceEdit {
        let mut changes = std::collections::HashMap::new();
        for (uri, edit) in targets {
            changes
                .entry((*uri).clone())
                .or_insert_with(Vec::new)
                .push(edit.clone());
        }
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }
    }

    fn revisions<'a>(
        open: &'a Uri,
        closed: &'a Uri,
    ) -> impl FnMut(&Uri) -> EditTargetRevision + 'a {
        move |uri: &Uri| {
            if uri == open {
                EditTargetRevision::Open(4)
            } else if uri == closed {
                EditTargetRevision::Closed
            } else {
                EditTargetRevision::Uncaptured
            }
        }
    }

    #[test]
    fn versioned_client_receives_one_versioned_document_edit_per_target() {
        let open: Uri = "file:///b/Open.vue".parse().unwrap();
        let closed: Uri = "file:///a/closed.ts".parse().unwrap();
        let mut edit = changes_edit(&[(&open, insert(1, "x")), (&closed, insert(2, "y"))]);

        bind_workspace_edit(
            &mut edit,
            WorkspaceEditSupport::VersionedDocumentChanges,
            &mut revisions(&open, &closed),
        )
        .expect("every target is bound");

        assert!(edit.changes.is_none(), "no unversioned `changes` survive");
        let Some(DocumentChanges::Edits(edits)) = &edit.document_changes else {
            panic!("expected TextDocumentEdits, got {edit:?}");
        };
        let delivered: Vec<(&str, Option<i32>, usize)> = edits
            .iter()
            .map(|edit| {
                (
                    edit.text_document.uri.as_str(),
                    edit.text_document.version,
                    edit.edits.len(),
                )
            })
            .collect();
        assert_eq!(
            delivered,
            vec![
                ("file:///a/closed.ts", None, 1),
                ("file:///b/Open.vue", Some(4), 1)
            ],
            "ordered by URI; the open target names its captured version, the closed one null"
        );
    }

    #[test]
    fn versioned_client_versions_existing_document_changes() {
        let open: Uri = "file:///Open.vue".parse().unwrap();
        let closed: Uri = "file:///New.vue".parse().unwrap();
        let mut edit = make_insert_edit(&open, Position::default(), "a".into());
        let mut create = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: closed.clone(),
                    options: None,
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: open.clone(),
                        version: Some(99),
                    },
                    edits: vec![OneOf::Left(insert(0, "b"))],
                }),
            ])),
            ..Default::default()
        };

        for edit in [&mut edit, &mut create] {
            bind_workspace_edit(
                edit,
                WorkspaceEditSupport::VersionedDocumentChanges,
                &mut revisions(&open, &closed),
            )
            .expect("bound");
        }

        let Some(DocumentChanges::Edits(edits)) = &edit.document_changes else {
            panic!("expected edits");
        };
        assert_eq!(edits[0].text_document.version, Some(4));
        let Some(DocumentChanges::Operations(operations)) = &create.document_changes else {
            panic!("expected operations");
        };
        assert!(
            matches!(
                &operations[0],
                DocumentChangeOperation::Op(ResourceOp::Create(_))
            ),
            "resource operations keep their place"
        );
        let DocumentChangeOperation::Edit(versioned) = &operations[1] else {
            panic!("expected an edit operation");
        };
        assert_eq!(
            versioned.text_document.version,
            Some(4),
            "the captured version replaces whatever version the route wrote"
        );
    }

    #[test]
    fn unversioned_client_keeps_changes_edits_as_they_are() {
        let open: Uri = "file:///Open.vue".parse().unwrap();
        let closed: Uri = "file:///closed.ts".parse().unwrap();
        let mut edit = changes_edit(&[(&open, insert(1, "x"))]);
        let before = edit.clone();

        bind_workspace_edit(
            &mut edit,
            WorkspaceEditSupport::Unversioned,
            &mut revisions(&open, &closed),
        )
        .expect("bound");

        assert_eq!(edit, before);
    }

    #[test]
    fn unversioned_client_receives_document_changes_as_changes() {
        let open: Uri = "file:///Open.vue".parse().unwrap();
        let closed: Uri = "file:///closed.ts".parse().unwrap();
        let mut edit = make_insert_edit(&open, Position::default(), "a".into());
        let Some(DocumentChanges::Edits(edits)) = edit.document_changes.as_mut() else {
            panic!("expected edits");
        };
        edits.push(TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: closed.clone(),
                version: Some(7),
            },
            edits: vec![OneOf::Left(insert(2, "b"))],
        });
        edit.change_annotations = Some(Default::default());

        bind_workspace_edit(
            &mut edit,
            WorkspaceEditSupport::Unversioned,
            &mut revisions(&open, &closed),
        )
        .expect("bound");

        assert!(edit.document_changes.is_none(), "{edit:?}");
        assert!(edit.change_annotations.is_none(), "{edit:?}");
        let changes = edit.changes.as_ref().expect("changes");
        assert_eq!(changes.len(), 2, "{edit:?}");
        assert_eq!(changes[&open].len(), 1);
        assert_eq!(changes[&open][0].new_text, "a");
        assert_eq!(changes[&closed][0].new_text, "b");
    }

    #[test]
    fn unversioned_client_refuses_an_edit_that_needs_a_resource_operation() {
        let open: Uri = "file:///Open.vue".parse().unwrap();
        let closed: Uri = "file:///New.vue".parse().unwrap();
        let mut edit = WorkspaceEdit {
            document_changes: Some(DocumentChanges::Operations(vec![
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: closed.clone(),
                    options: None,
                    annotation_id: None,
                })),
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: open.clone(),
                        version: None,
                    },
                    edits: vec![OneOf::Left(insert(0, "b"))],
                }),
            ])),
            ..Default::default()
        };
        let before = edit.clone();

        let refused = bind_workspace_edit(
            &mut edit,
            WorkspaceEditSupport::Unversioned,
            &mut revisions(&open, &closed),
        );

        assert_eq!(refused, Err(EditRefusal::ResourceOperation));
        assert_eq!(edit, before, "a refused edit is not partially rewritten");
    }

    #[test]
    fn an_uncaptured_open_target_refuses_the_whole_edit() {
        let open: Uri = "file:///Open.vue".parse().unwrap();
        let closed: Uri = "file:///closed.ts".parse().unwrap();
        let uncaptured: Uri = "file:///Other.vue".parse().unwrap();
        for support in [
            WorkspaceEditSupport::VersionedDocumentChanges,
            WorkspaceEditSupport::Unversioned,
        ] {
            let mut edit = changes_edit(&[(&open, insert(1, "x")), (&uncaptured, insert(2, "y"))]);
            let before = edit.clone();

            let refused = bind_workspace_edit(&mut edit, support, &mut revisions(&open, &closed));

            assert_eq!(
                refused,
                Err(EditRefusal::UnboundTarget(uncaptured.clone())),
                "{support:?}"
            );
            assert_eq!(
                edit, before,
                "{support:?}: a refused edit is not partially rewritten"
            );
        }
    }

    #[test]
    fn negotiation_reads_workspace_edit_document_changes() {
        let advertised = |document_changes: Option<bool>| ClientCapabilities {
            workspace: Some(WorkspaceClientCapabilities {
                workspace_edit: Some(WorkspaceEditClientCapabilities {
                    document_changes,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            WorkspaceEditSupport::negotiate(&advertised(Some(true))),
            WorkspaceEditSupport::VersionedDocumentChanges
        );
        assert_eq!(
            WorkspaceEditSupport::negotiate(&advertised(Some(false))),
            WorkspaceEditSupport::Unversioned
        );
        assert_eq!(
            WorkspaceEditSupport::negotiate(&ClientCapabilities::default()),
            WorkspaceEditSupport::Unversioned
        );
    }

    // ── LiveEditTarget::anchor_position (F3) ────────────────────────────
    //
    // Anchors are minted by `verter_semantic` from live OXC spans, so these
    // cases exercise the conversion boundary directly: it is the single point
    // where a macro anchor becomes an editable position, and it must refuse an
    // offset it cannot prove addresses the live bytes.

    /// Build an anchor by running the real analyzer over a `defineSlots` source
    /// and taking the type-literal anchor it mints.
    fn minted_type_literal_anchor(source: &str) -> MemberListAnchor {
        let script = crate::features::macro_fixture::analyze_sfc_script(source);
        *script.macros[0]
            .edit_anchors
            .type_literal
            .available()
            .expect("a type-literal type argument mints an available anchor")
    }

    #[test]
    fn anchor_position_maps_an_in_bounds_anchor() {
        let source = "<script setup lang=\"ts\">\ndefineSlots<{}>()\n</script>";
        let anchor = minted_type_literal_anchor(source);
        let line_index = LineIndex::new_utf16(source);
        let target = LiveEditTarget::new(source, &line_index);

        let position = target
            .anchor_position(&anchor)
            .expect("an anchor minted from these bytes must map");
        // Positive: the anchor addresses the type literal's `}`.
        let offset = line_index.position_to_offset(&position).unwrap() as usize;
        assert_eq!(&source[offset..offset + 1], "}");
        // Negative: it is NOT the `)` the old `span.end - 4` arithmetic found.
        assert_ne!(&source[offset..offset + 1], ")");
    }

    /// Contract test for the out-of-range half of F3. `LineIndex` independently
    /// refuses an out-of-range offset, so this case is satisfied by two guards
    /// and does not by itself discriminate the `is_char_boundary` predicate —
    /// the char-boundary case below does that. It is retained because the
    /// REQUIRED observable is "no position from an out-of-range anchor",
    /// whichever guard supplies it.
    #[test]
    fn anchor_position_out_of_bounds_returns_none() {
        let analyzed =
            "<script setup lang=\"ts\">\ndefineSlots<{}>()\nconst padding = 1\n</script>";
        let anchor = minted_type_literal_anchor(analyzed);
        let live = "<";
        assert!(
            anchor.insert_offset() as usize > live.len(),
            "fixture must be out of range"
        );
        let line_index = LineIndex::new_utf16(live);
        let target = LiveEditTarget::new(live, &line_index);

        assert!(
            target.anchor_position(&anchor).is_none(),
            "an offset past the live buffer's end is a typed miss, never a clamped position"
        );
    }

    #[test]
    fn anchor_position_off_char_boundary_returns_none() {
        let analyzed = "<script setup lang=\"ts\">\ndefineSlots<{}>()\n</script>";
        let anchor = minted_type_literal_anchor(analyzed);
        let offset = anchor.insert_offset() as usize;
        // Same byte length, but a 3-byte character now straddles the anchor
        // offset — an in-bounds offset that addresses no character boundary.
        let mut live = analyzed.as_bytes()[..offset - 1].to_vec();
        live.extend_from_slice("★".as_bytes());
        live.extend_from_slice(&analyzed.as_bytes()[offset + 2..]);
        let live = String::from_utf8(live).expect("valid utf-8");
        assert_eq!(live.len(), analyzed.len(), "byte length must be preserved");
        assert!(
            !live.is_char_boundary(offset),
            "fixture must actually straddle the anchor offset"
        );

        let line_index = LineIndex::new_utf16(&live);
        let target = LiveEditTarget::new(&live, &line_index);
        assert!(
            target.anchor_position(&anchor).is_none(),
            "an offset off a UTF-8 character boundary is a typed miss"
        );
    }
}
