// Linked editing ranges: auto-rename matching open/close HTML tags.

use tower_lsp_server::ls_types::*;
use verter_host::FileAnalysisSnapshot;

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;

/// Find linked editing ranges for tag names at the given position.
///
/// When the cursor is on an opening tag name (e.g., `<div>`), returns ranges for
/// both the opening and closing tag names so they can be edited simultaneously.
/// Self-closing elements (e.g., `<br />`) return `None`.
pub fn linked_editing_ranges(
    position: &Position,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
) -> Option<LinkedEditingRanges> {
    let offset = line_index.position_to_offset(position)?;

    // Only applicable in template blocks
    let _template_block = blocks.iter().find(|b| {
        b.tag_name == "template" && {
            let (cs, ce) = b.content_range();
            offset >= cs && offset <= ce
        }
    })?;

    let template = analysis?.template.as_ref()?;

    // Find an element whose tag name range contains the cursor
    for elem in &template.elements {
        if elem.is_self_closing {
            continue;
        }

        let tag_bytes = elem.tag.as_bytes();
        let tag_len = tag_bytes.len() as u32;

        // Open tag name: starts at span_start + 1 (skip '<'), length = tag.len()
        let open_name_start = elem.span.start + 1;
        let open_name_end = open_name_start + tag_len;

        // Close tag name: find "</tagname>" ending at span_end.
        // Pattern: </tag_name> — close tag end is at span_end.
        // We search backwards from span_end for the close tag.
        let close_range = find_close_tag_name(source, elem.span.end, &elem.tag)?;

        let cursor_on_open = offset >= open_name_start && offset <= open_name_end;
        let cursor_on_close = offset >= close_range.0 && offset <= close_range.1;

        if cursor_on_open || cursor_on_close {
            let open_start = line_index.offset_to_position(open_name_start)?;
            let open_end = line_index.offset_to_position(open_name_end)?;
            let close_start = line_index.offset_to_position(close_range.0)?;
            let close_end = line_index.offset_to_position(close_range.1)?;

            return Some(LinkedEditingRanges {
                ranges: vec![
                    Range {
                        start: open_start,
                        end: open_end,
                    },
                    Range {
                        start: close_start,
                        end: close_end,
                    },
                ],
                word_pattern: None,
            });
        }
    }

    None
}

