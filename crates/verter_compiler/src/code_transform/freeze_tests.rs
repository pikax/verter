//! A frozen transform is the same output as a fragment of pieces that keep
//! what each run of bytes means, without copying text.

use oxc_allocator::Allocator;
use verter_span::Span;

use super::super::{AnchorKind, CodeTransform, CodeTransformError, EditRecorder, SegmentAnchor};
use super::MarkedFreezeError;
use crate::emission::{test_source, EmissionFragment, MapMode, MarkerId, PieceText, Provenance};

type TextTransform<'a> = CodeTransform<'a>;

fn text_of(fragment: &EmissionFragment<'_>) -> String {
    fragment
        .pieces()
        .iter()
        .map(|piece| fragment.piece_text(piece))
        .collect()
}

#[test]
fn a_frozen_transform_writes_what_build_string_builds() {
    let allocator = Allocator::default();
    let source = "let a = 1;\nlet b = 2;";
    let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    ct.prepend("// head\n");
    ct.overwrite(8, 9, "10");
    ct.prepend_left(11, "/* note */");
    ct.move_slice(11, 15, 0);
    ct.append("\n// tail");
    let built = ct.build_string();
    let fragment = ct.freeze(test_source("a.ts", source)).expect("own source");
    assert_eq!(text_of(&fragment), built);
    assert_eq!(fragment.len() as usize, built.len());
}

#[test]
fn each_chunk_keeps_its_correspondence_as_a_piece() {
    let allocator = Allocator::default();
    let source = "abcdefghijklmnop";
    let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    ct.overwrite(0, 1, "A");
    ct.overwrite_unmapped(2, 3, "C");
    ct.batch_prepend_left_with_source_map(&[(5, Some((4, 2)), "__e")]);
    let anchors = [SegmentAnchor::new(1, 1, 7, AnchorKind::Exact)];
    let correspondence = ct
        .check_correspondence("(h)", &anchors)
        .expect("the anchor copies the authored byte");
    ct.try_overwrite_segmented(7, 8, correspondence)
        .expect("one live chunk");
    ct.move_slice(10, 12, 16);
    let fragment = ct.freeze(test_source("a.ts", source)).expect("own source");

    let kinds: Vec<(String, Provenance, bool)> = fragment
        .pieces()
        .iter()
        .map(|piece| {
            (
                fragment.piece_text(piece).to_owned(),
                piece.provenance(),
                piece.relocated(),
            )
        })
        .collect();
    let has = |text: &str, provenance: Provenance, relocated: bool| {
        kinds.contains(&(text.to_owned(), provenance, relocated))
    };
    assert!(has(
        "A",
        Provenance::Rewritten {
            source: Span::new(0, 1)
        },
        false
    ));
    assert!(
        has("C", Provenance::Unmapped, false),
        "a synthetic overwrite stays unmapped"
    );
    assert!(has("__", Provenance::Unmapped, false));
    assert!(has("e", Provenance::Anchor { source_offset: 4 }, false));
    assert!(has("(", Provenance::Unmapped, false));
    assert!(has("h", Provenance::Exact { source_start: 7 }, false));
    assert!(has("kl", Provenance::Exact { source_start: 10 }, true));
}

#[test]
fn a_frozen_transform_copies_no_text() {
    let allocator = Allocator::default();
    let source = "<a>{{x}}</a>";
    let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    ct.overwrite(3, 8, "_ctx.x");
    ct.prepend("import x;\n");
    crate::emission::take_text_buffers();
    let fragment = ct.freeze(test_source("a.vue", source)).expect("own source");
    assert_eq!(
        crate::emission::take_text_buffers(),
        0,
        "no piece owns a copy"
    );
    for piece in fragment.pieces() {
        if let Provenance::Exact { .. } = piece.provenance() {
            assert!(matches!(piece.text(), PieceText::Source(_)));
        }
        assert!(!matches!(piece.text(), PieceText::Owned(_)));
    }
}

