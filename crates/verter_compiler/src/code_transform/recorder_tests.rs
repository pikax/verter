//! The deferred-edit law: what a recorded set of edits renders to, and which
//! correspondence each output byte keeps, whatever order the edits were
//! recorded in.

use oxc_allocator::Allocator;
use verter_span::Span;

use super::EditRecorder;
use crate::code_transform::{CodeTransform, CodeTransformError, MappingProduct, ProjectedClass};

const SOURCE: &str = "<a>{{x}}</a>";

/// One recorded edit.
#[derive(Clone, Copy)]
enum Edit {
    Replace(u32, u32, &'static str),
    ReplaceUnmapped(u32, u32, &'static str),
    Segmented(u32, u32, &'static str),
    Insert(u32, &'static str),
    InsertOrdered(u32, &'static str),
    RootPrefix(u32, &'static str),
    /// Reserve an ordered insertion; `Fill(n, ..)` fills the n-th reservation.
    Reserve(u32),
    Fill(usize, &'static str),
    Relocate(u32, u32, u32, &'static str, &'static str),
    Extract(u32, u32, u32, &'static str, &'static str),
}

fn record<'a>(allocator: &'a Allocator, edits: &[Edit]) -> EditRecorder<'a> {
    let mut recorder = EditRecorder::new(allocator);
    let mut reserved = Vec::new();
    for edit in edits {
        match *edit {
            Edit::Replace(start, end, text) => recorder.overwrite(start, end, text),
            Edit::ReplaceUnmapped(start, end, text) => {
                recorder.overwrite_unmapped(start, end, text);
            }
            Edit::Segmented(start, end, text) => {
                recorder.overwrite_segmented(start, end, text, &[]);
            }
            Edit::Insert(at, text) => recorder.prepend_alloc(at, text),
            Edit::InsertOrdered(at, text) => recorder.prepend_ordered_unmapped(at, text),
            Edit::RootPrefix(at, text) => recorder.overwrite_or_root_prefix(at, at, text),
            Edit::Reserve(at) => reserved.push(recorder.reserve_ordered_unmapped(at)),
            Edit::Fill(index, text) => recorder.fill_reserved(reserved[index], text),
            Edit::Relocate(start, end, target, prefix, suffix) => {
                recorder.relocate(start, end, target, prefix, suffix);
            }
            Edit::Extract(start, end, target, prefix, suffix) => {
                recorder.extract_original(start, end, target, prefix, suffix);
            }
        }
    }
    recorder
}

/// One letter per output byte: `I` identity, `L` relocated, `R` rewritten,
/// `S` synthesized.
fn classes(ct: &CodeTransform<'_>) -> String {
    let product = MappingProduct::of(ct).expect("mapped transform");
    (0..product.projected_len())
        .map(
            |offset| match product.projected_at(offset).map(|region| region.class) {
                Some(ProjectedClass::Identity) => 'I',
                Some(ProjectedClass::Relocated) => 'L',
                Some(ProjectedClass::Rewritten) => 'R',
                Some(ProjectedClass::Synthesized) => 'S',
                None => '?',
            },
        )
        .collect()
}

/// Apply `edits` to a fresh transform of [`SOURCE`]: the text and the
/// classes of its bytes, or the refusal.
fn resolve(edits: &[Edit]) -> Result<(String, String), CodeTransformError> {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(SOURCE, &allocator);
    record(&allocator, edits).apply(&mut ct)?;
    Ok((ct.build_string(), classes(&ct)))
}

#[test]
fn recorded_edits_resolve_by_one_law() {
    use Edit::*;
    type Expected = Result<(&'static str, &'static str), CodeTransformError>;
    let rows: Vec<(&str, Vec<Edit>, Expected)> = vec![
        (
            "plain replace",
            vec![Replace(3, 8, "_ctx.x")],
            Ok(("<a>_ctx.x</a>", "IIIRRRRRRIIII")),
        ),
        (
            "two inserts at one position keep call order",
            vec![Insert(5, "A"), Insert(5, "B")],
            Ok(("<a>{{ABx}}</a>", "IIIIISSIIIIIII")),
        ),
        (
            "plain before ordered at one position",
            vec![InsertOrdered(5, "O"), Insert(5, "P")],
            Ok(("<a>{{POx}}</a>", "IIIIISSIIIIIII")),
        ),
        (
            "root prefix first, the latest one first",
            vec![
                Insert(0, "P"),
                RootPrefix(0, "R"),
                InsertOrdered(0, "O"),
                RootPrefix(0, "S"),
            ],
            Ok(("SRPO<a>{{x}}</a>", "SSSSIIIIIIIIIIII")),
        ),
        (
            "insert at a replacement's start precedes it, at its end follows it",
            vec![Replace(3, 8, "X"), Insert(3, "["), Insert(8, "]")],
            Ok(("<a>[X]</a>", "IIISRSIIII")),
        ),
        (
            "contained replacement dropped",
            vec![Replace(4, 7, "Y"), Replace(3, 8, "X")],
            Ok(("<a>X</a>", "IIIRIIII")),
        ),
        (
            "contained replacement in another channel dropped",
            vec![ReplaceUnmapped(4, 7, "Y"), Replace(3, 8, "X")],
            Ok(("<a>X</a>", "IIIRIIII")),
        ),
        (
            "equal ranges: the first recorded wins",
            vec![Replace(3, 8, "X"), ReplaceUnmapped(3, 8, "Y")],
            Ok(("<a>X</a>", "IIIRIIII")),
        ),
        (
            "equal ranges, recorded the other way round",
            vec![ReplaceUnmapped(3, 8, "Y"), Replace(3, 8, "X")],
            Ok(("<a>Y</a>", "IIISIIII")),
        ),
        (
            "partial overlap",
            vec![Replace(3, 6, "X"), Replace(5, 8, "Y")],
            Err(CodeTransformError::Conflict {
                first: Span::new(3, 6),
                second: Span::new(5, 8),
            }),
        ),
        (
            "partial overlap across channels",
            vec![Segmented(5, 8, "Y"), ReplaceUnmapped(3, 6, "X")],
            Err(CodeTransformError::Conflict {
                first: Span::new(3, 6),
                second: Span::new(5, 8),
            }),
        ),
        (
            "reserved then filled lands between earlier and later ordered inserts",
            vec![
                InsertOrdered(5, "A"),
                Reserve(5),
                InsertOrdered(5, "C"),
                Fill(0, "B"),
            ],
            Ok(("<a>{{ABCx}}</a>", "IIIIISSSIIIIIII")),
        ),
        (
            "unfilled reservation emits nothing",
            vec![InsertOrdered(5, "A"), Reserve(5), InsertOrdered(5, "C")],
            Ok(("<a>{{ACx}}</a>", "IIIIISSIIIIIII")),
        ),
        (
            "relocate with prefix and suffix",
            vec![Relocate(3, 8, 0, "(", ")")],
            Ok(("({{x}})<a></a>", "SLLLLLSLLLIIII")),
        ),
        (
            "relocate carries a replacement inside its range",
            vec![Relocate(3, 8, 12, "", ""), Replace(5, 6, "y")],
            Ok(("<a></a>{{y}}", "IIIIIIILLRLL")),
        ),
        (
            "extract reads the authored text under an enclosing replacement",
            vec![Extract(5, 6, 0, "T=", ";"), Replace(3, 8, "P")],
            Ok(("T=x;<a>P</a>", "SSLSLLLRIIII")),
        ),
        (
            "edit outside the source",
            vec![Replace(13, 14, "Z")],
            Err(CodeTransformError::OutOfRange {
                offset: 13,
                len: 12,
            }),
        ),
    ];
    for (name, edits, expected) in rows {
        let actual = resolve(&edits);
        let expected = expected.map(|(text, classes)| (text.to_owned(), classes.to_owned()));
        assert_eq!(actual, expected, "{name}");
    }
}

#[test]
fn a_segmented_overwrite_over_replaced_text_is_refused_not_a_panic() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(SOURCE, &allocator);
    ct.overwrite(4, 5, "Q");
    let result = record(&allocator, &[Edit::Segmented(3, 8, "S")]).apply(&mut ct);
    assert_eq!(
        result,
        Err(CodeTransformError::SegmentedTargetNotLive {
            range: Span::new(3, 8),
        })
    );
}

#[test]
fn call_order_of_relocations_and_replacements_does_not_matter() {
    use Edit::*;
    let edits = [
        Replace(5, 6, "y"),
        Relocate(3, 8, 12, "(", ")"),
        Insert(9, "!"),
        Extract(9, 11, 0, "[", "]"),
    ];
    let mut reversed = edits;
    reversed.reverse();
    let forward = resolve(&edits).expect("valid edits");
    assert_eq!(resolve(&reversed).expect("valid edits"), forward);
    assert_eq!(forward.0, "[/a]<a><!/a>({{y}})");
}

#[test]
fn a_contained_replacement_in_one_batch_is_accepted_after_the_recorder_filters_it() {
    // The transform's checked batch refuses a contained pair; the recorder
    // resolves containment first, so the same edits apply.
    let (text, _) = resolve(&[Edit::Replace(3, 8, "X"), Edit::Replace(4, 6, "Y")])
        .expect("the contained replacement is dropped");
    assert_eq!(text, "<a>X</a>");
}

#[test]
fn conflicting_replacements_are_refused() {
    use crate::template::code_gen::types::CodeGenOutput;
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(SOURCE, &allocator);
    let mut out = CodeGenOutput::new(&allocator);
    out.overwrite(3, 6, "X");
    out.prepend_alloc(0, "P");
    out.edits.overwrite_unmapped(5, 8, "Y");
    assert!(matches!(
        out.apply_to(&mut ct),
        Err(CodeTransformError::Conflict { .. })
    ));
    // Nothing was applied: the transform still builds the source.
    assert_eq!(ct.build_string(), SOURCE);
}

#[test]
fn a_text_only_recorder_stores_no_anchor() {
    use crate::code_transform::{AnchorKind, SegmentAnchor};
    use crate::emission::{map_capture_events, reset_map_capture_events, MapMode};
    for (mode, captured) in [(MapMode::NoMap, false), (MapMode::Map, true)] {
        let allocator = Allocator::default();
        reset_map_capture_events();
        let mut recorder = EditRecorder::with_mode(&allocator, mode);
        let mut anchors = recorder.anchors();
        for offset in 0..100 {
            anchors.push(SegmentAnchor::new(offset, 1, offset, AnchorKind::Point));
        }
        recorder.prepend_alloc_mapped(1, 1, "x");
        recorder.add_sourcemap_location(2);
        assert_eq!(anchors.is_empty(), !captured, "{mode:?}");
        assert_eq!(map_capture_events() > 0, captured, "{mode:?}");
    }
}

#[test]
fn a_recorder_applies_only_to_a_transform_with_its_map_policy() {
    use crate::emission::MapMode;
    let allocator = Allocator::default();
    let mut ct = CodeTransform::<verter_audit::observe::Uncaptured>::with_mode_in(
        SOURCE,
        &allocator,
        MapMode::Map,
    );
    let recorder = EditRecorder::with_mode(&allocator, MapMode::NoMap);
    assert_eq!(
        recorder.apply(&mut ct),
        Err(CodeTransformError::MapNotCaptured)
    );
}

#[test]
fn an_insertion_off_a_character_or_past_the_source_is_refused_before_any_edit() {
    let source = "é";
    for (at, refusal) in [
        (1, CodeTransformError::MidChar { offset: 1 }),
        (3, CodeTransformError::OutOfRange { offset: 3, len: 2 }),
    ] {
        let allocator = Allocator::default();
        let mut ct = CodeTransform::new(source, &allocator);
        let mut recorder = EditRecorder::new(&allocator);
        recorder.overwrite(0, 2, "e");
        recorder.prepend_alloc(at, "X");
        assert_eq!(recorder.apply(&mut ct), Err(refusal));
        assert_eq!(ct.build_string(), source, "nothing was applied");
    }
}

#[test]
fn a_relocation_splitting_a_segmented_replacement_is_refused() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new("abcdef", &allocator);
    let result = record(
        &allocator,
        &[
            Edit::Segmented(1, 4, "XYZ"),
            Edit::Relocate(2, 5, 0, "", ""),
        ],
    )
    .apply(&mut ct);
    assert_eq!(
        result,
        Err(CodeTransformError::ReplacedContentSplit { offset: 2 })
    );
}

/// A helper-import preamble recorded on the recorder reaches the transform
/// on apply: the generated map reports the position right after it.
#[test]
fn a_recorded_helper_preamble_reports_its_end_boundary_after_apply() {
    let allocator = Allocator::default();
    let mut ct = CodeTransform::new(
        "const a = 1;
",
        &allocator,
    );
    let mut out = EditRecorder::new(&allocator);
    out.prepend_helper_preamble(
        0,
        None,
        "import x;
import y;
",
    );
    out.apply(&mut ct).expect("recorded edits apply");
    assert!(ct.build_string().starts_with(
        "import x;
import y;
"
    ));
    let json = ct
        .generate_map_json_with_preamble(
            crate::code_transform::SourceMapOptions::new()
                .with_source("App.vue")
                .include_content(true),
        )
        .expect("mapped transform");
    assert!(
        json.contains(r#""x_verter_helper_preamble_end":{"line":2,"character":0}"#),
        "the recorded preamble publishes its end boundary: {json}"
    );
}
