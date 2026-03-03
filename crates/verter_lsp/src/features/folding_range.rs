// Folding ranges from SFC block boundaries + template elements.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Build folding ranges from SFC blocks and template elements.
///
/// Produces folding ranges for:
/// - SFC block boundaries (template, script, style) — folds at the block level
/// - Template elements that span multiple lines — folds within the template
pub fn build_folding_ranges(
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Vec<FoldingRange> {
    let mut ranges: Vec<FoldingRange> = blocks
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
        .collect();

    // Add template element folding ranges
    if let Some(template) = analysis.and_then(|a| a.template.as_ref()) {
        for elem in &template.elements {
            // Skip self-closing elements — they never span multiple lines
            if elem.is_self_closing {
                continue;
            }

            if let (Some(start), Some(end)) = (
                line_index.offset_to_position(elem.span.start),
                line_index.offset_to_position(elem.span.end),
            ) {
                // Only fold if the element spans more than one line
                if start.line < end.line {
                    ranges.push(FoldingRange {
                        start_line: start.line,
                        start_character: Some(start.character),
                        end_line: end.line,
                        end_character: Some(end.character),
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some(format!("<{}>...", elem.tag)),
                    });
                }
            }
        }
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement};

    #[test]
    fn test_basic_folding() {
        let source =
            "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let ranges = build_folding_ranges(&blocks, None, &line_index);

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
        let line_index = LineIndex::new_utf16(source);
        let ranges = build_folding_ranges(&blocks, None, &line_index);

        // Single-line block should not produce a folding range
        assert!(ranges.is_empty());
    }

    #[test]
    fn test_collapsed_text() {
        let source = "<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);
        let ranges = build_folding_ranges(&blocks, None, &line_index);

        assert_eq!(ranges[0].collapsed_text, Some("<script>...".to_string()));
    }

    #[test]
    fn test_template_element_folding() {
        let source = "<template>\n  <div>\n    <p>hello</p>\n  </div>\n</template>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "div".to_string(),
                    is_self_closing: false,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(13, 44),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let ranges = build_folding_ranges(&blocks, Some(&analysis), &line_index);
        // 1 block fold (template) + 1 element fold (div)
        assert_eq!(ranges.len(), 2);
        // Template block: lines 0-4
        assert_eq!(ranges[0].start_line, 0);
        // Div element: lines 1-3
        assert_eq!(ranges[1].start_line, 1);
        assert_eq!(ranges[1].end_line, 3);
        assert_eq!(ranges[1].collapsed_text, Some("<div>...".to_string()));
    }

    #[test]
    fn test_self_closing_not_foldable() {
        let source = "<template>\n  <br />\n</template>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "br".to_string(),
                    is_self_closing: true,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(13, 19),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let ranges = build_folding_ranges(&blocks, Some(&analysis), &line_index);
        // Only template block fold, no element fold for self-closing
        assert_eq!(ranges.len(), 1);
    }

    #[test]
    fn test_single_line_element_not_foldable() {
        let source = "<template>\n  <div>inline</div>\n</template>\n";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = FileAnalysisSnapshot {
            template: Some(TemplateAnalysisSnapshot {
                elements: vec![TemplateElement {
                    tag: "div".to_string(),
                    is_self_closing: false,
                    dynamic_classes: vec![],
                    span: verter_span::Span::new(13, 30), // same line
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };

        let ranges = build_folding_ranges(&blocks, Some(&analysis), &line_index);
        // Only template block fold, single-line element doesn't fold
        assert_eq!(ranges.len(), 1);
    }
}