/// Search backwards from `span_end` to find the close tag name range.
///
/// Given `</tagname>` at the end, returns (start, end) of `tagname`.
/// Returns `None` if the close tag pattern is not found.
fn find_close_tag_name(source: &str, span_end: u32, tag: &str) -> Option<(u32, u32)> {
    let bytes = source.as_bytes();
    let end = span_end as usize;

    if end == 0 || end > bytes.len() {
        return None;
    }

    // span_end points past the closing '>'
    // Walk backwards: '>' then tag name then '</'
    let gt_pos = end - 1;
    if bytes.get(gt_pos).copied() != Some(b'>') {
        return None;
    }

    // Skip optional whitespace between tag name and '>'
    let mut pos = gt_pos;
    while pos > 0 {
        pos -= 1;
        if bytes[pos] != b' ' && bytes[pos] != b'\t' && bytes[pos] != b'\n' && bytes[pos] != b'\r' {
            break;
        }
    }

    // Check tag name matches (backwards)
    let tag_bytes = tag.as_bytes();
    let tag_end = pos + 1;
    if tag_end < tag_bytes.len() {
        return None;
    }
    let tag_start = tag_end - tag_bytes.len();

    if &bytes[tag_start..tag_end] != tag_bytes {
        return None;
    }

    // Check '</' prefix (possibly with whitespace after '</')
    let mut check = tag_start;
    // Skip whitespace between '</' and tag name
    while check > 0 {
        check -= 1;
        if bytes[check] != b' ' && bytes[check] != b'\t' {
            break;
        }
    }

    if check == 0 || bytes[check] != b'/' {
        return None;
    }
    if check == 0 || bytes[check - 1] != b'<' {
        return None;
    }

    Some((tag_start as u32, tag_end as u32))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::sfc_scanner::scan_sfc_blocks;
    use verter_analysis::*;

    fn make_template_analysis(elements: Vec<TemplateElement>) -> FileAnalysisSnapshot {
        FileAnalysisSnapshot {
            template: Some(
                (TemplateAnalysisSnapshot {
                    elements,
                    ..Default::default()
                })
                .into(),
            ),
            ..Default::default()
        }
    }

    #[test]
    fn test_linked_editing_on_open_tag() {
        // <template>\n<div>hello</div>\n</template>
        let source = "<template>\n<div>hello</div>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // <div> starts at offset 11, </div> ends at offset 27
        let analysis = make_template_analysis(vec![TemplateElement {
            tag: "div".to_string(),
            is_self_closing: false,
            dynamic_classes: vec![],
            span: verter_span::Span::new(11, 27),
            content_end: 0,
            ..Default::default()
        }]);

        // Cursor on 'd' in opening <div> — offset 12 = line 1, char 1
        let pos = line_index.offset_to_position(12).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        let ranges = result.unwrap().ranges;
        assert_eq!(ranges.len(), 2);
        // Open tag name: "div" at offsets 12..15
        assert_eq!(ranges[0].start, line_index.offset_to_position(12).unwrap());
        assert_eq!(ranges[0].end, line_index.offset_to_position(15).unwrap());
        // Close tag name: "div" at offsets 23..26
        assert_eq!(ranges[1].start, line_index.offset_to_position(23).unwrap());
        assert_eq!(ranges[1].end, line_index.offset_to_position(26).unwrap());
    }

    #[test]
    fn test_linked_editing_on_close_tag() {
        let source = "<template>\n<div>hello</div>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_template_analysis(vec![TemplateElement {
            tag: "div".to_string(),
            is_self_closing: false,
            dynamic_classes: vec![],
            span: verter_span::Span::new(11, 27),
            content_end: 0,
            ..Default::default()
        }]);

        // Cursor on 'd' in closing </div> — offset 23
        let pos = line_index.offset_to_position(23).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());
        assert_eq!(result.unwrap().ranges.len(), 2);
    }

    #[test]
    fn test_self_closing_returns_none() {
        let source = "<template>\n<br />\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_template_analysis(vec![TemplateElement {
            tag: "br".to_string(),
            is_self_closing: true,
            dynamic_classes: vec![],
            span: verter_span::Span::new(11, 17),
            content_end: 0,
            ..Default::default()
        }]);

        // Cursor on 'b' in <br /> — offset 12
        let pos = line_index.offset_to_position(12).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_none());
    }

    #[test]
    fn test_cursor_outside_tag_name() {
        let source = "<template>\n<div>hello</div>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let analysis = make_template_analysis(vec![TemplateElement {
            tag: "div".to_string(),
            is_self_closing: false,
            dynamic_classes: vec![],
            span: verter_span::Span::new(11, 27),
            content_end: 0,
            ..Default::default()
        }]);

        // Cursor on 'h' in "hello" — offset 16
        let pos = line_index.offset_to_position(16).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_none());
    }

    #[test]
    fn test_no_analysis_returns_none() {
        let source = "<template>\n<div>hello</div>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        let pos = line_index.offset_to_position(12).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, None, &line_index);
        assert!(result.is_none());
    }

    #[test]
    fn test_component_tag() {
        let source = "<template>\n<MyComponent>slot</MyComponent>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let line_index = LineIndex::new_utf16(source);

        // <MyComponent> starts at 11, </MyComponent> ends at 42
        let analysis = make_template_analysis(vec![TemplateElement {
            tag: "MyComponent".to_string(),
            is_component: true,
            is_self_closing: false,
            dynamic_classes: vec![],
            span: verter_span::Span::new(11, 42),
            content_end: 0,
            ..Default::default()
        }]);

        // Cursor on 'M' — offset 12
        let pos = line_index.offset_to_position(12).unwrap();
        let result = linked_editing_ranges(&pos, source, &blocks, Some(&analysis), &line_index);
        assert!(result.is_some());

        let ranges = result.unwrap().ranges;
        // Open: "MyComponent" at 12..23
        assert_eq!(ranges[0].start, line_index.offset_to_position(12).unwrap());
        assert_eq!(ranges[0].end, line_index.offset_to_position(23).unwrap());
        // Close: "MyComponent" at 30..41 (inside </MyComponent>)
        assert_eq!(ranges[1].start, line_index.offset_to_position(30).unwrap());
        assert_eq!(ranges[1].end, line_index.offset_to_position(41).unwrap());
    }
}
