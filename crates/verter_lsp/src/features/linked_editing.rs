//! Linked editing ranges projected from neutral nested-markup syntax.

use tower_lsp_server::ls_types::{LinkedEditingRanges, Position, Range};
use verter_language::parse_artifact::carrier_inventory::MarkupNodeKind;
use verter_session::carrier_publication_store::RegisteredFileStructure;

use crate::documents::line_index::LineIndex;

/// Return the exact opening/closing name spans for the element at `position`.
///
/// This feature is syntax-only: it reads the registered markup arena and does
/// not require semantic analysis or search source text for a closing delimiter.
pub fn linked_editing_ranges(
    position: &Position,
    structure: &RegisteredFileStructure,
    line_index: &LineIndex,
) -> Option<LinkedEditingRanges> {
    let offset = line_index.position_to_offset(position)?;

    for node in structure.inventory().markup().nodes() {
        let MarkupNodeKind::Element(element) = node.kind() else {
            continue;
        };
        if element.self_closing || element.void_element {
            continue;
        }
        let Some(closing_name) = element.closing_name_span else {
            continue;
        };
        let opening_name = element.opening_name_span;
        let on_opening = opening_name.start <= offset && offset <= opening_name.end;
        let on_closing = closing_name.start <= offset && offset <= closing_name.end;
        if !on_opening && !on_closing {
            continue;
        }

        return Some(LinkedEditingRanges {
            ranges: vec![
                Range {
                    start: line_index.offset_to_position(opening_name.start)?,
                    end: line_index.offset_to_position(opening_name.end)?,
                },
                Range {
                    start: line_index.offset_to_position(closing_name.start)?,
                    end: line_index.offset_to_position(closing_name.end)?,
                },
            ],
            word_pattern: None,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::carrier_structure::test_structure;

    fn ranges_at(source: &str, offset: u32) -> Option<LinkedEditingRanges> {
        let structure = test_structure(source, false);
        let line_index = LineIndex::new_utf16(source);
        linked_editing_ranges(
            &line_index.offset_to_position(offset)?,
            &structure,
            &line_index,
        )
    }

    #[test]
    fn nested_element_uses_exact_arena_name_spans_without_analysis() {
        let source = "<template>\n<div>hello</div>\n</template>";
        let line_index = LineIndex::new_utf16(source);
        let ranges = ranges_at(source, 12).expect("linked ranges").ranges;
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, line_index.offset_to_position(12).unwrap());
        assert_eq!(ranges[0].end, line_index.offset_to_position(15).unwrap());
        assert_eq!(ranges[1].start, line_index.offset_to_position(23).unwrap());
        assert_eq!(ranges[1].end, line_index.offset_to_position(26).unwrap());
    }

    #[test]
    fn closing_name_selects_the_same_pair() {
        let source = "<template>\n<div>hello</div>\n</template>";
        assert_eq!(ranges_at(source, 23).unwrap().ranges.len(), 2);
    }

    #[test]
    fn self_closing_and_content_positions_decline() {
        assert!(ranges_at("<template>\n<br />\n</template>", 12).is_none());
        assert!(ranges_at("<template>\n<div>hello</div>\n</template>", 16).is_none());
    }

    #[test]
    fn component_names_preserve_authored_case() {
        let source = "<template>\n<MyComponent>slot</MyComponent>\n</template>";
        let line_index = LineIndex::new_utf16(source);
        let ranges = ranges_at(source, 12).unwrap().ranges;
        assert_eq!(ranges[0].end, line_index.offset_to_position(23).unwrap());
        assert_eq!(ranges[1].start, line_index.offset_to_position(30).unwrap());
        assert_eq!(ranges[1].end, line_index.offset_to_position(41).unwrap());
    }
}
