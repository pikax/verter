// Document formatting: basic SFC formatting with block-level indentation.
//
// Full formatting delegates to external tools (Prettier/dprint) per block.
// This implementation provides basic structural formatting:
// - Ensure consistent newlines between SFC blocks
// - Normalize block-level indentation

use tower_lsp_server::lsp_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Format an SFC document.
///
/// Provides basic structural formatting:
/// - Ensures single blank line between top-level SFC blocks
/// - Trims trailing whitespace
/// - Ensures file ends with a newline
///
/// Note: This does not format the *content* of blocks (HTML, CSS, JS/TS).
/// Content formatting should be delegated to specialized formatters
/// (Prettier, dprint, etc.) via the client's formatting pipeline.
pub fn format_document(
    source: &str,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
    _options: &FormattingOptions,
) -> Vec<TextEdit> {
    let mut edits = Vec::new();

    // 1. Ensure single blank line between consecutive blocks
    let mut sorted_blocks: Vec<&SfcBlock> = blocks.iter().collect();
    sorted_blocks.sort_by_key(|b| b.open_tag_start);

    for window in sorted_blocks.windows(2) {
        let prev_end = window[0].close_tag_end;
        let next_start = window[1].open_tag_start;

        // Get the text between blocks
        let between = match source.get(prev_end as usize..next_start as usize) {
            Some(s) => s,
            None => continue,
        };

        // Count newlines in the gap
        let newline_count = between.chars().filter(|&c| c == '\n').count();
        let is_only_whitespace = between.chars().all(|c| c.is_whitespace());

        // We want exactly 2 newlines (one blank line) between blocks
        if is_only_whitespace && newline_count != 2 {
            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(prev_end),
                line_index.offset_to_position(next_start),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: "\n\n".to_string(),
                });
            }
        }
    }

    // 2. Ensure file ends with exactly one newline
    if !source.is_empty() {
        let trimmed_end = source.trim_end();
        let trailing = &source[trimmed_end.len()..];

        if trailing != "\n" {
            let start_offset = trimmed_end.len() as u32;
            let end_offset = source.len() as u32;

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(start_offset),
                line_index.offset_to_position(end_offset),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: "\n".to_string(),
                });
            }
        }
    }

    // 3. Trim trailing whitespace on each line
    for (line_num, line) in source.lines().enumerate() {
        let trimmed = line.trim_end();
        if trimmed.len() < line.len() {
            let line_start_offset = line_index
                .position_to_offset(&Position {
                    line: line_num as u32,
                    character: 0,
                })
                .unwrap_or(0);

            let ws_start = line_start_offset + trimmed.len() as u32;
            let ws_end = line_start_offset + line.len() as u32;

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(ws_start),
                line_index.offset_to_position(ws_end),
            ) {
                edits.push(TextEdit {
                    range: Range { start, end },
                    new_text: String::new(),
                });
            }
        }
    }

    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;

    fn default_options() -> FormattingOptions {
        FormattingOptions {
            tab_size: 2,
            insert_spaces: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_no_edits_for_well_formatted() {
        let source =
            "<template>\n  <div />\n</template>\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let edits = format_document(source, &blocks, &line_index, &default_options());
        assert!(edits.is_empty(), "well-formatted file should have no edits");
    }

    #[test]
    fn test_missing_trailing_newline() {
        let source = "<template>\n  <div />\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let edits = format_document(source, &blocks, &line_index, &default_options());
        assert!(!edits.is_empty(), "should add trailing newline");
    }

    #[test]
    fn test_excessive_blank_lines() {
        let source =
            "<template>\n  <div />\n</template>\n\n\n\n<script setup>\nconst x = 1\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let edits = format_document(source, &blocks, &line_index, &default_options());
        // Should normalize to exactly one blank line between blocks
        let block_gap_edit = edits.iter().find(|e| e.new_text == "\n\n");
        assert!(
            block_gap_edit.is_some(),
            "should normalize blank lines between blocks"
        );
    }

    #[test]
    fn test_trailing_whitespace_removed() {
        let source = "<template>  \n  <div />\n</template>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let edits = format_document(source, &blocks, &line_index, &default_options());
        let ws_edit = edits.iter().find(|e| e.new_text.is_empty());
        assert!(ws_edit.is_some(), "should remove trailing whitespace");
    }
}
