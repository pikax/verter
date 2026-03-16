// Extract component: refactor selected template fragment into a new .vue file.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Generate "Extract to Component" code action for a selected template range.
///
/// When the user selects a range within a `<template>` block, this returns a code action
/// that will:
/// 1. Create a new .vue file with the selected template fragment
/// 2. Replace the selection with a `<ComponentName />` tag
/// 3. Add an import statement for the new component
///
/// Returns `None` if the selection is empty or not within a template block.
pub fn extract_component_action(
    source: &str,
    range: &Range,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
    uri: &Uri,
) -> Option<CodeActionOrCommand> {
    // Only offer extraction for non-empty selections
    if range.start == range.end {
        return None;
    }

    // Find template block
    let template_block = blocks.iter().find(|b| b.tag_name == "template")?;
    let (content_start, content_end) = template_block.content_range();

    // Convert selection to byte offsets
    let sel_start = line_index.position_to_offset(&range.start)? as usize;
    let sel_end = line_index.position_to_offset(&range.end)? as usize;

    // Ensure selection is within the template block
    if sel_start < content_start as usize || sel_end > content_end as usize {
        return None;
    }

    // Extract the selected text
    let selected_text = source.get(sel_start..sel_end)?;

    // Skip if selection is just whitespace
    if selected_text.trim().is_empty() {
        return None;
    }

    // Generate component name from URI
    let component_name = generate_component_name(uri);

    // Build the new component file content
    let new_component_source = format!(
        "<template>\n  {}\n</template>\n\n<script setup lang=\"ts\">\n</script>\n",
        selected_text.trim()
    );

    // Build the replacement tag
    let replacement_tag = format!("<{component_name} />");

    // Build the import statement to add
    let import_line = format!("import {component_name} from './{component_name}.vue'\n");

    // Build edits for the current file:
    // 1. Replace selection with component tag
    let mut edits = vec![TextEdit {
        range: *range,
        new_text: replacement_tag,
    }];

    // 2. Add import to script block (if there's a <script setup>)
    if let Some(script_block) = blocks
        .iter()
        .find(|b| b.tag_name == "script" && b.is_setup())
    {
        let (script_start, _) = script_block.content_range();
        if let Some(import_pos) = line_index.offset_to_position(script_start) {
            edits.push(TextEdit {
                range: Range {
                    start: import_pos,
                    end: import_pos,
                },
                new_text: import_line,
            });
        }
    }

    // Build the code action
    // Note: Creating the new file requires a CreateFile resource operation,
    // which is supported via DocumentChanges.
    let new_file_uri = build_sibling_uri(uri, &component_name)?;

    #[allow(deprecated)]
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Extract to <{component_name} />"),
        kind: Some(CodeActionKind::REFACTOR_EXTRACT),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Operations(vec![
                // Create the new file
                DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                    uri: new_file_uri.clone(),
                    options: Some(CreateFileOptions {
                        overwrite: Some(false),
                        ignore_if_exists: Some(true),
                    }),
                    annotation_id: None,
                })),
                // Write content to the new file
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: new_file_uri,
                        version: None,
                    },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range::default(),
                        new_text: new_component_source,
                    })],
                }),
                // Edit the current file (replace selection + add import)
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: None,
                    },
                    edits: edits.into_iter().map(OneOf::Left).collect(),
                }),
            ])),
            change_annotations: None,
        }),
        is_preferred: None,
        disabled: None,
        data: None,
        command: None,
    }))
}

/// Generate a PascalCase component name from a counter.
/// Uses "ExtractedComponent" as the base name.
fn generate_component_name(_uri: &Uri) -> String {
    "ExtractedComponent".to_string()
}

/// Build a sibling file URI for the new component.
fn build_sibling_uri(uri: &Uri, component_name: &str) -> Option<Uri> {
    let uri_str = uri.as_str();
    // Find the last '/' to get the directory
    let dir_end = uri_str.rfind('/')?;
    let dir = &uri_str[..=dir_end];
    format!("{dir}{component_name}.vue").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;

    #[test]
    fn test_extract_component_in_template() {
        let source = "<template>\n  <div>\n    <span>Hello</span>\n  </div>\n</template>\n\n<script setup lang=\"ts\">\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

        // Select the <span>Hello</span> part
        let range = Range {
            start: Position {
                line: 2,
                character: 4,
            },
            end: Position {
                line: 2,
                character: 23,
            },
        };

        let action = extract_component_action(source, &range, &blocks, &line_index, &uri);
        assert!(action.is_some());
        let action = action.unwrap();
        if let CodeActionOrCommand::CodeAction(ca) = action {
            assert!(ca.title.contains("Extract to"));
            assert_eq!(ca.kind, Some(CodeActionKind::REFACTOR_EXTRACT));
            assert!(ca.edit.is_some());
        } else {
            panic!("Expected CodeAction");
        }
    }

    #[test]
    fn test_no_extract_empty_selection() {
        let source = "<template>\n  <div/>\n</template>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

        let range = Range {
            start: Position {
                line: 1,
                character: 2,
            },
            end: Position {
                line: 1,
                character: 2,
            },
        };

        let action = extract_component_action(source, &range, &blocks, &line_index, &uri);
        assert!(action.is_none(), "empty selection should not extract");
    }

    #[test]
    fn test_no_extract_outside_template() {
        let source =
            "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let uri: Uri = "file:///project/src/App.vue".parse().unwrap();

        // Select inside script block
        let range = Range {
            start: Position {
                line: 4,
                character: 0,
            },
            end: Position {
                line: 4,
                character: 11,
            },
        };

        let action = extract_component_action(source, &range, &blocks, &line_index, &uri);
        assert!(
            action.is_none(),
            "selection outside template should not extract"
        );
    }

    #[test]
    fn test_build_sibling_uri() {
        let uri: Uri = "file:///project/src/App.vue".parse().unwrap();
        let sibling = build_sibling_uri(&uri, "MyComponent");
        assert!(sibling.is_some());
        assert!(sibling.unwrap().as_str().ends_with("MyComponent.vue"));
    }
}
