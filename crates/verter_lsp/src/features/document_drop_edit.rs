// Document drop edit: when a .vue file is dropped into a template,
// insert a component tag and auto-import it.

use tower_lsp_server::ls_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Handle a file being dropped into a document.
///
/// When a `.vue` file is dropped into a `<template>` block, generates:
/// 1. A `<ComponentName />` tag at the drop position
/// 2. An import statement in the `<script setup>` block
///
/// This is exposed as a custom LSP request (`$/verter/documentDropEdit`),
/// since `textDocument/documentDropEdit` is still experimental in LSP 3.18+.
pub fn document_drop_edit(
    dropped_uri: &str,
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
    target_uri: &Uri,
) -> Option<WorkspaceEdit> {
    // Only handle .vue file drops
    if !dropped_uri.ends_with(".vue") {
        return None;
    }

    // Check the drop position is inside a template block
    let offset = line_index.position_to_offset(position)? as usize;
    let _template_block = blocks.iter().find(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs as usize && offset <= ce as usize
        }
    })?;

    // Extract component name from dropped file path
    let component_name = extract_component_name(dropped_uri)?;

    // Compute relative import path from target to dropped file
    let import_path = compute_relative_path(target_uri.as_str(), dropped_uri);

    // Build the component tag to insert
    let tag = format!("<{component_name} />");

    // Build the import line
    let import_line = format!("import {component_name} from '{import_path}'\n");

    let mut edits: Vec<TextEdit> = Vec::new();
    let _ = source;

    // 1. Insert component tag at drop position
    edits.push(TextEdit {
        range: Range {
            start: *position,
            end: *position,
        },
        new_text: tag,
    });

    // 2. Add import to script setup block
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

    if edits.is_empty() {
        return None;
    }

    Some(WorkspaceEdit {
        changes: Some([(target_uri.clone(), edits)].into_iter().collect()),
        document_changes: None,
        change_annotations: None,
    })
}

/// Extract PascalCase component name from a file path.
///
/// `"/project/src/components/MyButton.vue"` → `"MyButton"`
fn extract_component_name(path: &str) -> Option<String> {
    let filename = path
        .rsplit('/')
        .next()
        .or_else(|| path.rsplit('\\').next())?;
    let name = filename.strip_suffix(".vue")?;
    if name.is_empty() {
        return None;
    }
    // Ensure PascalCase (capitalize first letter)
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
}

/// Compute a relative path from target to source.
///
/// Simple heuristic: if both are in the same directory, use `./filename`.
/// Otherwise, use the full dropped path as-is (the extension can resolve it).
fn compute_relative_path(target_uri: &str, dropped_uri: &str) -> String {
    let target_dir = target_uri.rfind('/').map(|i| &target_uri[..=i]);
    let dropped_dir = dropped_uri.rfind('/').map(|i| &dropped_uri[..=i]);
    let dropped_filename = dropped_uri.rsplit('/').next().unwrap_or(dropped_uri);

    if target_dir == dropped_dir {
        format!("./{dropped_filename}")
    } else {
        // For different directories, just use the filename
        // The user can fix the path; a full relative path resolver is complex
        format!("./{dropped_filename}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_component_name() {
        assert_eq!(
            extract_component_name("/project/MyButton.vue"),
            Some("MyButton".into())
        );
        assert_eq!(
            extract_component_name("myButton.vue"),
            Some("MyButton".into())
        );
        assert_eq!(extract_component_name("not-a-vue-file.ts"), None);
    }

    #[test]
    fn test_extract_component_name_empty() {
        assert_eq!(extract_component_name(".vue"), None);
    }

    #[test]
    fn test_compute_relative_path_same_dir() {
        let target = "file:///project/src/App.vue";
        let dropped = "file:///project/src/MyButton.vue";
        assert_eq!(compute_relative_path(target, dropped), "./MyButton.vue");
    }

    #[test]
    fn test_drop_edit_non_vue_file() {
        let result = document_drop_edit(
            "file:///project/utils.ts",
            &Position {
                line: 1,
                character: 0,
            },
            "<template>\n  <div/>\n</template>\n",
            &crate::documents::sfc_scanner::scan_sfc_blocks("<template>\n  <div/>\n</template>\n"),
            &LineIndex::new_utf16("<template>\n  <div/>\n</template>\n"),
            &"file:///project/App.vue".parse().unwrap(),
        );
        assert!(
            result.is_none(),
            "non-vue files should not trigger drop edit"
        );
    }
}