#[test]
fn freezing_a_text_only_transform_yields_no_map_data() {
    for (mode, captures) in [(MapMode::NoMap, false), (MapMode::Map, true)] {
        let allocator = Allocator::default();
        let source = "let a = 1;";
        crate::emission::reset_map_capture_events();
        let mut ct = TextTransform::with_mode_in(source, &allocator, mode);
        ct.overwrite(8, 9, "2");
        ct.try_add_sourcemap_location(4).expect("in range");
        ct.prepend_helper_preamble_content("// p\n");
        let fragment = ct.freeze(test_source("a.ts", source)).expect("own source");
        assert_eq!(fragment.hints().is_some(), captures, "{mode:?}");
        assert_eq!(
            crate::emission::map_capture_events() > 0,
            captures,
            "{mode:?}"
        );
        if !captures {
            assert!(fragment.pieces().iter().all(|piece| matches!(
                piece.provenance(),
                Provenance::Exact { .. } | Provenance::Unmapped
            )));
        }
    }
}

#[test]
fn a_transform_is_frozen_only_over_its_own_source() {
    let allocator = Allocator::default();
    let source = "abc";
    let other = String::from("abc");
    let ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    assert!(matches!(
        ct.freeze(test_source("a.ts", &other)),
        Err(CodeTransformError::ForeignSource)
    ));
}

/// One generated chunk holding three `__sfc__` references, the last inside
/// a terminal export statement.
const COMPONENT: &str = "const __sfc__ = {};\n__sfc__.a = 1;\nexport default __sfc__;\n";

#[test]
fn a_marked_run_becomes_its_own_slot_piece() {
    for mode in [MapMode::Map, MapMode::NoMap] {
        let allocator = Allocator::default();
        let source = "<x>let a = 1;";
        let mut ct = TextTransform::with_mode_in(source, &allocator, mode);
        let content = ct.alloc_str(COMPONENT);
        ct.overwrite_unmapped_alloc(0, 3, content);
        ct.overwrite(11, 12, "2");
        let mark = |start: u32, end: u32| {
            ct.mark_generated_sub_range(content, start, end)
                .expect("a range of the content")
        };
        let (first, second, export, in_export) =
            (mark(6, 13), mark(20, 27), mark(35, 59), mark(50, 57));
        let ids: Vec<MarkerId> = (0..4).map(|_| MarkerId::fresh()).collect();
        let built = ct.build_string();
        let fragment = ct
            .freeze_marked(
                test_source("a.ts", source),
                &[
                    (first, ids[0]),
                    (export, ids[2]),
                    (in_export, ids[3]),
                    (second, ids[1]),
                ],
            )
            .expect("every marker covers synthetic text");

        assert_eq!(text_of(&fragment), built, "{mode:?}");
        let slot_text = |id: MarkerId| {
            let slot = fragment.slot(id)?;
            let piece = &fragment.pieces()[slot.piece() as usize];
            assert_eq!(piece.provenance(), Provenance::Unmapped, "{mode:?}");
            Some(fragment.piece_text(piece).to_owned())
        };
        assert_eq!(slot_text(ids[0]).as_deref(), Some("__sfc__"), "{mode:?}");
        assert_eq!(slot_text(ids[1]).as_deref(), Some("__sfc__"), "{mode:?}");
        assert_eq!(
            slot_text(ids[2]).as_deref(),
            Some("export default __sfc__;\n"),
            "{mode:?}"
        );
        assert_eq!(
            slot_text(ids[3]),
            None,
            "a marker inside another is filled with it: {mode:?}"
        );
        // The authored text around the generated chunk keeps its pieces.
        assert!(fragment
            .pieces()
            .iter()
            .any(|piece| fragment.piece_text(piece) == "let a = "));
    }
}

#[test]
fn freezing_without_markers_matches_the_plain_freeze() {
    let allocator = Allocator::default();
    let source = "<x>let a = 1;";
    let build = || {
        let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
        let content = ct.alloc_str(COMPONENT);
        ct.overwrite_unmapped_alloc(0, 3, content);
        ct.overwrite(11, 12, "2");
        ct
    };
    let plain = build().freeze(test_source("a.ts", source)).unwrap();
    let marked = build()
        .freeze_marked(test_source("a.ts", source), &[])
        .unwrap();
    assert_eq!(plain.pieces(), marked.pieces());
}

