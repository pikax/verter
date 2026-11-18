use verter_span::Span;

use super::super::{
    map_capture_events, reset_map_capture_events, test_source, EmissionBuilder, EmissionError,
    EmissionFragment, MapMode, PieceText, Provenance,
};

fn text_of(fragment: &EmissionFragment<'_>) -> String {
    fragment
        .pieces()
        .iter()
        .map(|piece| fragment.piece_text(piece))
        .collect()
}

#[test]
fn a_fragment_reports_its_exact_length_without_walking() {
    let source = test_source("App.vue", "abcdef");
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(source);
    builder.push_source(Span::new(0, 2)).unwrap();
    builder.push_unmapped("é");
    builder.push_anchored("xyz", 4);
    let fragment = builder.finish().unwrap();
    assert_eq!(fragment.len(), 2 + 2 + 3);
    assert_eq!(text_of(&fragment), "abéxyz");

    // A push that would carry the output past `u32::MAX` bytes is refused
    // when the fragment is finished. The length is preset rather than built
    // from four gigabytes of text.
    let mut builder = EmissionBuilder::new(MapMode::NoMap);
    builder.len = u32::MAX - 2;
    builder.push_unmapped("ab");
    assert_eq!(builder.len(), u32::MAX);
    builder.push_unmapped("c");
    assert_eq!(builder.finish().err(), Some(EmissionError::Capacity));
}

#[test]
fn an_authored_slice_outside_its_source_is_refused() {
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(test_source("a", "abc"));
    assert_eq!(
        builder.push_source(Span::new(1, 4)),
        Err(EmissionError::InvalidSourceSlice {
            span: Span::new(1, 4)
        })
    );
    assert_eq!(
        builder.push_source(Span::new(2, 1)),
        Err(EmissionError::InvalidSourceSlice {
            span: Span::new(2, 1)
        })
    );
    // A builder with no source holds no authored slice at all.
    let mut sourceless = EmissionBuilder::new(MapMode::Map);
    assert_eq!(
        sourceless.push_source(Span::new(0, 0)),
        Err(EmissionError::InvalidSourceSlice {
            span: Span::new(0, 0)
        })
    );
    assert!(builder.finish().unwrap().pieces().is_empty());
}

#[test]
fn an_authored_slice_splitting_a_code_point_is_refused() {
    // "é" is two bytes: offset 2 is inside it.
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(test_source("a", "aéb"));
    assert_eq!(
        builder.push_source(Span::new(0, 2)),
        Err(EmissionError::InvalidSourceSlice {
            span: Span::new(0, 2)
        })
    );
    assert_eq!(builder.push_source(Span::new(0, 3)), Ok(()));
    assert_eq!(text_of(&builder.finish().unwrap()), "aé");
}

/// One push of every kind, under `mode`.
fn every_kind(mode: MapMode) -> EmissionFragment<'static> {
    let mut builder = EmissionBuilder::new(mode).with_source(test_source("App.vue", "let foo = 1"));
    builder.push_source(Span::new(0, 4)).unwrap();
    builder.push_rewritten("bar", Span::new(4, 7));
    // The mapping of the next piece stops after its first byte.
    builder.hint_unmapped_boundary(1);
    builder.push_anchored("_ctx.x", 8);
    builder.push_unmapped(";");
    builder.push_owned(
        String::from("owned"),
        Provenance::Anchor { source_offset: 10 },
    );
    builder
        .push_owned_ranges(String::from("(a)"), &[(1..2, 10)])
        .unwrap();
    builder.hint_location(2);
    builder.finish().unwrap()
}

#[test]
fn a_text_only_builder_records_no_anchor() {
    reset_map_capture_events();
    let fragment = every_kind(MapMode::NoMap);
    assert_eq!(text_of(&fragment), "let bar_ctx.x;owned(a)");
    assert!(fragment.pieces().iter().all(|piece| matches!(
        piece.provenance(),
        Provenance::Unmapped | Provenance::Exact { .. }
    )));
    assert_eq!(fragment.hints(), None);
    assert_eq!(map_capture_events(), 0);
}

