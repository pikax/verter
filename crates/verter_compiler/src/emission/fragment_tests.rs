use verter_span::Span;

use super::super::{test_source, EmissionBuilder, MapMode, MarkerId, Piece, PieceText, Provenance};

#[test]
fn a_marked_piece_is_a_slot_of_the_fragment_that_holds_it() {
    let marker = MarkerId::fresh();
    let source = test_source("App.vue", "const __sfc__ = {}");
    let mut inner = EmissionBuilder::new(MapMode::Map).with_source(source);
    inner.push_source(Span::new(0, 6)).unwrap();
    inner
        .push_marked(
            Piece::new(
                PieceText::Source(Span::new(6, 13)),
                Provenance::Exact { source_start: 6 },
            ),
            marker,
        )
        .unwrap();
    let inner = inner.finish().unwrap();
    let inner_slot = inner.slot(marker).expect("the marked piece is a slot");
    assert_eq!(inner_slot.piece(), 1);
    assert_eq!(inner.slot(MarkerId::fresh()), None);

    // Nested into another fragment, the marker names the same bytes there;
    // the inner fragment's slot does not name the outer fragment.
    let mut outer = EmissionBuilder::new(MapMode::Map);
    outer.push_unmapped("/* lead */");
    outer.append_fragment(inner).unwrap();
    let outer = outer.finish().unwrap();
    let outer_slot = outer.slot(marker).expect("nesting keeps the marker");
    assert_eq!(outer_slot.piece(), 2);
    assert_ne!(outer_slot.fragment(), inner_slot.fragment());
    assert_eq!(
        outer.piece_text(&outer.pieces()[outer_slot.piece() as usize]),
        "__sfc__"
    );
}
