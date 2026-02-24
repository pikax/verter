// Phase 2: Folding ranges from SFC block boundaries.

use tower_lsp_server::lsp_types::*;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Build folding ranges from SFC blocks.
///
/// Each block produces a folding range spanning from the opening tag line
/// to the closing tag line. The fold kind is `Region` for all blocks.
pub fn build_folding_ranges(blocks: &[SfcBlock], line_index: &LineIndex) -> Vec<FoldingRange> {
    blocks
        .iter()
        .filter_map(|block| {
            let start = line_index.offset_to_position(block.open_tag_start)?;
            let end = line_index.offset_to_position(block.close_tag_end)?;

            // Only fold if the block spans more than one line
            if start.line >= end.line {
                return None;
            }

            Some(FoldingRange {
                start_line: start.line,
                start_character: Some(start.character),
                end_line: end.line,
                end_character: Some(end.character),
                kind: Some(FoldingRangeKind::Region),
                collapsed_text: Some(format!("<{}>...", block.tag_name)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;

    #[test]
    fn test_basic_folding() {
        let source =
            "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);
        let ranges = build_folding_ranges(&blocks, &line_index);

        assert_eq!(ranges.len(), 2);
        // Template: lines 0-2
        assert_eq!(ranges[0].start_line, 0);
        assert_eq!(ranges[0].end_line, 2);
        // Script: lines 4-6
        assert_eq!(ranges[1].start_line, 4);
        assert_eq!(ranges[1].end_line, 6);
    }

    #[test]
    fn test_single_line_block_not_foldable() {
        let source = "<style>.foo {}</style>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);
        let ranges = build_folding_ranges(&blocks, &line_index);

        // Single-line block should not produce a folding range
        assert!(ranges.is_empty());
    }

    #[test]
    fn test_collapsed_text() {
        let source = "<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new(source);
        let ranges = build_folding_ranges(&blocks, &line_index);

        assert_eq!(ranges[0].collapsed_text, Some("<script>...".to_string()));
    }
}