#[test]
fn a_mapped_builder_keeps_the_four_kinds_distinct() {
    reset_map_capture_events();
    let fragment = every_kind(MapMode::Map);
    assert_eq!(text_of(&fragment), "let bar_ctx.x;owned(a)");
    let kinds: Vec<_> = fragment
        .pieces()
        .iter()
        .map(|piece| (fragment.piece_text(piece), piece.provenance()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("let ", Provenance::Exact { source_start: 0 }),
            (
                "bar",
                Provenance::Rewritten {
                    source: Span::new(4, 7)
                }
            ),
            ("_ctx.x", Provenance::Anchor { source_offset: 8 }),
            (";", Provenance::Unmapped),
            ("owned", Provenance::Anchor { source_offset: 10 }),
            ("(", Provenance::Unmapped),
            ("a", Provenance::Anchor { source_offset: 10 }),
            (")", Provenance::Unmapped),
        ]
    );
    let hints = fragment.hints().expect("a mapped fragment keeps its hints");
    assert_eq!(hints.locations(), &[2]);
    assert_eq!(hints.unmapped_boundaries(), &[8]);
    assert!(map_capture_events() > 0);
}

#[test]
fn an_owned_buffer_is_moved_not_copied() {
    let text = String::from("generated output");
    let address = text.as_ptr();
    let mut builder = EmissionBuilder::new(MapMode::NoMap);
    builder.push_owned(text, Provenance::Unmapped);
    let fragment = builder.finish().unwrap();
    let piece = &fragment.pieces()[0];
    assert!(matches!(piece.text(), PieceText::Owned(_)));
    assert_eq!(fragment.piece_text(piece).as_ptr(), address);
}

#[test]
fn an_exact_claim_over_other_bytes_is_refused() {
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(test_source("a", "let foo"));
    builder.push_owned(String::from("foo"), Provenance::Exact { source_start: 4 });
    builder.push_owned(String::from("bar"), Provenance::Exact { source_start: 4 });
    assert_eq!(
        builder.finish().err(),
        Some(EmissionError::InvalidSourceSlice {
            span: Span::new(4, 7)
        })
    );
}

#[test]
fn a_nested_fragment_moves_in_with_its_source() {
    let source = test_source("App.vue", "abcdef");
    let mut inner = EmissionBuilder::new(MapMode::Map).with_source(source.clone());
    inner.push_unmapped("<");
    inner.push_source(Span::new(2, 4)).unwrap();
    let inner = inner.finish().unwrap();

    let mut outer = EmissionBuilder::new(MapMode::Map);
    outer.push_unmapped("x");
    outer.append_fragment(inner).unwrap();
    outer.push_source(Span::new(0, 1)).unwrap();
    let fragment = outer.finish().unwrap();
    assert_eq!(text_of(&fragment), "x<cda");
    assert_eq!(fragment.len(), 5);
    assert!(fragment.source().unwrap().key.same(&source.key));

    let mut foreign = EmissionBuilder::new(MapMode::Map).with_source(test_source("Other.vue", "z"));
    let mut other = EmissionBuilder::new(MapMode::Map).with_source(source);
    other.push_source(Span::new(0, 1)).unwrap();
    assert_eq!(
        foreign.append_fragment(other.finish().unwrap()),
        Err(EmissionError::ForeignSource)
    );
    let text_only = EmissionBuilder::new(MapMode::NoMap).finish().unwrap();
    assert_eq!(
        foreign.append_fragment(text_only),
        Err(EmissionError::ModeMismatch)
    );
}

#[test]
fn a_builder_keeps_the_source_its_pieces_were_checked_against() {
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(test_source("A.vue", "a"));
    builder.push_owned(String::from("a"), Provenance::Exact { source_start: 0 });
    let rebound = builder.with_source(test_source("B.vue", "b"));
    assert_eq!(rebound.finish().err(), Some(EmissionError::ForeignSource));

    // Binding the same source again changes nothing.
    let source = test_source("A.vue", "a");
    let mut builder = EmissionBuilder::new(MapMode::Map).with_source(source.clone());
    builder.push_source(Span::new(0, 1)).unwrap();
    assert_eq!(builder.with_source(source).finish().unwrap().len(), 1);
}