#[test]
fn a_marker_in_a_replacement_keeps_its_correspondence() {
    let allocator = Allocator::default();
    let source = "export default {};";
    let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    let mut recorder = EditRecorder::with_mode(&allocator, MapMode::Map);
    let content = recorder.alloc_str("const __sfc__ =");
    recorder.overwrite_alloc(0, 14, content);
    recorder.apply(&mut ct).expect("one replacement");
    let marker = ct.mark_generated_sub_range(content, 6, 13).unwrap();
    let id = MarkerId::fresh();
    let built = ct.build_string();
    let fragment = ct
        .freeze_marked(test_source("a.ts", source), &[(marker, id)])
        .expect("a replacement can hold a slot");
    assert_eq!(text_of(&fragment), built);
    let replaced = Provenance::Rewritten {
        source: Span::new(0, 14),
    };
    let runs: Vec<(String, Provenance, bool)> = fragment
        .pieces()
        .iter()
        .map(|piece| {
            (
                fragment.piece_text(piece).to_owned(),
                piece.provenance(),
                piece.marker() == Some(id),
            )
        })
        .collect();
    assert_eq!(
        runs[..3],
        [
            ("const ".to_owned(), replaced, false),
            ("__sfc__".to_owned(), replaced, true),
            (" =".to_owned(), replaced, false),
        ],
        "every part keeps standing for the replaced span"
    );
}

#[test]
fn a_marker_no_chunk_holds_is_refused() {
    let allocator = Allocator::default();
    let source = "abc";
    let ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    let stray = ct.alloc_str("__sfc__");
    let marker = ct.mark_generated_sub_range(stray, 0, 7).unwrap();
    let id = MarkerId::fresh();
    assert_eq!(
        ct.freeze_marked(test_source("a.ts", source), &[(marker, id)])
            .unwrap_err(),
        MarkedFreezeError::UnresolvedMarker(id)
    );
}

#[test]
fn partly_overlapping_markers_are_refused() {
    let allocator = Allocator::default();
    let source = "<x>";
    let mut ct = TextTransform::with_mode_in(source, &allocator, MapMode::Map);
    let content = ct.alloc_str("abcdef");
    ct.overwrite_unmapped_alloc(0, 3, content);
    let left = ct.mark_generated_sub_range(content, 0, 4).unwrap();
    let right = ct.mark_generated_sub_range(content, 2, 6).unwrap();
    let (left_id, right_id) = (MarkerId::fresh(), MarkerId::fresh());
    assert_eq!(
        ct.freeze_marked(
            test_source("a.ts", source),
            &[(left, left_id), (right, right_id)]
        )
        .unwrap_err(),
        MarkedFreezeError::OverlappingMarkers(right_id)
    );
}

#[test]
fn a_recorded_relocation_keeps_the_generated_suffix_marker() {
    for mode in [MapMode::Map, MapMode::NoMap] {
        let allocator = Allocator::default();
        let source = "ABCDEF";
        let mut recorder = EditRecorder::with_mode(&allocator, mode);
        let suffix = recorder.alloc_str("[]");
        recorder.relocate_allocated(2, 4, 0, "", suffix);
        let mut transform = TextTransform::with_mode_in(source, &allocator, mode);
        let marker = transform.mark_generated_sub_range(suffix, 0, 2).unwrap();
        recorder.apply(&mut transform).unwrap();
        let id = MarkerId::fresh();
        let fragment = transform
            .freeze_marked(test_source("a.ts", source), &[(marker, id)])
            .unwrap();
        assert_eq!(text_of(&fragment), "CD[]ABEF");
        let slot = fragment
            .slot(id)
            .expect("the relocated suffix is the registered allocation");
        assert_eq!(
            fragment.piece_text(&fragment.pieces()[slot.piece() as usize]),
            "[]"
        );
    }
}
